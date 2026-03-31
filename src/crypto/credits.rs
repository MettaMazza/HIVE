/// HIVE Credits Engine — Non-crypto internal points system.
///
/// Completely separate from HIVE Coin. No blockchain, no regulation concerns.
/// Local JSON-backed ledger of earned/spent points. Peer-scoped, never
/// transmitted off-device. Credits buy priority, not access — everyone
/// can use the mesh even with zero credits.
///
/// EARNING:
///   - Providing compute to the mesh (per 1K tokens served)
///   - Providing network relay (per 100 requests relayed)
///   - Staying connected and idle (per hour)
///   - Contributing code (merged PRs)
///   - Sharing on social media with reference links
///   - Positive community behaviour votes
///   - Governance participation
///   - Content contributions (lessons, routines)
///
/// SPENDING:
///   - Remote compute (per 1K tokens consumed)
///   - Network relay usage (per 100 requests)
///   - Marketplace purchases
///   - Priority queue boost
///
/// DYNAMIC PRICING:
///   All earn/spend rates are multiplied by demand. High demand = providers
///   earn more AND consumers pay more. Low demand = base rates.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ─── Credit Source ──────────────────────────────────────────────────

/// How credits were earned or spent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CreditSource {
    /// Provided compute to a mesh peer.
    ComputeProvided {
        tokens_served: u64,
        demand_multiplier: f64,
    },
    /// Relayed network requests for a mesh peer.
    NetworkProvided {
        requests_relayed: u64,
        demand_multiplier: f64,
    },
    /// Stayed connected to the mesh, contributing capacity.
    IdleContribution {
        hours_connected: f64,
    },
    /// Contributed code to the HIVE project.
    CodeContribution {
        pr_id: String,
        lines_changed: u32,
    },
    /// Shared HIVE on social media with a reference link.
    SocialShare {
        platform: String,
        reference_url: String,
    },
    /// Received a positive community behaviour vote.
    CommunityVote {
        voter_id: String,
        positive: bool,
    },
    /// Participated in governance (voted on a proposal).
    GovernanceParticipation {
        proposal_id: String,
    },
    /// Contributed content (lesson, routine, mesh site).
    ContentContribution {
        content_type: String,
    },
    /// Spent credits on a service.
    Spent {
        service: String,
    },
    /// Welcome bonus for new peers.
    WelcomeBonus,
    /// Manual adjustment by system (admin only).
    SystemAdjustment {
        reason: String,
    },
}

impl CreditSource {
    pub fn label(&self) -> &str {
        match self {
            CreditSource::ComputeProvided { .. } => "compute_provided",
            CreditSource::NetworkProvided { .. } => "network_provided",
            CreditSource::IdleContribution { .. } => "idle_contribution",
            CreditSource::CodeContribution { .. } => "code_contribution",
            CreditSource::SocialShare { .. } => "social_share",
            CreditSource::CommunityVote { .. } => "community_vote",
            CreditSource::GovernanceParticipation { .. } => "governance",
            CreditSource::ContentContribution { .. } => "content",
            CreditSource::Spent { .. } => "spent",
            CreditSource::WelcomeBonus => "welcome_bonus",
            CreditSource::SystemAdjustment { .. } => "system_adjustment",
        }
    }
}

// ─── Credit Account ─────────────────────────────────────────────────

/// A single peer's credit account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditAccount {
    pub peer_id: String,
    pub balance: f64,
    pub lifetime_earned: f64,
    pub lifetime_spent: f64,
    /// Consecutive days this peer has contributed to the mesh.
    pub contribution_streak: u32,
    /// ISO 8601 timestamp of last contribution.
    pub last_contribution: String,
    /// Community reputation score (0.0 – 1.0). Derived from votes.
    pub reputation_score: f64,
    /// Breakdown of earnings by source label.
    pub earning_sources: HashMap<String, f64>,
    /// Social shares today (for daily cap enforcement).
    pub social_shares_today: u32,
    /// Date of last social share count reset (YYYY-MM-DD).
    pub social_share_date: String,
    /// Created at timestamp.
    pub created_at: String,
}

