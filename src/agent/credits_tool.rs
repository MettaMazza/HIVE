//! Credits Tool — Query and manage credit accounts for HIVE agents.
//!
//! Actions: balance, history, earn, spend, leaderboard, stats, reputation
//!
//! Description tags:
//!   action:[balance|history|earn|spend|leaderboard|stats|reputation]
//!   limit:[number]  (for history)
//!   amount:[number]  (for earn/spend)
//!   source:[tag]  (for earn, identifies the earning source)
//!
//! Permissions:
//!   - balance, history, leaderboard, stats, reputation: Available to all users
//!   - earn: Admin only (prevents gaming the system)
//!   - spend: Available to all users (they spend their own credits)

use std::sync::Arc;
use tokio::sync::mpsc;
use crate::models::tool::{ToolResult, ToolStatus};
use crate::models::scope::Scope;
use crate::crypto::credits::CreditsEngine;

/// Execute a credits operation.
pub async fn execute_credits(
    task_id: String,
    desc: String,
    scope: &Scope,
    credits_engine: Arc<CreditsEngine>,
    capabilities: Option<Arc<crate::models::capabilities::AgentCapabilities>>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    // Extract user identity from scope
    let invoker_id = match scope {
        Scope::Public { user_id, .. } => user_id.clone(),
        Scope::Private { user_id } => user_id.clone(),
    };

    // Determine if invoker is admin
    let is_admin = capabilities.as_ref().map_or(false, |c| c.admin_users.contains(&invoker_id));
    // NOTE: is_system bypass REMOVED. See wallet_tool.rs for rationale.

    // Extract action (default to balance)
    let action = extract_tag(&desc, "action:").unwrap_or_else(|| "balance".into());

    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("💰 Credits: `{}` | user: {}\n", action, invoker_id)).await;
    }

    match action.as_str() {
        "balance" => execute_balance(task_id, &invoker_id, &credits_engine).await,
        "history" => execute_history(task_id, &desc, &invoker_id, &credits_engine).await,
        "earn" => {
            if !is_admin {
                return ToolResult {
                    task_id,
                    output: "⛔ Earning credits is restricted to administrators only. This prevents gaming the system.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Not admin".into()),
                };
            }
            execute_earn(task_id, &desc, &invoker_id, &credits_engine).await
        }
        "spend" => execute_spend(task_id, &desc, &invoker_id, &credits_engine).await,
        "leaderboard" => execute_leaderboard(task_id, &desc, &credits_engine).await,
        "stats" => execute_stats(task_id, &credits_engine).await,
        "reputation" => execute_reputation(task_id, &invoker_id, &credits_engine).await,
        _ => ToolResult {
            task_id,
            output: format!(
                "Unknown credits action: '{}'. Use balance, history, earn (admin), spend, leaderboard, stats, or reputation.",
                action
            ),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        },
    }
}

// ─── BALANCE ──────────────────────────────────────────────────────────

