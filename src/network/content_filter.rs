/// Content Security Shield — Multi-layer content scanning for all mesh traffic.
///
/// Layers:
/// 1. Hash-based blocking — SHA-256 of known-bad content (CSAM, malware, abuse)
/// 2. Pattern detection — Injection attacks, phishing, social engineering
/// 3. Rate limiting — Per-peer message caps
/// 4. Reputation scoring — Clean messages increase rep, flagged content decreases
///
/// SURVIVABILITY: Community-first, federated approach. The community blocklist
/// works without internet. Official databases (NCMEC/IWF) can be loaded as an
/// optional one-time import.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};

use crate::network::messages::PeerId;

/// Result of scanning content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScanResult {
    /// Content is clean.
    Clean,
    /// Content matches a known-bad hash.
    BlockedHash { category: String, hash: String },
    /// Content matches a dangerous pattern.
    PatternMatch { pattern_type: PatternType, detail: String },
    /// Sender is rate-limited.
    RateLimited { peer: String, cooldown_secs: u64 },
    /// Sender has low reputation.
    LowReputation { peer: String, score: f64 },
}

/// Types of dangerous patterns detected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PatternType {
    PromptInjection,
    SqlInjection,
    XssAttack,
    PhishingUrl,
    HomoglyphAttack,
    SocialEngineering,
    MalwareLink,
}

impl std::fmt::Display for PatternType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PromptInjection => write!(f, "Prompt Injection"),
            Self::SqlInjection => write!(f, "SQL Injection"),
            Self::XssAttack => write!(f, "XSS Attack"),
            Self::PhishingUrl => write!(f, "Phishing URL"),
            Self::HomoglyphAttack => write!(f, "Homoglyph Attack"),
            Self::SocialEngineering => write!(f, "Social Engineering"),
            Self::MalwareLink => write!(f, "Malware Link"),
        }
    }
}

/// Per-peer reputation tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerReputation {
    pub peer_id: String,
    pub score: f64,           // 0.0 to 100.0 (starts at 50.0)
    pub clean_messages: u64,
    pub flagged_messages: u64,
    pub last_updated: String,
}

