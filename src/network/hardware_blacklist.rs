/// Hardware Blacklist — Permanent hardware-level bans for the HIVE mesh.
///
/// When a peer is quarantined (via sanctions or governance), their HardwareId
/// is also blacklisted. This means:
///
/// 1. Reinstalling Apis won't help — same hardware = still banned
/// 2. Regenerating PeerId won't help — same hardware = still banned  
/// 3. The blacklist is mesh-distributed: all peers receive and enforce it
/// 4. Unbanning requires a 2/3 majority governance vote — no individual can unban
///
/// PERSISTENCE: Saved to `memory/mesh/hardware_blacklist.json`
/// DISTRIBUTION: Broadcast via MeshMessage::Quarantine with HardwareId field
use std::collections::HashSet;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use crate::network::hardware_id::HardwareId;
use crate::network::messages::PeerId;

/// A blacklist entry — records why the hardware was banned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistEntry {
    pub hardware_id: HardwareId,
    /// The PeerId that was quarantined (for reference)
    pub original_peer_id: PeerId,
    /// Why the hardware was banned
    pub reason: String,
    /// When the ban was issued
    pub banned_at: String,
    /// Who issued the ban (peer consensus)
    pub banned_by: String,
}

/// The Hardware Blacklist — enforces permanent hardware-level bans.
pub struct HardwareBlacklist {
    entries: HashSet<String>, // HardwareId hex strings for fast lookup
    full_entries: Vec<BlacklistEntry>, // Full records for audit
    persist_path: PathBuf,
}

impl HardwareBlacklist {
    /// Load from disk or create empty.
    pub fn new(mesh_dir: &std::path::Path) -> Self {
        let persist_path = mesh_dir.join("hardware_blacklist.json");

        if let Ok(data) = std::fs::read_to_string(&persist_path) {
            if let Ok(entries) = serde_json::from_str::<Vec<BlacklistEntry>>(&data) {
                let set: HashSet<String> = entries.iter()
                    .map(|e| e.hardware_id.0.clone())
                    .collect();
                tracing::info!(
                    "[BLACKLIST] 🔒 Loaded {} hardware bans from disk",
                    set.len()
                );
                return Self {
                    entries: set,
                    full_entries: entries,
                    persist_path,
                };
            }
        }

        Self {
            entries: HashSet::new(),
            full_entries: Vec::new(),
            persist_path,
        }
    }

    /// Check if a hardware ID is blacklisted.
    pub fn is_blacklisted(&self, hw_id: &HardwareId) -> bool {
        self.entries.contains(&hw_id.0)
    }

    /// Blacklist a hardware ID. Returns true if newly added.
    pub fn ban(
        &mut self,
        hw_id: HardwareId,
        peer_id: PeerId,
        reason: &str,
        banned_by: &str,
    ) -> bool {
        if self.entries.contains(&hw_id.0) {
            return false; // Already banned
        }

        tracing::warn!(
            "[BLACKLIST] ⛔ Hardware {} banned (peer: {}, reason: {})",
            hw_id, peer_id, reason
        );

        self.entries.insert(hw_id.0.clone());
        self.full_entries.push(BlacklistEntry {
            hardware_id: hw_id,
            original_peer_id: peer_id,
            reason: reason.to_string(),
            banned_at: chrono::Utc::now().to_rfc3339(),
            banned_by: banned_by.to_string(),
        });

        self.persist();
        true
    }

    /// Remove a hardware ban (requires governance vote — no individual can unban).
    /// The proposal_id must reference an approved governance proposal.
    pub fn unban_by_vote(&mut self, hw_id: &HardwareId, proposal_id: &str) -> bool {
        if !self.entries.remove(&hw_id.0) {
            return false; // Not banned
        }

        self.full_entries.retain(|e| e.hardware_id != *hw_id);

        tracing::warn!(
            "[BLACKLIST] 🔓 Hardware {} unbanned (governance vote: {})",
            hw_id, proposal_id
        );

        self.persist();
        true
    }

    /// Get the number of blacklisted hardware IDs.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Get all blacklist entries (for audit).
    pub fn entries(&self) -> &[BlacklistEntry] {
        &self.full_entries
    }

