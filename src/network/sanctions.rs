/// Sanctions System — Violation tracking and quarantine enforcement.
///
/// Detects and quarantines compromised or malicious peers.
/// Quarantine is permanent for attestation failures and PII leaks.
/// Network-wide quarantine propagation ensures the entire mesh is protected.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use crate::network::messages::PeerId;

/// Types of violations a peer can commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Violation {
    AttestationFailure,
    AttestationTimeout,
    InvalidSignature,
    PIIDetected { field: String },
    MalformedMessage,
    RateLimitExceeded,
    PoisonAttempt,
    BinaryHashChanged,
    OversizedPayload { size: usize },
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AttestationFailure => write!(f, "AttestationFailure"),
            Self::AttestationTimeout => write!(f, "AttestationTimeout"),
            Self::InvalidSignature => write!(f, "InvalidSignature"),
            Self::PIIDetected { field } => write!(f, "PIIDetected({})", field),
            Self::MalformedMessage => write!(f, "MalformedMessage"),
            Self::RateLimitExceeded => write!(f, "RateLimitExceeded"),
            Self::PoisonAttempt => write!(f, "PoisonAttempt"),
            Self::BinaryHashChanged => write!(f, "BinaryHashChanged"),
            Self::OversizedPayload { size } => write!(f, "OversizedPayload({}B)", size),
        }
    }
}

/// A recorded violation with timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationRecord {
    pub violation: Violation,
    pub at: String, // RFC3339
}

/// Sanctions store — tracks violations and enforces quarantine.
pub struct SanctionStore {
    violations: HashMap<PeerId, Vec<ViolationRecord>>,
    quarantined: HashSet<PeerId>,
    persist_path: std::path::PathBuf,
}

impl SanctionStore {
    pub fn new(persist_dir: &std::path::Path) -> Self {
        let persist_path = persist_dir.join("quarantine.json");
        let _ = std::fs::create_dir_all(persist_dir);

        let quarantined = if persist_path.exists() {
            std::fs::read_to_string(&persist_path)
                .ok()
                .and_then(|s| serde_json::from_str::<HashSet<PeerId>>(&s).ok())
                .unwrap_or_default()
        } else {
            HashSet::new()
        };

        Self {
            violations: HashMap::new(),
            quarantined,
            persist_path,
        }
    }

    /// Check if a peer is quarantined.
    pub fn is_quarantined(&self, peer_id: &PeerId) -> bool {
        self.quarantined.contains(peer_id)
    }

    /// Record a violation for a peer. Returns true if the peer should be quarantined.
    pub fn record_violation(&mut self, peer_id: &PeerId, violation: Violation) -> bool {
        let now = chrono::Utc::now();
        let record = ViolationRecord {
            violation: violation.clone(),
            at: now.to_rfc3339(),
        };

        tracing::warn!("[SANCTIONS] 🚨 Violation from {}: {}", peer_id, violation);

        // Check for instant-quarantine violations
        let instant_quarantine = matches!(
            violation,
            Violation::AttestationFailure
            | Violation::BinaryHashChanged
            | Violation::InvalidSignature
            | Violation::PIIDetected { .. }
        );

        if instant_quarantine {
            tracing::error!("[SANCTIONS] ⛔ INSTANT QUARANTINE for {}: {}", peer_id, violation);
            self.quarantine(peer_id);
            return true;
        }

        // Accumulate violation
        let records = self.violations.entry(peer_id.clone()).or_default();
        records.push(record);

        // Check thresholds
        let one_hour_ago = now - chrono::Duration::hours(1);
        let one_day_ago = now - chrono::Duration::hours(24);

        let violations_1h = records.iter()
            .filter(|r| {
                chrono::DateTime::parse_from_rfc3339(&r.at)
                    .map(|dt| dt.with_timezone(&chrono::Utc) > one_hour_ago)
                    .unwrap_or(false)
            })
            .count();

        let violations_24h = records.iter()
            .filter(|r| {
                chrono::DateTime::parse_from_rfc3339(&r.at)
                    .map(|dt| dt.with_timezone(&chrono::Utc) > one_day_ago)
                    .unwrap_or(false)
            })
            .count();

        // 3 violations in 1h → quarantine
        if violations_1h >= 3 {
            tracing::error!("[SANCTIONS] ⛔ QUARANTINE for {} (3+ violations in 1h)", peer_id);
            self.quarantine(peer_id);
            return true;
        }

        // 5 violations in 24h → permanent quarantine
        if violations_24h >= 5 {
            tracing::error!("[SANCTIONS] ⛔ PERMANENT QUARANTINE for {} (5+ violations in 24h)", peer_id);
            self.quarantine(peer_id);
            return true;
        }

        false
    }

