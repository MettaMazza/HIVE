//! Wallet Tool — HIVE Coin wallet management for the agent.
//!
//! Admin-only: Only instance owners (admins) can create and use wallet features.
//! Platform-agnostic: Works across Discord, mesh network, glasses, web.
//!
//! Actions: create, balance, send, receive, history, mint
//!
//! Description tags:
//!   action:[create|balance|send|receive|history|mint]
//!   user_id:[platform_user_id]
//!   to:[recipient_pubkey_or_user_id]
//!   amount:[number]

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::mpsc;
use crate::models::tool::{ToolResult, ToolStatus};
use crate::models::scope::Scope;
use crate::crypto::keystore::{Keystore, WalletRole};
use crate::crypto::solana::HiveSolanaClient;

/// Transaction deduplication state — prevents double-sends.
struct DeduplicationEntry {
    to: String,
    amount: f64,
    timestamp: std::time::Instant,
}

// Thread-safe deduplication store (Rust 2024 safe)
static DEDUP_MAP: std::sync::OnceLock<tokio::sync::RwLock<HashMap<String, DeduplicationEntry>>> = std::sync::OnceLock::new();

fn get_dedup() -> &'static tokio::sync::RwLock<HashMap<String, DeduplicationEntry>> {
    DEDUP_MAP.get_or_init(|| tokio::sync::RwLock::new(HashMap::new()))
}

/// Execute a wallet operation.
pub async fn execute_wallet(
    task_id: String,
    desc: String,
    scope: &Scope,
    keystore: Arc<Keystore>,
    solana: Arc<HiveSolanaClient>,
    capabilities: Option<Arc<crate::models::capabilities::AgentCapabilities>>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    // ── Admin gate ─────────────────────────────────────────────────
    let invoker_id = match scope {
        Scope::Public { user_id, .. } => user_id.clone(),
        Scope::Private { user_id } => user_id.clone(),
    };

    let is_admin = capabilities.as_ref().map_or(false, |c| c.admin_users.contains(&invoker_id));
    // NOTE: is_system bypass REMOVED. In an open-source codebase, any admin could
    // instruct Apis to act as "apis_system" — making System a proxy for admin.
    // All tool-facing operations gate on is_admin. Minting additionally requires
    // the creator key file to exist on disk (physical possession).

    if !is_admin {
        return ToolResult {
            task_id,
            output: "⛔ Wallet operations are restricted to administrators only. You must be an admin of this HIVE instance to use wallet features.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Not admin".into()),
        };
    }

    let action = extract_tag(&desc, "action:").unwrap_or_else(|| "balance".into());

    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("🪙 Wallet: `{}` | user: {}\n", action, invoker_id)).await;
    }

    match action.as_str() {
        "create" => execute_create(task_id, &desc, &invoker_id, &keystore, &solana).await,
        "balance" => execute_balance(task_id, &desc, &invoker_id, &keystore, &solana).await,
        "send" => execute_send(task_id, &desc, &invoker_id, &keystore, &solana).await,
        "receive" => execute_receive(task_id, &invoker_id, &keystore).await,
        "history" => execute_history(task_id, &desc, &invoker_id, &keystore, &solana).await,
        "mint" => {
            // CREATOR KEY REQUIRED — physical key file must exist on disk.
            // This cannot be bypassed by any user, admin, or the agent itself.
            if !crate::network::creator_key::creator_key_exists() {
                return ToolResult {
                    task_id,
                    output: "⛔ Mint denied. The Creator Key file must be present on this machine to mint HIVE Coin. This is a physical possession requirement — no role or instruction can bypass it.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Creator key not found".into()),
                };
            }
            execute_mint(task_id, &desc, &invoker_id, &keystore, &solana).await
        }
        _ => ToolResult {
            task_id,
            output: format!("Unknown wallet action: '{}'. Use create, balance, send, receive, history, or mint.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        },
    }
}

// ─── CREATE WALLET ────────────────────────────────────────────────

