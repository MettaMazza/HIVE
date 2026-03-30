//! NFT Tool — Browse, buy, gift, and manage HIVE Trading Cards.
//!
//! Admin-only. Platform-agnostic.
//!
//! Actions:
//!   action:[gallery] — List all cards for sale
//!   action:[collection] user_id:[id] — Show a user's collection
//!   action:[buy] card_id:[id] — Purchase a card with HIVE Coin
//!   action:[gift] card_id:[id] to:[recipient] — Gift a card
//!   action:[sell] card_id:[id] price:[amount] — List a card for sale
//!   action:[stats] — Gallery statistics

use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::mpsc;
use crate::models::tool::{ToolResult, ToolStatus};
use crate::models::scope::Scope;
use crate::crypto::keystore::Keystore;
use crate::crypto::solana::HiveSolanaClient;
use crate::crypto::nft::CardGallery;

const GALLERY_PATH: &str = "data/wallets/gallery.json";

/// Execute an NFT tool operation.
pub async fn execute_nft(
    task_id: String,
    desc: String,
    scope: &Scope,
    keystore: Arc<Keystore>,
    solana: Arc<HiveSolanaClient>,
    capabilities: Option<Arc<crate::models::capabilities::AgentCapabilities>>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    // Admin gate
    let invoker_id = match scope {
        Scope::Public { user_id, .. } => user_id.clone(),
        Scope::Private { user_id } => user_id.clone(),
    };

    let is_admin = capabilities.as_ref().map_or(false, |c| c.admin_users.contains(&invoker_id));
    let is_system = invoker_id == "apis_autonomy" || invoker_id == "apis_system";

    if !is_admin && !is_system {
        return ToolResult {
            task_id,
            output: "⛔ NFT operations are restricted to administrators only.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Not admin".into()),
        };
    }

    let action = extract_tag(&desc, "action:").unwrap_or_else(|| "gallery".into());
    let gallery_path = PathBuf::from(GALLERY_PATH);

    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("🎴 NFT: `{}` | user: {}\n", action, invoker_id)).await;
    }

    match action.as_str() {
        "gallery" => execute_gallery(task_id, &gallery_path).await,
        "collection" => execute_collection(task_id, &desc, &invoker_id, &keystore, &gallery_path).await,
        "buy" => execute_buy(task_id, &desc, &invoker_id, &keystore, &solana, &gallery_path).await,
        "gift" => execute_gift(task_id, &desc, &invoker_id, &keystore, &gallery_path).await,
        "sell" => execute_sell(task_id, &desc, &invoker_id, &keystore, &gallery_path).await,
        "stats" => execute_stats(task_id, &gallery_path).await,
        _ => ToolResult {
            task_id,
            output: format!("Unknown NFT action: '{}'. Use gallery, collection, buy, gift, sell, or stats.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        },
    }
}

// ─── GALLERY (browse all for-sale cards) ─────────────────────────

async fn execute_gallery(task_id: String, gallery_path: &PathBuf) -> ToolResult {
    let gallery = CardGallery::load(gallery_path);
    let for_sale = gallery.cards_for_sale();

    if for_sale.is_empty() {
        return ToolResult {
            task_id,
            output: "🎴 **HIVE Gallery**\n\nNo cards available for sale yet. Cards are auto-minted when Apis generates images during autonomy.".into(),
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    let mut output = format!("🎴 **HIVE Gallery** — {} cards for sale\n\n", for_sale.len());
    for card in for_sale.iter().take(20) {
        let rarity_emoji = match card.rarity.as_str() {
            "Common" => "⚪",
            "Uncommon" => "🔵",
            "Rare" => "💎",
            "Legendary" => "⭐",
            _ => "❓",
        };
        output.push_str(&format!(
            "{} **{}** | {:.2} HIVE | `{}`\n",
            rarity_emoji, card.name, card.price, &card.id[..8]
        ));
    }
    if for_sale.len() > 20 {
        output.push_str(&format!("\n_...and {} more_\n", for_sale.len() - 20));
    }

    ToolResult { task_id, output, tokens_used: 0, status: ToolStatus::Success }
}

// ─── COLLECTION (user's owned cards) ─────────────────────────────

async fn execute_collection(
    task_id: String,
    desc: &str,
    invoker_id: &str,
    keystore: &Keystore,
    gallery_path: &PathBuf,
) -> ToolResult {
    let wallet_id = extract_tag(desc, "user_id:").unwrap_or_else(|| invoker_id.to_string());
    let pubkey = match keystore.get_public_key(&wallet_id) {
        Some(pk) => pk,
        None => return ToolResult {
            task_id,
            output: format!("No wallet found for '{}'. Create one first with the wallet tool.", wallet_id),
            tokens_used: 0,
            status: ToolStatus::Failed("No wallet".into()),
        },
    };

    let gallery = CardGallery::load(gallery_path);
    let owned = gallery.cards_owned_by(&pubkey);

    if owned.is_empty() {
        return ToolResult {
            task_id,
            output: format!("📂 **{}'s Collection**\n\nNo cards yet. Browse the gallery and buy some!", wallet_id),
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    let mut output = format!("📂 **{}'s Collection** — {} cards\n\n", wallet_id, owned.len());
    for card in &owned {
        let sale_tag = if card.for_sale { " [FOR SALE]" } else { "" };
        output.push_str(&format!(
            "• **{}** | {} | {:.2} HIVE{} | `{}`\n",
            card.name, card.rarity, card.price, sale_tag, &card.id[..8]
        ));
    }

    ToolResult { task_id, output, tokens_used: 0, status: ToolStatus::Success }
}

// ─── BUY ─────────────────────────────────────────────────────────

async fn execute_buy(
    task_id: String,
    desc: &str,
    invoker_id: &str,
    keystore: &Keystore,
    solana: &HiveSolanaClient,
    gallery_path: &PathBuf,
) -> ToolResult {
    let card_id = match extract_tag(desc, "card_id:") {
        Some(id) => id,
        None => return ToolResult {
            task_id,
            output: "Error: 'card_id:' required. Use 'card_id:[first 8 chars of card ID]'".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing card_id".into()),
        },
    };

    let buyer_pubkey = match keystore.get_public_key(invoker_id) {
        Some(pk) => pk,
        None => return ToolResult {
            task_id,
            output: "You need a wallet first. Use the wallet tool with action:[create].".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("No wallet".into()),
        },
    };

    let mut gallery = CardGallery::load(gallery_path);

    // Find card by full ID or prefix
    let full_id = match gallery.cards.iter().find(|c| c.id.starts_with(&card_id)) {
        Some(c) => c.id.clone(),
        None => return ToolResult {
            task_id,
            output: format!("Card '{}' not found.", card_id),
            tokens_used: 0,
            status: ToolStatus::Failed("Not found".into()),
        },
    };

    // Check buyer balance first
    let card_price = gallery.get_card(&full_id).unwrap().price;
    let balance = solana.get_balance(&buyer_pubkey).map_err(|e| e.to_string());
    if let Ok(bal) = &balance {
        if bal.hive < card_price {
            return ToolResult {
                task_id,
                output: format!(
                    "Insufficient HIVE balance. Card costs {:.2} HIVE but you have {:.2} HIVE.",
                    card_price, bal.hive
                ),
                tokens_used: 0,
                status: ToolStatus::Failed("Insufficient balance".into()),
            };
        }
    }

    // Execute purchase
    match gallery.purchase_card(&full_id, &buyer_pubkey) {
        Ok((price, seller_pubkey)) => {
            // Transfer HIVE from buyer to seller
            let tx_result = solana.transfer_hive(keystore, invoker_id, &seller_pubkey, price);

            gallery.save(gallery_path).unwrap_or_else(|e| {
                tracing::error!("[NFT] Failed to save gallery: {}", e);
            });

            let card = gallery.get_card(&full_id).unwrap();
            ToolResult {
                task_id,
                output: format!(
                    "🎴 **Card purchased!**\n\n\
                    **{}**\n\
                    💰 Price: {:.2} HIVE\n\
                    📝 Tx: `{}`\n\n\
                    The card is now in your collection.",
                    card.name, price,
                    tx_result.unwrap_or_else(|e| format!("(payment pending: {})", e))
                ),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => ToolResult {
            task_id,
            output: format!("❌ Purchase failed: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e),
        },
    }
}

// ─── GIFT ────────────────────────────────────────────────────────

async fn execute_gift(
    task_id: String,
    desc: &str,
    invoker_id: &str,
    keystore: &Keystore,
    gallery_path: &PathBuf,
) -> ToolResult {
    let card_id = match extract_tag(desc, "card_id:") {
        Some(id) => id,
        None => return ToolResult {
            task_id,
            output: "Error: 'card_id:' required.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing card_id".into()),
        },
    };
    let to_raw = match extract_tag(desc, "to:") {
        Some(t) => t,
        None => return ToolResult {
            task_id,
            output: "Error: 'to:' required.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing recipient".into()),
        },
    };

    let from_pubkey = match keystore.get_public_key(invoker_id) {
        Some(pk) => pk,
        None => return ToolResult {
            task_id,
            output: "You need a wallet first.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("No wallet".into()),
        },
    };

    let to_pubkey = keystore.get_public_key(&to_raw).unwrap_or(to_raw);

    let mut gallery = CardGallery::load(gallery_path);
    let full_id = match gallery.cards.iter().find(|c| c.id.starts_with(&card_id)) {
        Some(c) => c.id.clone(),
        None => return ToolResult {
            task_id,
            output: format!("Card '{}' not found.", card_id),
            tokens_used: 0,
            status: ToolStatus::Failed("Not found".into()),
        },
    };

    match gallery.gift_card(&full_id, &from_pubkey, &to_pubkey) {
        Ok(()) => {
            gallery.save(gallery_path).unwrap_or_else(|e| {
                tracing::error!("[NFT] Failed to save gallery: {}", e);
            });
            let card = gallery.get_card(&full_id).unwrap();
            ToolResult {
                task_id,
                output: format!("🎁 **Card gifted!**\n\n**{}** → `{}`", card.name, to_pubkey),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => ToolResult {
            task_id,
            output: format!("❌ Gift failed: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e),
        },
    }
}

// ─── SELL (list for sale) ────────────────────────────────────────

async fn execute_sell(
    task_id: String,
    desc: &str,
    invoker_id: &str,
    keystore: &Keystore,
    gallery_path: &PathBuf,
) -> ToolResult {
    let card_id = match extract_tag(desc, "card_id:") {
        Some(id) => id,
        None => return ToolResult {
            task_id,
            output: "Error: 'card_id:' required.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing card_id".into()),
        },
    };

    let owner_pubkey = match keystore.get_public_key(invoker_id) {
        Some(pk) => pk,
        None => return ToolResult {
            task_id,
            output: "You need a wallet first.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("No wallet".into()),
        },
    };

    let price: Option<f64> = extract_tag(desc, "price:").and_then(|v| v.parse().ok());

    let mut gallery = CardGallery::load(gallery_path);
    let full_id = match gallery.cards.iter().find(|c| c.id.starts_with(&card_id)) {
        Some(c) => c.id.clone(),
        None => return ToolResult {
            task_id,
            output: format!("Card '{}' not found.", card_id),
            tokens_used: 0,
            status: ToolStatus::Failed("Not found".into()),
        },
    };

    match gallery.list_for_sale(&full_id, &owner_pubkey, price) {
        Ok(()) => {
            gallery.save(gallery_path).unwrap_or_else(|e| {
                tracing::error!("[NFT] Failed to save gallery: {}", e);
            });
            let card = gallery.get_card(&full_id).unwrap();
            ToolResult {
                task_id,
                output: format!("✅ **Card listed!**\n\n**{}** — {:.2} HIVE", card.name, card.price),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => ToolResult {
            task_id,
            output: format!("❌ Failed to list: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e),
        },
    }
}

// ─── STATS ───────────────────────────────────────────────────────

async fn execute_stats(task_id: String, gallery_path: &PathBuf) -> ToolResult {
    let gallery = CardGallery::load(gallery_path);
    let stats = gallery.stats();

    let output = format!(
        "📊 **HIVE Gallery Stats**\n\n\
        Total minted: {}\n\
        For sale: {}\n\n\
        **Rarity breakdown:**\n\
        ⚪ Common: {}\n\
        🔵 Uncommon: {}\n\
        💎 Rare: {}\n\
        ⭐ Legendary: {}",
        stats["total_minted"], stats["for_sale"],
        stats["rarity_breakdown"]["common"],
        stats["rarity_breakdown"]["uncommon"],
        stats["rarity_breakdown"]["rare"],
        stats["rarity_breakdown"]["legendary"],
    );

    ToolResult { task_id, output, tokens_used: 0, status: ToolStatus::Success }
}

fn extract_tag(text: &str, tag: &str) -> Option<String> {
    crate::agent::preferences::extract_tag(text, tag)
}