impl CreditAccount {
    pub fn new(peer_id: &str) -> Self {
        let now = chrono::Utc::now();
        Self {
            peer_id: peer_id.to_string(),
            balance: 0.0,
            lifetime_earned: 0.0,
            lifetime_spent: 0.0,
            contribution_streak: 0,
            last_contribution: now.to_rfc3339(),
            reputation_score: 0.5, // Start neutral
            earning_sources: HashMap::new(),
            social_shares_today: 0,
            social_share_date: now.format("%Y-%m-%d").to_string(),
            created_at: now.to_rfc3339(),
        }
    }
}

// ─── Credit Transaction ─────────────────────────────────────────────

/// A credit transaction record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditTransaction {
    pub id: String,
    pub peer_id: String,
    /// Positive = earned, negative = spent.
    pub amount: f64,
    pub source: CreditSource,
    pub timestamp: String,
    pub description: String,
    /// Running balance after this transaction.
    pub balance_after: f64,
}

// ─── Credit Configuration ───────────────────────────────────────────

/// Configurable credit rates loaded from environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditConfig {
    /// Welcome bonus for new peers.
    pub welcome_bonus: f64,
    /// Credits per 1K tokens of compute provided.
    pub compute_earn_per_1k: f64,
    /// Credits per 100 network requests relayed.
    pub network_earn_per_100: f64,
    /// Credits per hour of idle connection.
    pub idle_earn_per_hour: f64,
    /// Credits per merged code contribution.
    pub code_contribution_base: f64,
    /// Credits per social media share.
    pub social_share_earn: f64,
    /// Max social shares per day that earn credits.
    pub social_share_max_per_day: u32,
    /// Credits per positive community vote.
    pub community_vote_earn: f64,
    /// Credits per governance vote cast.
    pub governance_vote_earn: f64,
    /// Credits per content contribution (lesson/routine).
    pub content_contribution_earn: f64,
    /// Cost per 1K tokens of remote compute consumed.
    pub compute_cost_per_1k: f64,
    /// Cost per 100 network relay requests consumed.
    pub network_cost_per_100: f64,
    /// Cost for priority queue boost.
    pub priority_boost_cost: f64,
    /// Multiplier applied during high demand (>80% capacity).
    pub high_demand_multiplier: f64,
    /// Multiplier applied during moderate demand (>50% capacity).
    pub moderate_demand_multiplier: f64,
}

impl CreditConfig {
    /// Mesh-governed credit rates. These are CONSTANTS — not configurable
    /// per-node. Allowing individual nodes to set their own earn/spend rates
    /// would break mesh fairness (equivalent to printing your own money).
    /// Changes to these values must go through mesh governance, not .env.
    pub fn mesh_defaults() -> Self {
        Self {
            welcome_bonus: 100.0,
            compute_earn_per_1k: 2.0,
            network_earn_per_100: 1.0,
            idle_earn_per_hour: 0.5,
            code_contribution_base: 10.0,
            social_share_earn: 3.0,
            social_share_max_per_day: 5,
            community_vote_earn: 1.0,
            governance_vote_earn: 2.0,
            content_contribution_earn: 2.0,
            compute_cost_per_1k: 1.0,
            network_cost_per_100: 0.5,
            priority_boost_cost: 5.0,
            high_demand_multiplier: 1.5,
            moderate_demand_multiplier: 1.2,
        }
    }
}


// ─── Credit Ledger ──────────────────────────────────────────────────

/// The credit ledger — stores all accounts and transactions.
/// Persisted to disk as JSON. Completely local, never shared.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreditLedger {
    pub accounts: HashMap<String, CreditAccount>,
    pub transactions: Vec<CreditTransaction>,
    pub tx_counter: u64,
    pub total_credits_issued: f64,
    pub total_credits_spent: f64,
}

impl CreditLedger {
    /// Load ledger from disk, or create empty if none exists.
    pub fn load(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    /// Persist ledger to disk.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create credits directory: {}", e))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize credits: {}", e))?;
        std::fs::write(path, json)
            .map_err(|e| format!("Failed to write credits: {}", e))
    }