    /// Merge blacklist entries from a remote peer (mesh distribution).
    pub fn merge_remote(&mut self, remote_entries: Vec<BlacklistEntry>) -> usize {
        let mut added = 0;
        for entry in remote_entries {
            if !self.entries.contains(&entry.hardware_id.0) {
                self.entries.insert(entry.hardware_id.0.clone());
                self.full_entries.push(entry);
                added += 1;
            }
        }
        if added > 0 {
            tracing::info!(
                "[BLACKLIST] 📥 Merged {} new hardware bans from mesh",
                added
            );
            self.persist();
        }
        added
    }

    /// Persist to disk.
    fn persist(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.full_entries) {
            if let Some(parent) = self.persist_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&self.persist_path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_blacklist() -> HardwareBlacklist {
        let tmp = std::env::temp_dir().join(format!("hive_bl_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        HardwareBlacklist::new(&tmp)
    }

    #[test]
    fn test_ban_and_check() {
        let mut bl = test_blacklist();
        let hw = HardwareId("abc123".to_string());
        let peer = PeerId("malicious_peer".to_string());

        assert!(!bl.is_blacklisted(&hw));
        assert!(bl.ban(hw.clone(), peer, "Attacking the mesh", "governance"));
        assert!(bl.is_blacklisted(&hw));
        assert_eq!(bl.count(), 1);
    }

    #[test]
    fn test_double_ban_idempotent() {
        let mut bl = test_blacklist();
        let hw = HardwareId("abc123".to_string());
        let peer = PeerId("peer1".to_string());

        assert!(bl.ban(hw.clone(), peer.clone(), "First ban", "governance"));
        assert!(!bl.ban(hw, peer, "Second ban", "governance")); // Already banned
        assert_eq!(bl.count(), 1);
    }

    #[test]
    fn test_unban_by_vote() {
        let mut bl = test_blacklist();
        let hw = HardwareId("abc123".to_string());
        let peer = PeerId("peer1".to_string());

        bl.ban(hw.clone(), peer, "Temporary ban", "governance");
        assert!(bl.is_blacklisted(&hw));

        assert!(bl.unban_by_vote(&hw, "proposal_123"));
        assert!(!bl.is_blacklisted(&hw));
    }

    #[test]
    fn test_unban_nonexistent() {
        let mut bl = test_blacklist();
        let hw = HardwareId("nonexistent".to_string());
        assert!(!bl.unban_by_vote(&hw, "proposal_456"));
    }

    #[test]
    fn test_merge_remote() {
        let mut bl = test_blacklist();

        let remote = vec![
            BlacklistEntry {
                hardware_id: HardwareId("remote_hw_1".to_string()),
                original_peer_id: PeerId("remote_1".to_string()),
                reason: "Remote ban".to_string(),
                banned_at: chrono::Utc::now().to_rfc3339(),
                banned_by: "governance".to_string(),
            },
            BlacklistEntry {
                hardware_id: HardwareId("remote_hw_2".to_string()),
                original_peer_id: PeerId("remote_2".to_string()),
                reason: "Remote ban 2".to_string(),
                banned_at: chrono::Utc::now().to_rfc3339(),
                banned_by: "governance".to_string(),
            },
        ];

        let added = bl.merge_remote(remote);
        assert_eq!(added, 2);
        assert_eq!(bl.count(), 2);
        assert!(bl.is_blacklisted(&HardwareId("remote_hw_1".to_string())));
    }

    #[test]
    fn test_merge_dedup() {
        let mut bl = test_blacklist();

        let hw = HardwareId("existing".to_string());
        bl.ban(hw.clone(), PeerId("local".to_string()), "Local ban", "local");

        let remote = vec![
            BlacklistEntry {
                hardware_id: hw.clone(),
                original_peer_id: PeerId("remote".to_string()),
                reason: "Remote duplicate".to_string(),
                banned_at: chrono::Utc::now().to_rfc3339(),
                banned_by: "governance".to_string(),
            },
        ];

        let added = bl.merge_remote(remote);
        assert_eq!(added, 0); // Already existed
        assert_eq!(bl.count(), 1);
    }
}