async fn execute_create(
    task_id: String,
    desc: &str,
    invoker_id: &str,
    keystore: &Keystore,
    solana: &HiveSolanaClient,
) -> ToolResult {
    let wallet_id = extract_tag(desc, "user_id:").unwrap_or_else(|| invoker_id.to_string());

    if keystore.wallet_exists(&wallet_id) {
        let pubkey = keystore.get_public_key(&wallet_id).unwrap_or_default();
        return ToolResult {
            task_id,
            output: format!("Wallet already exists for '{}'.\n\n🔑 **Address:** `{}`", wallet_id, pubkey),
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    let role = if wallet_id.starts_with("apis_") {
        WalletRole::System
    } else {
        WalletRole::User
    };

    match keystore.create_wallet(&wallet_id, role) {
        Ok(pubkey) => {
            // Auto-airdrop SOL on devnet/simulation for transaction fees
            if solana.is_configured() {
                let _ = solana.request_airdrop(&pubkey, 0.1);
            }

            ToolResult {
                task_id,
                output: format!(
                    "✅ **Wallet created successfully!**\n\n\
                    🔑 **Address:** `{}`\n\
                    👤 **Owner:** {}\n\
                    🌐 **Mode:** {}\n\n\
                    Your wallet is ready to receive HIVE Coin.",
                    pubkey, wallet_id,
                    if matches!(solana.mode(), crate::crypto::solana::WalletMode::Simulation) {
                        "Simulation (local ledger)"
                    } else {
                        "Live (Solana blockchain)"
                    }
                ),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => ToolResult {
            task_id,
            output: format!("Failed to create wallet: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e),
        },
    }
}

// ─── CHECK BALANCE ────────────────────────────────────────────────

async fn execute_balance(
    task_id: String,
    desc: &str,
    invoker_id: &str,
    keystore: &Keystore,
    solana: &HiveSolanaClient,
) -> ToolResult {
    let wallet_id = extract_tag(desc, "user_id:").unwrap_or_else(|| invoker_id.to_string());

    let pubkey = match keystore.get_public_key(&wallet_id) {
        Some(pk) => pk,
        None => return ToolResult {
            task_id,
            output: format!("No wallet found for '{}'. Use action:[create] to create one.", wallet_id),
            tokens_used: 0,
            status: ToolStatus::Failed("No wallet".into()),
        },
    };

    match solana.get_balance(&pubkey) {
        Ok(balance) => ToolResult {
            task_id,
            output: format!(
                "💰 **Wallet Balance**\n\n\
                👤 **Owner:** {}\n\
                🔑 **Address:** `{}`\n\
                🪙 **HIVE:** {:.2}\n\
                ◎ **SOL:** {:.4}\n",
                wallet_id, pubkey, balance.hive, balance.sol,
            ),
            tokens_used: 0,
            status: ToolStatus::Success,
        },
        Err(e) => ToolResult {
            task_id,
            output: format!("Failed to check balance: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e),
        },
    }
}

// ─── SEND HIVE ────────────────────────────────────────────────────

async fn execute_send(
    task_id: String,
    desc: &str,
    invoker_id: &str,
    keystore: &Keystore,
    solana: &HiveSolanaClient,
) -> ToolResult {
    let from_id = extract_tag(desc, "user_id:").unwrap_or_else(|| invoker_id.to_string());
    let to_raw = match extract_tag(desc, "to:") {
        Some(t) => t,
        None => return ToolResult {
            task_id,
            output: "Error: 'to:' tag required. Use 'to:[recipient_address_or_user_id]'".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing recipient".into()),
        },
    };
    let amount: f64 = match extract_tag(desc, "amount:").and_then(|v| v.parse().ok()) {
        Some(a) if a > 0.0 => a,
        _ => return ToolResult {
            task_id,
            output: "Error: 'amount:' tag required with a positive number. Use 'amount:[50]'".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Invalid amount".into()),
        },
    };

    // Resolve recipient — could be a user_id or a raw pubkey
    let to_pubkey = if let Some(pk) = keystore.get_public_key(&to_raw) {
        pk
    } else {
        to_raw.clone() // Assume it's a raw Solana address
    };

    // ── Deduplication guard ───────────────────────────────────────
    {
        let dedup = get_dedup();
        let map = dedup.read().await;
        if let Some(entry) = map.get(&from_id) {
            if entry.to == to_pubkey
                && (entry.amount - amount).abs() < f64::EPSILON
                && entry.timestamp.elapsed().as_secs() < 30
            {
                return ToolResult {
                    task_id,
                    output: format!(
                        "⚠️ **Duplicate transaction blocked!**\n\n\
                        You already sent {:.2} HIVE to `{}` {} seconds ago.\n\
                        Wait 30 seconds before sending the same amount to the same recipient.",
                        amount, to_pubkey, entry.timestamp.elapsed().as_secs()
                    ),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Duplicate blocked".into()),
                };
            }
        }
    }

    // Execute transfer
    match solana.transfer_hive(keystore, &from_id, &to_pubkey, amount) {
        Ok(sig) => {
            // Record in dedup map
            {
                let dedup = get_dedup();
                let mut map = dedup.write().await;
                map.insert(from_id.clone(), DeduplicationEntry {
                    to: to_pubkey.clone(),
                    amount,
                    timestamp: std::time::Instant::now(),
                });
            }

            ToolResult {
                task_id,
                output: format!(
                    "✅ **Transfer successful!**\n\n\
                    📤 **From:** {}\n\
                    📥 **To:** `{}`\n\
                    🪙 **Amount:** {:.2} HIVE\n\
                    📝 **Transaction:** `{}`",
                    from_id, to_pubkey, amount, sig
                ),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => ToolResult {
            task_id,
            output: format!("❌ Transfer failed: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e),
        },
    }
}

// ─── RECEIVE (SHOW ADDRESS) ──────────────────────────────────────

async fn execute_receive(
    task_id: String,
    invoker_id: &str,
    keystore: &Keystore,
) -> ToolResult {
    let pubkey = match keystore.get_public_key(invoker_id) {
        Some(pk) => pk,
        None => return ToolResult {
            task_id,
            output: format!("No wallet found for '{}'. Use action:[create] first.", invoker_id),
            tokens_used: 0,
            status: ToolStatus::Failed("No wallet".into()),
        },
    };

    ToolResult {
        task_id,
        output: format!(
            "📬 **Your Wallet Address**\n\n\
            🔑 `{}`\n\n\
            Share this address to receive HIVE Coin from other users.",
            pubkey
        ),
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

// ─── TRANSACTION HISTORY ─────────────────────────────────────────

async fn execute_history(
    task_id: String,
    desc: &str,
    invoker_id: &str,
    keystore: &Keystore,
    solana: &HiveSolanaClient,
) -> ToolResult {
    let wallet_id = extract_tag(desc, "user_id:").unwrap_or_else(|| invoker_id.to_string());
    let limit: usize = extract_tag(desc, "limit:").and_then(|v| v.parse().ok()).unwrap_or(10);

    let pubkey = match keystore.get_public_key(&wallet_id) {
        Some(pk) => pk,
        None => return ToolResult {
            task_id,
            output: format!("No wallet found for '{}'.", wallet_id),
            tokens_used: 0,
            status: ToolStatus::Failed("No wallet".into()),
        },
    };

    match solana.get_transaction_history(&pubkey, limit) {
        Ok(records) => {
            if records.is_empty() {
                return ToolResult {
                    task_id,
                    output: format!("📜 No transactions found for '{}'.", wallet_id),
                    tokens_used: 0,
                    status: ToolStatus::Success,
                };
            }
            let mut output = format!("📜 **Transaction History** ({})\n\n", wallet_id);
            for (i, tx) in records.iter().enumerate() {
                output.push_str(&format!(
                    "{}. **{}** | {:.2} HIVE | {} → {} | `{}`\n",
                    i + 1, tx.status, tx.amount, tx.from, tx.to,
                    &tx.id[..tx.id.len().min(16)]
                ));
            }
            ToolResult {
                task_id,
                output,
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => ToolResult {
            task_id,
            output: format!("Failed to get history: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e),
        },
    }
}

// ─── MINT (CREATOR/SYSTEM ONLY) ──────────────────────────────────

async fn execute_mint(
    task_id: String,
    desc: &str,
    invoker_id: &str,
    keystore: &Keystore,
    solana: &HiveSolanaClient,
) -> ToolResult {
    let to_raw = match extract_tag(desc, "to:") {
        Some(t) => t,
        None => return ToolResult {
            task_id,
            output: "Error: 'to:' tag required. Use 'to:[recipient_address_or_user_id]'".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing recipient".into()),
        },
    };
    let amount: f64 = match extract_tag(desc, "amount:").and_then(|v| v.parse().ok()) {
        Some(a) if a > 0.0 => a,
        _ => return ToolResult {
            task_id,
            output: "Error: 'amount:' tag required with a positive number.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Invalid amount".into()),
        },
    };

    // Resolve recipient
    let to_pubkey = if let Some(pk) = keystore.get_public_key(&to_raw) {
        pk
    } else {
        to_raw.clone()
    };

    // The mint authority is the system wallet (apis_system) or creator
    let mint_authority = if keystore.wallet_exists("apis_system") {
        "apis_system"
    } else {
        invoker_id
    };

    // Phase 5: Mint authorization hardening — ONLY Creator role can mint.
    // System role is NOT sufficient — in an open-source codebase, System is just
    // a string that any admin could instruct the agent to use as a proxy.
    // The creator_key_exists() check at the tool entry point is the real gate.
    let invoker_role = keystore.get_role(mint_authority);
    match invoker_role {
        Some(WalletRole::Creator) => {
            // Authorized — proceed
        }
        _ => {
            return ToolResult {
                task_id,
                output: "⛔ Mint authority denied. Only Creator or System wallets can mint HIVE Coin. Admin wallets cannot mint — this is by design to prevent inflation.".into(),
                tokens_used: 0,
                status: ToolStatus::Failed("Mint authority denied".into()),
            };
        }
    }

    match solana.mint_hive(keystore, mint_authority, &to_pubkey, amount) {
        Ok(sig) => ToolResult {
            task_id,
            output: format!(
                "🪙 **Minted successfully!**\n\n\
                📥 **To:** `{}`\n\
                🪙 **Amount:** {:.2} HIVE\n\
                📝 **Transaction:** `{}`\n\
                📊 **Total Supply:** {:.2} HIVE",
                to_pubkey, amount, sig, solana.total_supply()
            ),
            tokens_used: 0,
            status: ToolStatus::Success,
        },
        Err(e) => ToolResult {
            task_id,
            output: format!("❌ Mint failed: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e),
        },
    }
}

// ─── TAG EXTRACTION ──────────────────────────────────────────────

fn extract_tag(text: &str, tag: &str) -> Option<String> {
    crate::agent::preferences::extract_tag(text, tag)
}

#[cfg(test)]
#[path = "wallet_tool_tests.rs"]
mod tests;
