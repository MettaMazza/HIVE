//! HIVE Coin Token Configuration.
//!
//! Defines the SPL token parameters, pricing, and role-based access rules.
//! The creator key is sovereign — no admin or system wallet can override it.

use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// HIVE Coin has 6 decimal places (like USDC).
/// 1 HIVE = 1_000_000 base units.
pub const HIVE_DECIMALS: u8 = 6;
pub const HIVE_BASE_UNIT: u64 = 1_000_000;

/// Convert a human-readable HIVE amount to base units.
pub fn to_base_units(amount: f64) -> u64 {
    (amount * HIVE_BASE_UNIT as f64) as u64
}

/// Convert base units to human-readable HIVE amount.
pub fn from_base_units(base: u64) -> f64 {
    base as f64 / HIVE_BASE_UNIT as f64
}

/// NFT rarity tiers based on observer confidence score.
#[derive(Debug, Clone, PartialEq)]
pub enum Rarity {
    Common,     // 0.00 – 0.69
    Uncommon,   // 0.70 – 0.84
    Rare,       // 0.85 – 0.94
    Legendary,  // 0.95 – 1.00
}

impl Rarity {
    /// Determine rarity from observer confidence score.
    pub fn from_confidence(score: f64) -> Self {
        match score {
            s if s >= 0.95 => Rarity::Legendary,
            s if s >= 0.85 => Rarity::Rare,
            s if s >= 0.70 => Rarity::Uncommon,
            _ => Rarity::Common,
        }
    }

    /// Price in HIVE Coin (human-readable). Mesh-governed — not per-node configurable.
    pub fn price(&self) -> f64 {
        match self {
            Rarity::Common => 10.0,
            Rarity::Uncommon => 25.0,
            Rarity::Rare => 50.0,
            Rarity::Legendary => 100.0,
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Rarity::Common => "⚪",
            Rarity::Uncommon => "🔵",
            Rarity::Rare => "💎",
            Rarity::Legendary => "⭐",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Rarity::Common => "Common",
            Rarity::Uncommon => "Uncommon",
            Rarity::Rare => "Rare",
            Rarity::Legendary => "Legendary",
        }
    }
}

/// Reward amounts (in HIVE, human-readable).
pub struct Rewards;

impl Rewards {
    /// First message of the day.
    pub fn daily_engagement() -> f64 { 5.0 }
    /// Each unique tool used per session.
    pub fn tool_usage() -> f64 { 1.0 }
    /// Autonomy cycle completion (credited to Apis).
    pub fn autonomy_contribution() -> f64 { 2.0 }
    /// Voting on a governance proposal.
    pub fn governance_vote() -> f64 { 3.0 }
    /// Storing a lesson or creating a routine.
    pub fn content_contribution() -> f64 { 2.0 }
}

/// Runtime token configuration loaded from environment.
pub struct TokenConfig {
    /// The SPL token mint address for HIVE Coin.
    pub mint_address: Option<Pubkey>,
    /// Solana RPC endpoint.
    pub rpc_url: String,
    /// Whether we're on devnet (free tokens) or mainnet (real).
    pub is_devnet: bool,
    /// The system wallet ID for Apis.
    pub apis_wallet_id: String,
}

impl TokenConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        let rpc_url = std::env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://api.devnet.solana.com".into());

        let is_devnet = rpc_url.contains("devnet");

        let mint_address = std::env::var("REMOVED_MESH_GOVERNED")
            .ok()
            .and_then(|s| Pubkey::from_str(&s).ok());

        Self {
            mint_address,
            rpc_url,
            is_devnet,
            apis_wallet_id: "apis_system".into(),
        }
    }
}

impl std::fmt::Display for Rarity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.emoji(), self.label())
    }
}