    fn next_tx_id(&mut self) -> String {
        self.tx_counter += 1;
        format!("cred_{:08}_{}", self.tx_counter, &uuid::Uuid::new_v4().to_string()[..8])
    }
}

// ─── Credits Engine ─────────────────────────────────────────────────

/// The main credits engine. Handles earning, spending, and querying.
pub struct CreditsEngine {
    ledger_path: PathBuf,
    config: CreditConfig,
}

impl CreditsEngine {
    /// Create a new engine with default path.
    pub fn new() -> Self {
        Self {
            ledger_path: PathBuf::from("data/credits/ledger.json"),
            config: CreditConfig::mesh_defaults(),
        }
    }

    /// Create with explicit path (for testing).
    pub fn new_with_path(path: PathBuf) -> Self {
        Self {
            ledger_path: path,
            config: CreditConfig::mesh_defaults(),
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &CreditConfig { &self.config }

    // ── Account Management ──

    /// Get or create an account for a peer. New accounts receive welcome bonus.
    pub fn get_or_create_account(&self, peer_id: &str) -> CreditAccount {
        let mut ledger = CreditLedger::load(&self.ledger_path);

        if let Some(account) = ledger.accounts.get(peer_id) {
            return account.clone();
        }

        // Create new account with welcome bonus
        let mut account = CreditAccount::new(peer_id);
        account.balance = self.config.welcome_bonus;
        account.lifetime_earned = self.config.welcome_bonus;

        // Record welcome bonus transaction
        let tx_id = ledger.next_tx_id();
        ledger.transactions.push(CreditTransaction {
            id: tx_id,
            peer_id: peer_id.to_string(),
            amount: self.config.welcome_bonus,
            source: CreditSource::WelcomeBonus,
            timestamp: chrono::Utc::now().to_rfc3339(),
            description: format!("Welcome bonus: {} credits", self.config.welcome_bonus),
            balance_after: account.balance,
        });

        ledger.total_credits_issued += self.config.welcome_bonus;
        ledger.accounts.insert(peer_id.to_string(), account.clone());
        let _ = ledger.save(&self.ledger_path);

        tracing::info!("[CREDITS] 🎁 New peer {} — welcome bonus: {} credits",
            &peer_id[..peer_id.len().min(12)], self.config.welcome_bonus);

        account
    }

    /// Get balance for a peer. Returns 0 if no account exists.
    pub fn balance(&self, peer_id: &str) -> f64 {
        let ledger = CreditLedger::load(&self.ledger_path);
        ledger.accounts.get(peer_id).map(|a| a.balance).unwrap_or(0.0)
    }

    /// Get full account info for a peer.
    pub fn account(&self, peer_id: &str) -> Option<CreditAccount> {
        let ledger = CreditLedger::load(&self.ledger_path);
        ledger.accounts.get(peer_id).cloned()
    }

    // ── Earning ──

    /// Earn credits. The amount is the base amount before multiplier.
    /// The source determines any demand multiplier applied.
    pub fn earn(&self, peer_id: &str, source: CreditSource, base_amount: f64) -> Result<CreditTransaction, String> {
        if base_amount <= 0.0 {
            return Err("Earn amount must be positive".into());
        }

        let mut ledger = CreditLedger::load(&self.ledger_path);

        // Ensure account exists
        if !ledger.accounts.contains_key(peer_id) {
            let account = CreditAccount::new(peer_id);
            ledger.accounts.insert(peer_id.to_string(), account);
        }

        // Apply demand multiplier if applicable
        let multiplier = match &source {
            CreditSource::ComputeProvided { demand_multiplier, .. } => *demand_multiplier,
            CreditSource::NetworkProvided { demand_multiplier, .. } => *demand_multiplier,
            _ => 1.0,
        };

        // Check social share daily cap
        if let CreditSource::SocialShare { .. } = &source {
            let account = ledger.accounts.get_mut(peer_id).unwrap();
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            if account.social_share_date != today {
                account.social_shares_today = 0;
                account.social_share_date = today;
            }
            if account.social_shares_today >= self.config.social_share_max_per_day {
                return Err(format!(
                    "Daily social share limit reached ({}/{})",
                    account.social_shares_today, self.config.social_share_max_per_day
                ));
            }
            account.social_shares_today += 1;
        }

        let final_amount = base_amount * multiplier;
        let account = ledger.accounts.get_mut(peer_id).unwrap();
        account.balance += final_amount;
        account.lifetime_earned += final_amount;
        account.last_contribution = chrono::Utc::now().to_rfc3339();

        // Track earning source breakdown
        *account.earning_sources.entry(source.label().to_string()).or_insert(0.0) += final_amount;

        // Update contribution streak
        let now = chrono::Utc::now().date_naive();
        if let Ok(last) = chrono::NaiveDate::parse_from_str(
            &account.last_contribution[..10], "%Y-%m-%d"
        ) {
            let days_since = (now - last).num_days();
            if days_since <= 1 {
                // Same day or next day — streak continues
                if days_since == 1 {
                    account.contribution_streak += 1;
                }
            } else {
                account.contribution_streak = 1; // Reset
            }
        } else {
            account.contribution_streak = 1;
        }

        let balance_after = account.balance;

        // Record transaction
        let tx_id = ledger.next_tx_id();
        let description = match &source {
            CreditSource::ComputeProvided { tokens_served, .. } =>
                format!("Compute: {} tokens served (×{:.1})", tokens_served, multiplier),
            CreditSource::NetworkProvided { requests_relayed, .. } =>
                format!("Network: {} requests relayed (×{:.1})", requests_relayed, multiplier),
            CreditSource::IdleContribution { hours_connected } =>
                format!("Idle: {:.1} hours connected", hours_connected),
            CreditSource::CodeContribution { pr_id, lines_changed } =>
                format!("Code: PR {} ({} lines)", pr_id, lines_changed),
            CreditSource::SocialShare { platform, .. } =>
                format!("Social share on {}", platform),
            CreditSource::CommunityVote { voter_id, positive } =>
                format!("{} vote from {}", if *positive { "Positive" } else { "Negative" }, &voter_id[..voter_id.len().min(12)]),
            CreditSource::GovernanceParticipation { proposal_id } =>
                format!("Governance vote on {}", &proposal_id[..proposal_id.len().min(12)]),
            CreditSource::ContentContribution { content_type } =>
                format!("Content: {}", content_type),
            _ => format!("Earned {:.2} credits", final_amount),
        };

        let tx = CreditTransaction {
            id: tx_id,
            peer_id: peer_id.to_string(),
            amount: final_amount,
            source,
            timestamp: chrono::Utc::now().to_rfc3339(),
            description,
            balance_after,
        };

        ledger.transactions.push(tx.clone());
        ledger.total_credits_issued += final_amount;
        ledger.save(&self.ledger_path)?;

        tracing::info!("[CREDITS] ✅ {} earned {:.2} credits (balance: {:.2})",
            &peer_id[..peer_id.len().min(12)], final_amount, balance_after);

        Ok(tx)
    }

    // ── Spending ──

    /// Spend credits. Returns error if insufficient balance.
    /// NOTE: Even with zero credits, peers can still use the mesh (queued).
    /// This is only for priority/marketplace purchases.
    pub fn spend(&self, peer_id: &str, service: &str, amount: f64) -> Result<CreditTransaction, String> {
        if amount <= 0.0 {
            return Err("Spend amount must be positive".into());
        }

        let mut ledger = CreditLedger::load(&self.ledger_path);

        let account = ledger.accounts.get_mut(peer_id)
            .ok_or_else(|| format!("No credit account for peer {}", peer_id))?;

        if account.balance < amount {
            return Err(format!(
                "Insufficient credits: have {:.2}, need {:.2}",
                account.balance, amount
            ));
        }

        account.balance -= amount;
        account.lifetime_spent += amount;
        let balance_after = account.balance;

        let tx_id = ledger.next_tx_id();
        let tx = CreditTransaction {
            id: tx_id,
            peer_id: peer_id.to_string(),
            amount: -amount,
            source: CreditSource::Spent { service: service.to_string() },
            timestamp: chrono::Utc::now().to_rfc3339(),
            description: format!("Spent {:.2} on {}", amount, service),
            balance_after,
        };

        ledger.transactions.push(tx.clone());
        ledger.total_credits_spent += amount;
        ledger.save(&self.ledger_path)?;

        tracing::info!("[CREDITS] 💳 {} spent {:.2} credits on {} (balance: {:.2})",
            &peer_id[..peer_id.len().min(12)], amount, service, balance_after);

        Ok(tx)
    }

    // ── Community Reputation ──

    /// Record a community behaviour vote. Positive votes increase reputation.
    pub fn record_community_vote(&self, peer_id: &str, voter_id: &str, positive: bool) -> Result<CreditTransaction, String> {
        let source = CreditSource::CommunityVote {
            voter_id: voter_id.to_string(),
            positive,
        };

        // Earn credits for receiving a positive vote
        if positive {
            let tx = self.earn(peer_id, source, self.config.community_vote_earn)?;

            // Update reputation score
            let mut ledger = CreditLedger::load(&self.ledger_path);
            if let Some(account) = ledger.accounts.get_mut(peer_id) {
                // Reputation asymptotically approaches 1.0 with positive votes
                account.reputation_score = account.reputation_score + (1.0 - account.reputation_score) * 0.05;
                let _ = ledger.save(&self.ledger_path);
            }

            Ok(tx)
        } else {
            // Negative vote reduces reputation but doesn't cost credits
            let mut ledger = CreditLedger::load(&self.ledger_path);
            if let Some(account) = ledger.accounts.get_mut(peer_id) {
                account.reputation_score = (account.reputation_score - 0.05).max(0.0);
                let _ = ledger.save(&self.ledger_path);
            }

            // Return a zero-amount transaction record
            let tx_id = ledger.next_tx_id();
            let tx = CreditTransaction {
                id: tx_id,
                peer_id: peer_id.to_string(),
                amount: 0.0,
                source,
                timestamp: chrono::Utc::now().to_rfc3339(),
                description: "Negative community vote (reputation reduced)".to_string(),
                balance_after: ledger.accounts.get(peer_id).map(|a| a.balance).unwrap_or(0.0),
            };

            ledger.transactions.push(tx.clone());
            let _ = ledger.save(&self.ledger_path);
            Ok(tx)
        }
    }

    // ── Queries ──

    /// Transaction history for a peer.
    pub fn history(&self, peer_id: &str, limit: usize) -> Vec<CreditTransaction> {
        let ledger = CreditLedger::load(&self.ledger_path);
        ledger.transactions.iter()
            .rev()
            .filter(|tx| tx.peer_id == peer_id)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Global leaderboard — top earners by lifetime earned.
    pub fn leaderboard(&self, limit: usize) -> Vec<(String, f64, f64)> {
        let ledger = CreditLedger::load(&self.ledger_path);
        let mut accounts: Vec<_> = ledger.accounts.values()
            .map(|a| (a.peer_id.clone(), a.lifetime_earned, a.reputation_score))
            .collect();

        accounts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        accounts.into_iter().take(limit).collect()
    }

    /// Overall credits system stats.
    pub fn stats(&self) -> serde_json::Value {
        let ledger = CreditLedger::load(&self.ledger_path);
        let total_balance: f64 = ledger.accounts.values().map(|a| a.balance).sum();
        let avg_reputation: f64 = if ledger.accounts.is_empty() {
            0.0
        } else {
            ledger.accounts.values().map(|a| a.reputation_score).sum::<f64>()
                / ledger.accounts.len() as f64
        };

        serde_json::json!({
            "total_accounts": ledger.accounts.len(),
            "total_credits_issued": ledger.total_credits_issued,
            "total_credits_spent": ledger.total_credits_spent,
            "total_credits_in_circulation": total_balance,
            "total_transactions": ledger.transactions.len(),
            "average_reputation": format!("{:.2}", avg_reputation),
            "config": {
                "welcome_bonus": self.config.welcome_bonus,
                "compute_earn_per_1k": self.config.compute_earn_per_1k,
                "network_earn_per_100": self.config.network_earn_per_100,
                "idle_earn_per_hour": self.config.idle_earn_per_hour,
                "high_demand_multiplier": self.config.high_demand_multiplier,
            },
        })
    }

    // ── Convenience Earning Methods ──

    /// Credit a peer for providing compute.
    pub fn earn_compute(&self, peer_id: &str, tokens_served: u64, demand_multiplier: f64) -> Result<CreditTransaction, String> {
        let base = (tokens_served as f64 / 1000.0) * self.config.compute_earn_per_1k;
        self.earn(peer_id, CreditSource::ComputeProvided {
            tokens_served,
            demand_multiplier,
        }, base)
    }

    /// Credit a peer for providing network relay.
    pub fn earn_network(&self, peer_id: &str, requests_relayed: u64, demand_multiplier: f64) -> Result<CreditTransaction, String> {
        let base = (requests_relayed as f64 / 100.0) * self.config.network_earn_per_100;
        self.earn(peer_id, CreditSource::NetworkProvided {
            requests_relayed,
            demand_multiplier,
        }, base)
    }

    /// Credit a peer for idle contribution (staying connected).
    pub fn earn_idle(&self, peer_id: &str, hours: f64) -> Result<CreditTransaction, String> {
        let base = hours * self.config.idle_earn_per_hour;
        self.earn(peer_id, CreditSource::IdleContribution {
            hours_connected: hours,
        }, base)
    }

    /// Credit a peer for a code contribution.
    pub fn earn_code_contribution(&self, peer_id: &str, pr_id: &str, lines_changed: u32) -> Result<CreditTransaction, String> {
        let base = self.config.code_contribution_base * (1.0 + lines_changed as f64 / 100.0);
        self.earn(peer_id, CreditSource::CodeContribution {
            pr_id: pr_id.to_string(),
            lines_changed,
        }, base)
    }

    /// Credit a peer for sharing on social media.
    pub fn earn_social_share(&self, peer_id: &str, platform: &str, reference_url: &str) -> Result<CreditTransaction, String> {
        self.earn(peer_id, CreditSource::SocialShare {
            platform: platform.to_string(),
            reference_url: reference_url.to_string(),
        }, self.config.social_share_earn)
    }

    /// Credit a peer for governance participation.
    pub fn earn_governance_vote(&self, peer_id: &str, proposal_id: &str) -> Result<CreditTransaction, String> {
        self.earn(peer_id, CreditSource::GovernanceParticipation {
            proposal_id: proposal_id.to_string(),
        }, self.config.governance_vote_earn)
    }

    /// Credit a peer for content contribution.
    pub fn earn_content(&self, peer_id: &str, content_type: &str) -> Result<CreditTransaction, String> {
        self.earn(peer_id, CreditSource::ContentContribution {
            content_type: content_type.to_string(),
        }, self.config.content_contribution_earn)
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_engine() -> (CreditsEngine, PathBuf) {
        let path = std::env::temp_dir().join(format!("hive_credits_test_{}", uuid::Uuid::new_v4()));
        let engine = CreditsEngine::new_with_path(path.clone());
        (engine, path)
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_new_account_gets_welcome_bonus() {
        let (engine, path) = test_engine();
        let account = engine.get_or_create_account("peer_alice");
        assert_eq!(account.balance, 100.0); // Default welcome bonus
        assert_eq!(account.lifetime_earned, 100.0);
        cleanup(&path);
    }

    #[test]
    fn test_earn_compute_credits() {
        let (engine, path) = test_engine();
        engine.get_or_create_account("peer_bob");

        let tx = engine.earn_compute("peer_bob", 5000, 1.0).unwrap();
        assert_eq!(tx.amount, 10.0); // 5K tokens × 2.0/1K = 10.0
        assert_eq!(engine.balance("peer_bob"), 110.0); // 100 welcome + 10 earned
        cleanup(&path);
    }

    #[test]
    fn test_earn_with_demand_multiplier() {
        let (engine, path) = test_engine();
        engine.get_or_create_account("peer_carol");

        let tx = engine.earn_compute("peer_carol", 1000, 1.5).unwrap();
        assert_eq!(tx.amount, 3.0); // 1K tokens × 2.0/1K × 1.5 = 3.0
        cleanup(&path);
    }

    #[test]
    fn test_spend_credits() {
        let (engine, path) = test_engine();
        engine.get_or_create_account("peer_dave");

        let tx = engine.spend("peer_dave", "marketplace_purchase", 25.0).unwrap();
        assert_eq!(tx.amount, -25.0);
        assert_eq!(engine.balance("peer_dave"), 75.0); // 100 - 25
        cleanup(&path);
    }

    #[test]
    fn test_insufficient_balance() {
        let (engine, path) = test_engine();
        engine.get_or_create_account("peer_eve");

        let result = engine.spend("peer_eve", "expensive_item", 500.0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Insufficient"));
        cleanup(&path);
    }

    #[test]
    fn test_social_share_daily_cap() {
        let (engine, path) = test_engine();
        engine.get_or_create_account("peer_frank");

        // Should succeed 5 times (default cap)
        for i in 0..5 {
            let result = engine.earn_social_share(
                "peer_frank",
                "twitter",
                &format!("https://twitter.com/share_{}", i),
            );
            assert!(result.is_ok(), "Share {} should succeed", i);
        }

        // 6th should fail
        let result = engine.earn_social_share("peer_frank", "twitter", "https://twitter.com/share_6");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("limit reached"));
        cleanup(&path);
    }

    #[test]
    fn test_community_vote_reputation() {
        let (engine, path) = test_engine();
        engine.get_or_create_account("peer_grace");

        // Positive vote increases reputation
        engine.record_community_vote("peer_grace", "voter_1", true).unwrap();
        let account = engine.account("peer_grace").unwrap();
        assert!(account.reputation_score > 0.5);

        // Negative vote decreases reputation
        engine.record_community_vote("peer_grace", "voter_2", false).unwrap();
        let account2 = engine.account("peer_grace").unwrap();
        assert!(account2.reputation_score < account.reputation_score);
        cleanup(&path);
    }

    #[test]
    fn test_history() {
        let (engine, path) = test_engine();
        engine.get_or_create_account("peer_hank");
        engine.earn_compute("peer_hank", 1000, 1.0).unwrap();
        engine.earn_network("peer_hank", 200, 1.0).unwrap();

        let history = engine.history("peer_hank", 10);
        assert_eq!(history.len(), 3); // welcome + compute + network
        cleanup(&path);
    }

    #[test]
    fn test_leaderboard() {
        let (engine, path) = test_engine();
        engine.get_or_create_account("peer_a");
        engine.get_or_create_account("peer_b");
        engine.earn_compute("peer_a", 10000, 1.0).unwrap();

        let board = engine.leaderboard(10);
        assert_eq!(board.len(), 2);
        assert_eq!(board[0].0, "peer_a"); // peer_a earned more
        cleanup(&path);
    }

    #[test]
    fn test_stats() {
        let (engine, path) = test_engine();
        engine.get_or_create_account("peer_stats");
        let stats = engine.stats();
        assert_eq!(stats["total_accounts"], 1);
        assert!(stats["total_credits_issued"].as_f64().unwrap() > 0.0);
        cleanup(&path);
    }

    #[test]
    fn test_earn_all_sources() {
        let (engine, path) = test_engine();
        engine.get_or_create_account("peer_all");

        assert!(engine.earn_compute("peer_all", 1000, 1.0).is_ok());
        assert!(engine.earn_network("peer_all", 100, 1.0).is_ok());
        assert!(engine.earn_idle("peer_all", 2.0).is_ok());
        assert!(engine.earn_code_contribution("peer_all", "PR-123", 250).is_ok());
        assert!(engine.earn_social_share("peer_all", "reddit", "https://reddit.com/r/hive").is_ok());
        assert!(engine.earn_governance_vote("peer_all", "prop_001").is_ok());
        assert!(engine.earn_content("peer_all", "lesson").is_ok());

        let balance = engine.balance("peer_all");
        assert!(balance > 100.0); // More than welcome bonus
        cleanup(&path);
    }
}
