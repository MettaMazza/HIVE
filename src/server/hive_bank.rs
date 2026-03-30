/// HIVE Bank — Mesh-native DeFi portal.
///
/// Wallet management, NFT trading cards, and transaction history.
/// Served on localhost:3037 (configurable via REMOVED_MESH_GOVERNED).
///
/// API:
///   GET  /api/wallet/balance    — current wallet balance
///   POST /api/wallet/send       — send HIVE to another user
///   GET  /api/wallet/history    — transaction history
///   GET  /api/gallery           — all NFT trading cards
///   POST /api/gallery/buy       — purchase a card
///   GET  /api/stats             — overall system stats

use axum::{
    routing::{get, post},
    Router, Json,
    response::Html,
};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

use crate::crypto::keystore::Keystore;
use crate::crypto::solana::{HiveSolanaClient, WalletMode};
use crate::crypto::nft::CardGallery;
use crate::crypto::keystore::WalletRole;
use crate::crypto::credits::CreditsEngine;

const GALLERY_PATH: &str = "data/wallets/gallery.json";
const ADMIN_WALLET: &str = "apis_system";

#[derive(Clone)]
struct BankState {
    keystore: Arc<Keystore>,
    solana: Arc<HiveSolanaClient>,
    credits: Arc<CreditsEngine>,
}

impl BankState {
    fn gallery_path() -> PathBuf {
        PathBuf::from(GALLERY_PATH)
    }

    fn ensure_system_wallet(&self) {
        if !self.keystore.wallet_exists(ADMIN_WALLET) {
            match self.keystore.create_wallet(ADMIN_WALLET, WalletRole::System) {
                Ok(pk) => {
                    // Mint initial supply to system wallet
                    let _ = self.solana.request_airdrop(&pk, 1.0);
                    tracing::info!("[BANK] 🏦 Created system wallet: {}", pk);
                }
                Err(e) => tracing::warn!("[BANK] ⚠️ Failed to create system wallet: {}", e),
            }
        }
    }
}

// ─── Request/Response types ──────────────────────────────────────

#[derive(Deserialize)]
struct SendRequest {
    to: String,
    amount: f64,
}

#[derive(Deserialize)]
struct BuyRequest {
    card_id: String,
}

#[derive(Deserialize)]
struct NftListRequest {
    card_id: String,
    price: f64,
}

#[derive(Deserialize)]
struct NftDelistRequest {
    card_id: String,
}

pub async fn spawn_hive_bank_server() {
    let port: u16 = std::env::var("REMOVED_MESH_GOVERNED")
        .ok().and_then(|v| v.parse().ok())
        .unwrap_or(3037);

    let wallet_secret = std::env::var("HIVE_WALLET_SECRET").unwrap_or_else(|_| {
        let fallback = std::env::var("DISCORD_TOKEN")
            .unwrap_or_else(|_| "hive_default_secret_change_me".into());
        use sha2::{Sha256, Digest};
        format!("{:x}", Sha256::digest(fallback.as_bytes()))
    });

    let keystore = Arc::new(Keystore::new_with_secret("data/wallets", wallet_secret));
    let solana = Arc::new(HiveSolanaClient::new());
    let credits = Arc::new(CreditsEngine::new());

    let state = BankState { keystore, solana, credits };
    state.ensure_system_wallet();

    tokio::spawn(async move {
        tracing::info!("[BANK] 🏦 HIVE Bank starting on http://0.0.0.0:{}", port);

        let app = Router::new()
            .route("/api/wallet/balance", get(api_balance))
            .route("/api/wallet/send", post(api_send))
            .route("/api/wallet/history", get(api_history))
            .route("/api/gallery", get(api_gallery))
            .route("/api/gallery/buy", post(api_buy))
            .route("/api/stats", get(api_stats))
            .route("/api/nft/marketplace", get(api_nft_marketplace))
            .route("/api/nft/auctions", get(api_nft_auctions))
            .route("/api/nft/list", post(api_nft_list))
            .route("/api/nft/delist", post(api_nft_delist))
            .route("/api/credits/balance", get(api_credits_balance))
            .route("/api/credits/history", get(api_credits_history))
            .route("/api/credits/leaderboard", get(api_credits_leaderboard))
            .route("/api/credits/stats", get(api_credits_stats))
            .fallback(get(serve_bank_page))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let addr = format!("0.0.0.0:{}", port);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                tracing::info!("[BANK] 🏦 Bound on {}", addr);
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("[BANK] ❌ Server error: {}", e);
                }
            }
            Err(e) => tracing::error!("[BANK] ❌ Failed to bind {}: {}", addr, e),
        }
    });
}

// ─── Wallet Balance ──────────────────────────────────────────────