    /// Quarantine a peer and persist.
    fn quarantine(&mut self, peer_id: &PeerId) {
        self.quarantined.insert(peer_id.clone());
        self.save();
    }

    /// Get all quarantined peer IDs.
    pub fn quarantined_peers(&self) -> &HashSet<PeerId> {
        &self.quarantined
    }

    /// Get violation count for a peer.
    pub fn violation_count(&self, peer_id: &PeerId) -> usize {
        self.violations.get(peer_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Persist quarantine list to disk.
    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.quarantined) {
            let _ = std::fs::write(&self.persist_path, json);
        }
    }

    /// Rate limiter: check if peer has exceeded 10 messages/minute.
    pub fn check_rate_limit(&self, peer_id: &PeerId) -> bool {
        let records = match self.violations.get(peer_id) {
            Some(r) => r,
            None => return false,
        };
        let one_min_ago = chrono::Utc::now() - chrono::Duration::minutes(1);
        let recent = records.iter()
            .filter(|r| matches!(r.violation, Violation::RateLimitExceeded))
            .filter(|r| {
                chrono::DateTime::parse_from_rfc3339(&r.at)
                    .map(|dt| dt.with_timezone(&chrono::Utc) > one_min_ago)
                    .unwrap_or(false)
            })
            .count();
        recent >= 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_peer() -> PeerId {
        PeerId("sanctions_test_peer".to_string())
    }

    #[test]
    fn test_instant_quarantine_attestation() {
        let tmp = std::env::temp_dir().join(format!("hive_sanctions_1_{}", std::process::id()));
        let mut store = SanctionStore::new(&tmp);
        let peer = test_peer();

        let should_quarantine = store.record_violation(&peer, Violation::AttestationFailure);
        assert!(should_quarantine);
        assert!(store.is_quarantined(&peer));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_instant_quarantine_pii() {
        let tmp = std::env::temp_dir().join(format!("hive_sanctions_2_{}", std::process::id()));
        let mut store = SanctionStore::new(&tmp);
        let peer = test_peer();

        let should = store.record_violation(&peer, Violation::PIIDetected { field: "lesson.text".into() });
        assert!(should);
        assert!(store.is_quarantined(&peer));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_gradual_quarantine() {
        let tmp = std::env::temp_dir().join(format!("hive_sanctions_3_{}", std::process::id()));
        let mut store = SanctionStore::new(&tmp);
        let peer = test_peer();

        // First two violations: no quarantine
        assert!(!store.record_violation(&peer, Violation::MalformedMessage));
        assert!(!store.record_violation(&peer, Violation::MalformedMessage));

        // Third in 1h: quarantine
        assert!(store.record_violation(&peer, Violation::MalformedMessage));
        assert!(store.is_quarantined(&peer));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_persistence() {
        let tmp = std::env::temp_dir().join(format!("hive_sanctions_4_{}", std::process::id()));
        let peer = test_peer();

        {
            let mut store = SanctionStore::new(&tmp);
            store.record_violation(&peer, Violation::InvalidSignature);
        }

        // Reload
        let store2 = SanctionStore::new(&tmp);
        assert!(store2.is_quarantined(&peer));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_non_quarantined_peer() {
        let tmp = std::env::temp_dir().join(format!("hive_sanctions_5_{}", std::process::id()));
        let store = SanctionStore::new(&tmp);
        assert!(!store.is_quarantined(&test_peer()));

        std::fs::remove_dir_all(&tmp).ok();
    }
}