async fn execute_balance(task_id: String, invoker_id: &str, credits_engine: &CreditsEngine) -> ToolResult {
    let account = credits_engine.get_or_create_account(invoker_id);

    ToolResult {
        task_id,
        output: format!(
            "💰 **Credit Balance**\n\n\
            👤 **User:** {}\n\
            💎 **Balance:** {:.2} credits\n\
            📈 **Lifetime Earned:** {:.2} credits\n\
            📉 **Lifetime Spent:** {:.2} credits\n\
            ⭐ **Reputation:** {:.2}/1.0\n\
            🔥 **Streak:** {} days",
            invoker_id,
            account.balance,
            account.lifetime_earned,
            account.lifetime_spent,
            account.reputation_score,
            account.contribution_streak,
        ),
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

// ─── TRANSACTION HISTORY ──────────────────────────────────────────────

async fn execute_history(
    task_id: String,
    desc: &str,
    invoker_id: &str,
    credits_engine: &CreditsEngine,
) -> ToolResult {
    let limit: usize = extract_tag(desc, "limit:")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);

    let transactions = credits_engine.history(invoker_id, limit);

    if transactions.is_empty() {
        return ToolResult {
            task_id,
            output: format!("📜 No credit transactions found for '{}'.", invoker_id),
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    let mut output = format!("📜 **Credit History** ({}, limit {})\n\n", invoker_id, limit);
    for (i, tx) in transactions.iter().enumerate() {
        let symbol = if tx.amount >= 0.0 { "✅" } else { "💳" };
        let formatted_amount = format!("{:+.2}", tx.amount);
        output.push_str(&format!(
            "{}. {} **{}** | Balance: {:.2}\n   └─ {}\n",
            i + 1,
            symbol,
            formatted_amount,
            tx.balance_after,
            tx.description
        ));
    }

    ToolResult {
        task_id,
        output,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

// ─── EARN (ADMIN ONLY) ────────────────────────────────────────────────

async fn execute_earn(
    task_id: String,
    desc: &str,
    invoker_id: &str,
    credits_engine: &CreditsEngine,
) -> ToolResult {
    let amount: f64 = match extract_tag(desc, "amount:")
        .and_then(|v| v.parse().ok())
    {
        Some(a) if a > 0.0 => a,
        _ => {
            return ToolResult {
                task_id,
                output: "Error: 'amount:' tag required with a positive number. Use 'amount:[50]'".into(),
                tokens_used: 0,
                status: ToolStatus::Failed("Invalid amount".into()),
            }
        }
    };

    let source_tag = extract_tag(desc, "source:").unwrap_or_else(|| "manual".into());

    // Parse the source tag and create appropriate CreditSource
    use crate::crypto::credits::CreditSource;
    let credit_source = CreditSource::SystemAdjustment {
        reason: format!("Manual adjustment by {} ({})", invoker_id, source_tag),
    };

    match credits_engine.earn(invoker_id, credit_source, amount) {
        Ok(tx) => {
            let account = credits_engine.account(invoker_id).unwrap();
            ToolResult {
                task_id,
                output: format!(
                    "✅ **Credits awarded successfully!**\n\n\
                    👤 **User:** {}\n\
                    💎 **Amount:** +{:.2} credits\n\
                    💰 **New Balance:** {:.2} credits\n\
                    📝 **Transaction:** `{}`",
                    invoker_id, amount, account.balance, tx.id
                ),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => ToolResult {
            task_id,
            output: format!("❌ Failed to award credits: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e),
        },
    }
}

// ─── SPEND ────────────────────────────────────────────────────────────

async fn execute_spend(
    task_id: String,
    desc: &str,
    invoker_id: &str,
    credits_engine: &CreditsEngine,
) -> ToolResult {
    let amount: f64 = match extract_tag(desc, "amount:")
        .and_then(|v| v.parse().ok())
    {
        Some(a) if a > 0.0 => a,
        _ => {
            return ToolResult {
                task_id,
                output: "Error: 'amount:' tag required with a positive number. Use 'amount:[25]'".into(),
                tokens_used: 0,
                status: ToolStatus::Failed("Invalid amount".into()),
            }
        }
    };

    let service = extract_tag(desc, "service:")
        .unwrap_or_else(|| "marketplace_purchase".into());

    match credits_engine.spend(invoker_id, &service, amount) {
        Ok(tx) => {
            let account = credits_engine.account(invoker_id).unwrap();
            ToolResult {
                task_id,
                output: format!(
                    "✅ **Transaction successful!**\n\n\
                    👤 **User:** {}\n\
                    💳 **Amount:** -{:.2} credits\n\
                    🛒 **Service:** {}\n\
                    💰 **New Balance:** {:.2} credits\n\
                    📝 **Transaction:** `{}`",
                    invoker_id, amount, service, account.balance, tx.id
                ),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => ToolResult {
            task_id,
            output: format!("❌ Transaction failed: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e),
        },
    }
}

// ─── LEADERBOARD ──────────────────────────────────────────────────────

async fn execute_leaderboard(
    task_id: String,
    desc: &str,
    credits_engine: &CreditsEngine,
) -> ToolResult {
    let limit: usize = extract_tag(desc, "limit:")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);

    let leaderboard = credits_engine.leaderboard(limit);

    if leaderboard.is_empty() {
        return ToolResult {
            task_id,
            output: "📊 No credit earners yet. Be the first to earn credits!".into(),
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    let mut output = format!("🏆 **Credit Leaderboard** (Top {})\n\n", limit);
    for (i, (peer_id, earned, reputation)) in leaderboard.iter().enumerate() {
        let medal = match i {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "  ",
        };
        output.push_str(&format!(
            "{}. {} **{:.2}** credits | Reputation: {:.2}\n   └─ {}\n",
            i + 1,
            medal,
            earned,
            reputation,
            &peer_id[..peer_id.len().min(20)]
        ));
    }

    ToolResult {
        task_id,
        output,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

// ─── SYSTEM STATS ──────────────────────────────────────────────────────

async fn execute_stats(task_id: String, credits_engine: &CreditsEngine) -> ToolResult {
    let stats = credits_engine.stats();

    let total_accounts = stats["total_accounts"].as_u64().unwrap_or(0);
    let total_issued = stats["total_credits_issued"].as_f64().unwrap_or(0.0);
    let total_spent = stats["total_credits_spent"].as_f64().unwrap_or(0.0);
    let total_circulation = stats["total_credits_in_circulation"].as_f64().unwrap_or(0.0);
    let total_txs = stats["total_transactions"].as_u64().unwrap_or(0);
    let avg_reputation = stats["average_reputation"].as_str().unwrap_or("0.00");

    let output = format!(
        "📊 **Credits System Statistics**\n\n\
        👥 **Total Accounts:** {}\n\
        💎 **Total Issued:** {:.2} credits\n\
        💳 **Total Spent:** {:.2} credits\n\
        💰 **In Circulation:** {:.2} credits\n\
        📜 **Total Transactions:** {}\n\
        ⭐ **Average Reputation:** {}\n\n\
        ⚙️ **Configuration:**\n\
        • Welcome bonus: {:.2}\n\
        • Compute earn rate: {:.2}/1K tokens\n\
        • Network earn rate: {:.2}/100 requests\n\
        • Idle earn rate: {:.2}/hour\n\
        • High demand multiplier: {:.2}x",
        total_accounts,
        total_issued,
        total_spent,
        total_circulation,
        total_txs,
        avg_reputation,
        stats["config"]["welcome_bonus"].as_f64().unwrap_or(0.0),
        stats["config"]["compute_earn_per_1k"].as_f64().unwrap_or(0.0),
        stats["config"]["network_earn_per_100"].as_f64().unwrap_or(0.0),
        stats["config"]["idle_earn_per_hour"].as_f64().unwrap_or(0.0),
        stats["config"]["high_demand_multiplier"].as_f64().unwrap_or(0.0),
    );

    ToolResult {
        task_id,
        output,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

// ─── REPUTATION SCORE ─────────────────────────────────────────────────

async fn execute_reputation(
    task_id: String,
    invoker_id: &str,
    credits_engine: &CreditsEngine,
) -> ToolResult {
    let account = credits_engine.get_or_create_account(invoker_id);

    let reputation_bar = if account.reputation_score >= 0.8 {
        "████████████████████ Excellent"
    } else if account.reputation_score >= 0.6 {
        "████████████ Good"
    } else if account.reputation_score >= 0.4 {
        "████████ Fair"
    } else if account.reputation_score >= 0.2 {
        "████ Poor"
    } else {
        "██ Very Poor"
    };

    ToolResult {
        task_id,
        output: format!(
            "⭐ **Reputation Score**\n\n\
            👤 **User:** {}\n\
            📊 **Score:** {:.2}/1.0\n\
            📈 **Status:** {}\n\
            💎 **Balance:** {:.2} credits\n\
            🔥 **Streak:** {} days\n\
            💬 **Earned:** {:.2} total\n\
            📝 **Created:** {}",
            invoker_id,
            account.reputation_score,
            reputation_bar,
            account.balance,
            account.contribution_streak,
            account.lifetime_earned,
            &account.created_at[..10],
        ),
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

// ─── TAG EXTRACTION ──────────────────────────────────────────────────

fn extract_tag(text: &str, tag: &str) -> Option<String> {
    crate::agent::preferences::extract_tag(text, tag)
}

#[cfg(test)]
#[path = "credits_tool_tests.rs"]
mod tests;