async fn api_balance(
    axum::extract::State(state): axum::extract::State<BankState>,
) -> Json<Value> {
    let pubkey = state.keystore.get_public_key(ADMIN_WALLET)
        .unwrap_or_else(|| "no_wallet".into());

    let (hive, sol) = match state.solana.get_balance(&pubkey) {
        Ok(b) => (b.hive, b.sol),
        Err(_) => (0.0, 0.0),
    };

    let mode = match state.solana.mode() {
        WalletMode::Simulation => "simulation",
        WalletMode::Live => "live",
    };

    Json(json!({
        "address": pubkey,
        "hive": hive,
        "sol": sol,
        "mode": mode,
        "total_supply": state.solana.total_supply(),
    }))
}

// ─── Send HIVE ───────────────────────────────────────────────────

async fn api_send(
    axum::extract::State(state): axum::extract::State<BankState>,
    Json(req): Json<SendRequest>,
) -> Json<Value> {
    if req.amount <= 0.0 {
        return Json(json!({"success": false, "error": "Amount must be positive"}));
    }

    // Resolve recipient
    let to_pubkey = state.keystore.get_public_key(&req.to)
        .unwrap_or(req.to.clone());

    match state.solana.transfer_hive(&state.keystore, ADMIN_WALLET, &to_pubkey, req.amount) {
        Ok(sig) => Json(json!({
            "success": true,
            "signature": sig,
            "amount": req.amount,
            "to": to_pubkey,
        })),
        Err(e) => Json(json!({
            "success": false,
            "error": e,
        })),
    }
}

// ─── Transaction History ─────────────────────────────────────────

async fn api_history(
    axum::extract::State(state): axum::extract::State<BankState>,
) -> Json<Value> {
    let pubkey = state.keystore.get_public_key(ADMIN_WALLET)
        .unwrap_or_default();

    match state.solana.get_transaction_history(&pubkey, 50) {
        Ok(records) => {
            let txs: Vec<Value> = records.iter().map(|tx| json!({
                "id": tx.id,
                "tx_type": tx.tx_type,
                "amount": tx.amount,
                "from": tx.from,
                "to": tx.to,
                "status": tx.status,
                "timestamp": tx.timestamp,
            })).collect();

            Json(json!({ "transactions": txs }))
        }
        Err(e) => Json(json!({ "transactions": [], "error": e })),
    }
}

// ─── Gallery ─────────────────────────────────────────────────────

async fn api_gallery(
    axum::extract::State(state): axum::extract::State<BankState>,
) -> Json<Value> {
    let gallery = CardGallery::load(&BankState::gallery_path());
    let pubkey = state.keystore.get_public_key(ADMIN_WALLET).unwrap_or_default();

    let cards: Vec<Value> = gallery.cards.iter().map(|c| json!({
        "id": c.id,
        "name": c.name,
        "rarity": c.rarity,
        "price": c.price,
        "for_sale": c.for_sale,
        "owner": c.owner,
        "prompt": c.prompt,
        "created_at": c.created_at,
    })).collect();

    let owned_count = gallery.cards_owned_by(&pubkey).len();

    Json(json!({
        "cards": cards,
        "total": gallery.total_minted,
        "owned_count": owned_count,
    }))
}

// ─── Buy Card ────────────────────────────────────────────────────

async fn api_buy(
    axum::extract::State(state): axum::extract::State<BankState>,
    Json(req): Json<BuyRequest>,
) -> Json<Value> {
    let buyer_pubkey = match state.keystore.get_public_key(ADMIN_WALLET) {
        Some(pk) => pk,
        None => return Json(json!({"success": false, "error": "No wallet"})),
    };

    let mut gallery = CardGallery::load(&BankState::gallery_path());

    // Find card by prefix match
    let full_id = match gallery.cards.iter().find(|c| c.id.starts_with(&req.card_id)) {
        Some(c) => c.id.clone(),
        None => return Json(json!({"success": false, "error": "Card not found"})),
    };

    match gallery.purchase_card(&full_id, &buyer_pubkey) {
        Ok((price, seller_pubkey)) => {
            // Pay seller
            let _ = state.solana.transfer_hive(&state.keystore, ADMIN_WALLET, &seller_pubkey, price);
            let _ = gallery.save(&BankState::gallery_path());

            Json(json!({
                "success": true,
                "card_id": full_id,
                "price": price,
            }))
        }
        Err(e) => Json(json!({"success": false, "error": e})),
    }
}

// ─── System Stats ────────────────────────────────────────────────

async fn api_stats(
    axum::extract::State(state): axum::extract::State<BankState>,
) -> Json<Value> {
    let gallery = CardGallery::load(&BankState::gallery_path());
    let stats = gallery.stats();
    let wallets = state.keystore.list_wallets();

    Json(json!({
        "wallet_count": wallets.len(),
        "total_supply": state.solana.total_supply(),
        "nft": stats,
        "mode": match state.solana.mode() {
            WalletMode::Simulation => "simulation",
            WalletMode::Live => "live",
        },
    }))
}