impl PeerReputation {
    fn new(peer_id: &str) -> Self {
        Self {
            peer_id: peer_id.to_string(),
            score: 50.0,
            clean_messages: 0,
            flagged_messages: 0,
            last_updated: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn record_clean(&mut self) {
        self.clean_messages += 1;
        self.score = (self.score + 0.1).min(100.0);
        self.last_updated = chrono::Utc::now().to_rfc3339();
    }

    fn record_flagged(&mut self) {
        self.flagged_messages += 1;
        self.score = (self.score - 5.0).max(0.0);
        self.last_updated = chrono::Utc::now().to_rfc3339();
    }
}

/// Per-peer rate limiting state.
#[derive(Debug, Clone)]
struct RateState {
    message_count: u64,
    window_start: std::time::Instant,
}

/// The Content Security Shield.
pub struct ContentFilter {
    /// Known-bad content hashes (SHA-256)
    blocked_hashes: Arc<RwLock<HashSet<String>>>,
    /// Peer reputations
    reputations: Arc<RwLock<HashMap<String, PeerReputation>>>,
    /// Per-peer rate limiting
    rate_states: Arc<RwLock<HashMap<String, RateState>>>,
    /// Max messages per peer per window
    rate_limit: u64,
    /// Rate window duration in seconds
    rate_window_secs: u64,
    /// Minimum reputation score to allow messages through
    min_reputation: f64,
    /// Compiled regex patterns for detection
    injection_patterns: Vec<regex::Regex>,
    /// Known phishing TLD patterns
    phishing_tlds: Vec<String>,
}

impl ContentFilter {
    /// Create a new content filter with default settings.
    pub fn new() -> Self {
        let rate_limit = std::env::var("REMOVED_MESH_GOVERNED")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(30); // 30 msgs/min per peer

        let min_reputation = std::env::var("REMOVED_MESH_GOVERNED")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(10.0);

        // Compile injection detection patterns
        let injection_patterns = vec![
            // Prompt injection
            regex::Regex::new(r"(?i)(ignore\s+(all\s+)?previous|disregard\s+(all\s+)?instructions|you\s+are\s+now|new\s+instructions?:)").unwrap(),
            // SQL injection
            regex::Regex::new(r"(?i)(\b(union\s+select|drop\s+table|insert\s+into|delete\s+from|update\s+.+\s+set)\b|;\s*--)").unwrap(),
            // XSS
            regex::Regex::new(r"(?i)(<script[^>]*>|javascript:|on(load|error|click|mouseover)\s*=)").unwrap(),
            // Social engineering
            regex::Regex::new(r"(?i)(send\s+(me\s+)?(your|the)\s+(password|key|token|secret|credentials)|click\s+(here|this\s+link)\s+to\s+verify)").unwrap(),
        ];

        let phishing_tlds = vec![
            ".tk".to_string(), ".ml".to_string(), ".ga".to_string(),
            ".cf".to_string(), ".gq".to_string(), ".buzz".to_string(),
            ".top".to_string(), ".xyz".to_string(),
        ];

        tracing::info!("[CONTENT FILTER] 🛡️ Initialised (rate_limit={}/min, min_rep={})",
            rate_limit, min_reputation);

        Self {
            blocked_hashes: Arc::new(RwLock::new(HashSet::new())),
            reputations: Arc::new(RwLock::new(HashMap::new())),
            rate_states: Arc::new(RwLock::new(HashMap::new())),
            rate_limit,
            rate_window_secs: 60,
            min_reputation,
            injection_patterns,
            phishing_tlds,
        }
    }

    /// Scan content through all security layers.
    /// Returns Clean if all checks pass, or the first failure.
    pub async fn scan(&self, peer_id: &PeerId, content: &str) -> ScanResult {
        // Layer 1: Hash-based blocking
        let hash = self.hash_content(content);
        if self.blocked_hashes.read().await.contains(&hash) {
            self.record_flagged(&peer_id.0).await;
            return ScanResult::BlockedHash {
                category: "blocked_content".to_string(),
                hash,
            };
        }

        // Layer 2: Pattern detection
        if let Some(pattern_result) = self.detect_patterns(content) {
            self.record_flagged(&peer_id.0).await;
            return pattern_result;
        }

        // Layer 3: Rate limiting
        if let Some(rate_result) = self.check_rate_limit(&peer_id.0).await {
            return rate_result;
        }

        // Layer 4: Reputation check
        if let Some(rep_result) = self.check_reputation(&peer_id.0).await {
            return rep_result;
        }

        // All checks passed
        self.record_clean(&peer_id.0).await;
        ScanResult::Clean
    }

    /// Compute SHA-256 hash of content.
    fn hash_content(&self, content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Detect dangerous patterns in content.
    fn detect_patterns(&self, content: &str) -> Option<ScanResult> {
        // Check injection patterns
        let pattern_types = [
            PatternType::PromptInjection,
            PatternType::SqlInjection,
            PatternType::XssAttack,
            PatternType::SocialEngineering,
        ];

        for (regex, pattern_type) in self.injection_patterns.iter().zip(pattern_types.iter()) {
            if let Some(m) = regex.find(content) {
                return Some(ScanResult::PatternMatch {
                    pattern_type: pattern_type.clone(),
                    detail: m.as_str().to_string(),
                });
            }
        }

        // Check for phishing URLs
        for tld in &self.phishing_tlds {
            if content.contains(tld) {
                // More specific: check if it's actually a URL-like pattern
                let url_pattern = format!(r"https?://[^\s]*{}", regex::escape(tld));
                if let Ok(re) = regex::Regex::new(&url_pattern) {
                    if let Some(m) = re.find(content) {
                        return Some(ScanResult::PatternMatch {
                            pattern_type: PatternType::PhishingUrl,
                            detail: m.as_str().to_string(),
                        });
                    }
                }
            }
        }

        // Check for homoglyph attacks (Unicode lookalikes)
        if self.has_mixed_scripts(content) {
            return Some(ScanResult::PatternMatch {
                pattern_type: PatternType::HomoglyphAttack,
                detail: "Mixed Unicode scripts detected in URL-like context".to_string(),
            });
        }

        None
    }

    /// Detect mixed Unicode scripts (potential homoglyph attack).
    fn has_mixed_scripts(&self, content: &str) -> bool {
        // Simple heuristic: check for Cyrillic characters mixed with Latin in URL-like context
        if !content.contains("http") && !content.contains("www.") {
            return false;
        }

        let has_latin = content.chars().any(|c| c.is_ascii_alphabetic());
        let has_cyrillic = content.chars().any(|c| matches!(c, '\u{0400}'..='\u{04FF}'));

        has_latin && has_cyrillic
    }

    /// Check per-peer rate limit.
    async fn check_rate_limit(&self, peer_id: &str) -> Option<ScanResult> {
        let mut states = self.rate_states.write().await;
        let now = std::time::Instant::now();

        let state = states.entry(peer_id.to_string()).or_insert(RateState {
            message_count: 0,
            window_start: now,
        });

        // Reset window if expired
        if now.duration_since(state.window_start).as_secs() >= self.rate_window_secs {
            state.message_count = 0;
            state.window_start = now;
        }

        state.message_count += 1;

        if state.message_count > self.rate_limit {
            let remaining = self.rate_window_secs - now.duration_since(state.window_start).as_secs();
            return Some(ScanResult::RateLimited {
                peer: peer_id.to_string(),
                cooldown_secs: remaining,
            });
        }

        None
    }

    /// Check peer reputation.
    async fn check_reputation(&self, peer_id: &str) -> Option<ScanResult> {
        let reps = self.reputations.read().await;
        if let Some(rep) = reps.get(peer_id) {
            if rep.score < self.min_reputation {
                return Some(ScanResult::LowReputation {
                    peer: peer_id.to_string(),
                    score: rep.score,
                });
            }
        }
        None
    }

    /// Record a clean message for a peer.
    async fn record_clean(&self, peer_id: &str) {
        let mut reps = self.reputations.write().await;
        let rep = reps.entry(peer_id.to_string())
            .or_insert_with(|| PeerReputation::new(peer_id));
        rep.record_clean();
    }

    /// Record a flagged message for a peer.
    async fn record_flagged(&self, peer_id: &str) {
        let mut reps = self.reputations.write().await;
        let rep = reps.entry(peer_id.to_string())
            .or_insert_with(|| PeerReputation::new(peer_id));
        rep.record_flagged();
    }

    /// Add a blocked hash (e.g., from community blocklist or imported database).
    pub async fn add_blocked_hash(&self, hash: String) {
        self.blocked_hashes.write().await.insert(hash);
    }

    /// Import a batch of blocked hashes (e.g., from file).
    pub async fn import_blocked_hashes(&self, hashes: Vec<String>) {
        let count = hashes.len();
        let mut blocked = self.blocked_hashes.write().await;
        for hash in hashes {
            blocked.insert(hash);
        }
        tracing::info!("[CONTENT FILTER] 📥 Imported {} blocked hashes (total: {})", count, blocked.len());
    }

    /// Get peer reputation.
    pub async fn get_reputation(&self, peer_id: &str) -> Option<PeerReputation> {
        self.reputations.read().await.get(peer_id).cloned()
    }

    /// Get all reputations.
    pub async fn all_reputations(&self) -> Vec<PeerReputation> {
        self.reputations.read().await.values().cloned().collect()
    }

    /// Get filter stats.
    pub async fn stats(&self) -> serde_json::Value {
        let blocked_count = self.blocked_hashes.read().await.len();
        let peer_count = self.reputations.read().await.len();
        let reps = self.reputations.read().await;
        let avg_rep = if peer_count > 0 {
            reps.values().map(|r| r.score).sum::<f64>() / peer_count as f64
        } else {
            50.0
        };

        serde_json::json!({
            "blocked_hashes": blocked_count,
            "tracked_peers": peer_count,
            "average_reputation": format!("{:.1}", avg_rep),
            "rate_limit": self.rate_limit,
            "min_reputation": self.min_reputation,
            "pattern_rules": self.injection_patterns.len(),
        })
    }
}


#[cfg(test)]
#[path = "content_filter_tests.rs"]
mod tests;
