/// Trust System — Binary attestation gate for mesh peers.
///
/// Open mesh: ALL attested peers can share everything (weights, code, lessons).
/// Safety comes from inherent robustness, not trust tiers:
/// - Binary attestation (SHA-256 challenge-response)
/// - PII sanitization (regex scan + strip)
/// - Sanctions/quarantine (3 violations → quarantine)
/// - Signed envelopes (ed25519 per message)
/// - Cargo test gate (code patches must pass all tests)
/// - Integrity watchdog (60s binary hash re-verify)
/// - Self-destruct on tamper detection
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::network::messages::PeerId;

/// Trust levels — binary: either attested or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TrustLevel {
    /// Failed binary attestation or unknown hash. Silently dropped.
    Unattested = 0,
    /// Passed binary attestation. Full mesh participant — can share everything.
    Attested = 1,
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustLevel::Unattested => write!(f, "Unattested"),
            TrustLevel::Attested => write!(f, "Attested"),
        }
    }
}

/// Per-peer trust record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerTrust {
    pub peer_id: PeerId,
    pub level: TrustLevel,
    pub first_seen: String,          // RFC3339
    pub last_seen: String,           // RFC3339
    pub valid_messages: u64,         // Count of schema-valid signed messages received
    pub violations: u32,             // Lifetime violation count
    pub last_violation: Option<String>, // RFC3339
    pub attestation_verified: bool,  // Has passed challenge-response attestation
    pub binary_hash: Option<String>, // Last known binary hash
}

impl PeerTrust {
    pub fn new_unattested(peer_id: PeerId) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            peer_id,
            level: TrustLevel::Unattested,
            first_seen: now.clone(),
            last_seen: now,
            valid_messages: 0,
            violations: 0,
            last_violation: None,
            attestation_verified: false,
            binary_hash: None,
        }
    }

    /// Record a valid message from this peer.
    pub fn record_valid_message(&mut self) {
        self.valid_messages += 1;
        self.last_seen = chrono::Utc::now().to_rfc3339();
    }

    /// Record a successful attestation — peer is now fully trusted.
    pub fn record_attestation(&mut self, binary_hash: &str) {
        self.attestation_verified = true;
        self.binary_hash = Some(binary_hash.to_string());
        if self.level < TrustLevel::Attested {
            self.level = TrustLevel::Attested;
            tracing::info!("[TRUST] ✅ Peer {} attested — full mesh access granted", self.peer_id);
        }
    }

    /// Record a violation. Demotes to Unattested (requires re-attestation).
    pub fn record_violation(&mut self) {
        self.violations += 1;
        self.last_violation = Some(chrono::Utc::now().to_rfc3339());

        if self.level > TrustLevel::Unattested {
            tracing::warn!("[TRUST] ⬇️ Peer {} demoted to Unattested (violation #{})",
                self.peer_id, self.violations);
            self.level = TrustLevel::Unattested;
            self.attestation_verified = false;
        }
    }
}

/// Trust store — manages attestation status for all known peers.
pub struct TrustStore {
    peers: HashMap<PeerId, PeerTrust>,
    persist_path: std::path::PathBuf,
}

impl TrustStore {
    pub fn new(persist_dir: &std::path::Path) -> Self {
        let persist_path = persist_dir.join("trust.json");
        let _ = std::fs::create_dir_all(persist_dir);

        let peers = if persist_path.exists() {
            std::fs::read_to_string(&persist_path)
                .ok()
                .and_then(|s| serde_json::from_str::<HashMap<PeerId, PeerTrust>>(&s).ok())
                .unwrap_or_default()
        } else {
            HashMap::new()
        };

        Self { peers, persist_path }
    }

    /// Get or create trust record for a peer.
    pub fn get_or_create(&mut self, peer_id: &PeerId) -> &mut PeerTrust {
        if !self.peers.contains_key(peer_id) {
            let trust = PeerTrust::new_unattested(peer_id.clone());
            self.peers.insert(peer_id.clone(), trust);
        }
        self.peers.get_mut(peer_id).unwrap()
    }

    /// Get trust level for a peer.
    pub fn trust_level(&self, peer_id: &PeerId) -> TrustLevel {
        self.peers.get(peer_id)
            .map(|t| t.level)
            .unwrap_or(TrustLevel::Unattested)
    }

    /// Open mesh: all attested peers can share everything.
    pub fn can_share_lessons(&self, peer_id: &PeerId) -> bool {
        self.trust_level(peer_id) >= TrustLevel::Attested
    }

    pub fn can_share_golden(&self, peer_id: &PeerId) -> bool {
        self.trust_level(peer_id) >= TrustLevel::Attested
    }

    pub fn can_share_weights(&self, peer_id: &PeerId) -> bool {
        self.trust_level(peer_id) >= TrustLevel::Attested
    }

    pub fn can_share_code(&self, peer_id: &PeerId) -> bool {
        self.trust_level(peer_id) >= TrustLevel::Attested
    }

    /// Persist trust state to disk.
    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.peers) {
            let _ = std::fs::write(&self.persist_path, json);
        }
    }

    /// Get all known peers with their trust levels.
    pub fn all_peers(&self) -> Vec<&PeerTrust> {
        self.peers.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_peer() -> PeerId {
        PeerId("test_peer_abc123def456".to_string())
    }

    #[test]
    fn test_new_peer_is_unattested() {
        let trust = PeerTrust::new_unattested(test_peer());
        assert_eq!(trust.level, TrustLevel::Unattested);
        assert!(!trust.attestation_verified);
    }

    #[test]
    fn test_attestation_promotes_to_attested() {
        let mut trust = PeerTrust::new_unattested(test_peer());
        trust.record_attestation("abc123hash");
        assert_eq!(trust.level, TrustLevel::Attested);
        assert!(trust.attestation_verified);
    }

    #[test]
    fn test_violation_demotes_to_unattested() {
        let mut trust = PeerTrust::new_unattested(test_peer());
        trust.record_attestation("abc123hash");
        assert_eq!(trust.level, TrustLevel::Attested);
        trust.record_violation();
        assert_eq!(trust.level, TrustLevel::Unattested);
        assert!(!trust.attestation_verified);
        assert_eq!(trust.violations, 1);
    }

    #[test]
    fn test_trust_level_ordering() {
        assert!(TrustLevel::Attested > TrustLevel::Unattested);
    }

    #[test]
    fn test_trust_store_new_peer() {
        let tmp = std::env::temp_dir().join(format!("hive_trust_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let mut store = TrustStore::new(&tmp);
        let peer = test_peer();
        let trust = store.get_or_create(&peer);
        assert_eq!(trust.level, TrustLevel::Unattested);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_attested_can_share_everything() {
        let tmp = std::env::temp_dir().join(format!("hive_trust_perm_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let mut store = TrustStore::new(&tmp);
        let peer = test_peer();

        // Unattested cannot share
        assert!(!store.can_share_lessons(&peer));
        assert!(!store.can_share_golden(&peer));
        assert!(!store.can_share_weights(&peer));
        assert!(!store.can_share_code(&peer));

        // Attest the peer
        store.get_or_create(&peer).record_attestation("hash");

        // Attested can share everything
        assert!(store.can_share_lessons(&peer));
        assert!(store.can_share_golden(&peer));
        assert!(store.can_share_weights(&peer));
        assert!(store.can_share_code(&peer));

        std::fs::remove_dir_all(&tmp).ok();
    }
}