// ─── Serve HTML ──────────────────────────────────────────────────

async fn serve_bank_page() -> Html<&'static str> {
    Html(super::hive_bank_html::hive_bank_html())
}

// ─── NFT Marketplace ─────────────────────────────────────────

async fn api_nft_marketplace(
    axum::extract::State(_state): axum::extract::State<BankState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let page = params.get("page").and_then(|p| p.parse::<u32>().ok()).unwrap_or(1);
    let limit = params.get("limit").and_then(|l| l.parse::<usize>().ok()).unwrap_or(20);

    let gallery = CardGallery::load(&BankState::gallery_path());
    let for_sale: Vec<Value> = gallery.cards.iter()
        .filter(|c| c.for_sale)
        .skip(((page - 1) as usize) * limit)
        .take(limit)
        .map(|c| json!({
            "id": c.id,
            "name": c.name,
            "rarity": c.rarity,
            "price": c.price,
            "owner": c.owner,
            "created_at": c.created_at,
        }))
        .collect();

    Json(json!({
        "page": page,
        "limit": limit,
        "total": gallery.cards.iter().filter(|c| c.for_sale).count(),
        "listings": for_sale,
    }))
}

async fn api_nft_auctions(
    _state: axum::extract::State<BankState>,
) -> Json<Value> {
    Json(json!({
        "status": "coming_soon",
        "message": "NFT auctions feature will be available in a future release",
        "auctions": [],
    }))
}

async fn api_nft_list(
    axum::extract::State(state): axum::extract::State<BankState>,
    Json(req): Json<NftListRequest>,
) -> Json<Value> {
    let mut gallery = CardGallery::load(&BankState::gallery_path());
    let owner_pubkey = state.keystore.get_public_key(ADMIN_WALLET).unwrap_or_default();

    let result = match gallery.cards.iter_mut().find(|c| c.id == req.card_id) {
        Some(card) => {
            if card.owner != owner_pubkey {
                return Json(json!({"success": false, "error": "You do not own this card"}));
            }
            card.for_sale = true;
            card.price = req.price;
            let card_id = card.id.clone();
            let card_price = card.price;
            Some((card_id, card_price))
        }
        None => None,
    };

    match result {
        Some((card_id, card_price)) => {
            let _ = gallery.save(&BankState::gallery_path());
            Json(json!({
                "success": true,
                "card_id": card_id,
                "price": card_price,
                "message": format!("Card listed for sale at {:.2} HIVE", card_price),
            }))
        }
        None => Json(json!({"success": false, "error": "Card not found"})),
    }
}

async fn api_nft_delist(
    axum::extract::State(state): axum::extract::State<BankState>,
    Json(req): Json<NftDelistRequest>,
) -> Json<Value> {
    let mut gallery = CardGallery::load(&BankState::gallery_path());
    let owner_pubkey = state.keystore.get_public_key(ADMIN_WALLET).unwrap_or_default();

    let result = match gallery.cards.iter_mut().find(|c| c.id == req.card_id) {
        Some(card) => {
            if card.owner != owner_pubkey {
                return Json(json!({"success": false, "error": "You do not own this card"}));
            }
            card.for_sale = false;
            Some(card.id.clone())
        }
        None => None,
    };

    match result {
        Some(card_id) => {
            let _ = gallery.save(&BankState::gallery_path());
            Json(json!({
                "success": true,
                "card_id": card_id,
                "message": "Card delisted from marketplace",
            }))
        }
        None => Json(json!({"success": false, "error": "Card not found"})),
    }
}

// ─── Credits System ──────────────────────────────────────────

async fn api_credits_balance(
    axum::extract::State(state): axum::extract::State<BankState>,
) -> Json<Value> {
    let balance = state.credits.balance("system");

    Json(json!({
        "user": "system",
        "credits": balance,
        "status": "ok",
    }))
}

async fn api_credits_history(
    axum::extract::State(state): axum::extract::State<BankState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let limit = params.get("limit").and_then(|l| l.parse::<usize>().ok()).unwrap_or(50);
    let history = state.credits.history("system", limit);

    Json(json!({
        "user": "system",
        "transactions": history,
        "count": history.len(),
    }))
}

async fn api_credits_leaderboard(
    axum::extract::State(state): axum::extract::State<BankState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let limit = params.get("limit").and_then(|l| l.parse::<usize>().ok()).unwrap_or(10);
    let leaders = state.credits.leaderboard(limit);

    Json(json!({
        "leaderboard": leaders,
        "count": leaders.len(),
    }))
}

async fn api_credits_stats(
    axum::extract::State(state): axum::extract::State<BankState>,
) -> Json<Value> {
    Json(state.credits.stats())
}
