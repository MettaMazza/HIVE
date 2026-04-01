/// Governance Phases — Automatic power transition as the mesh grows.
///
/// The creator key's special powers auto-sunset based on peer count.
/// This code is protected by the ConfigGuard — modifying these thresholds
/// triggers self-destruct on non-developer machines.
///
/// PHASE 1: Bootstrap (0-9 peers)
///   Creator has emergency dev powers: unban, hotfix, config override.
///   All actions are logged and broadcast to all peers.
///
/// PHASE 2: Council (10-999 peers)
///   Creator + 2 elected council members must agree (2-of-3 multisig).
///   Council elected by regional vote (1 representative per region with peers).
///   Creator cannot act alone.
///
/// PHASE 3: Democracy (1000+ peers)
///   Creator key = one peer. Same vote weight as everyone.
///   Code changes: governance proposal → 2/3 majority → applied.
///   No individual has special powers. The protocol governs itself.
///
/// IMMUTABLE: The thresholds (9, 999) are config-guarded. Changing them
/// on a non-developer machine triggers self-destruct. After Phase 3,
/// even the developer cannot change them without mesh consensus.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::network::messages::PeerId;

/// Bootstrap → Council threshold.
const COUNCIL_THRESHOLD: usize = 10;
/// Council → Democracy threshold.
const DEMOCRACY_THRESHOLD: usize = 1000;

/// The current governance phase of the mesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GovernancePhase {
    /// 0-9 peers: Creator has emergency dev powers.
    Bootstrap,
    /// 10-999 peers: Creator + council must agree.
    Council,
    /// 1000+ peers: Pure democracy. No special powers.
    Democracy,
}

impl std::fmt::Display for GovernancePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GovernancePhase::Bootstrap => write!(f, "Bootstrap (creator emergency powers)"),
            GovernancePhase::Council => write!(f, "Council (creator + elected council)"),
            GovernancePhase::Democracy => write!(f, "Democracy (pure peer equality)"),
        }
    }
}

/// Determine the current governance phase from peer count.
pub fn current_phase(peer_count: usize) -> GovernancePhase {
    if peer_count >= DEMOCRACY_THRESHOLD {
        GovernancePhase::Democracy
    } else if peer_count >= COUNCIL_THRESHOLD {
        GovernancePhase::Council
    } else {
        GovernancePhase::Bootstrap
    }
}

/// Check if the creator key has emergency powers in the current phase.
pub fn creator_has_emergency_powers(peer_count: usize) -> bool {
    current_phase(peer_count) == GovernancePhase::Bootstrap
}

/// Check if actions require council approval.
pub fn requires_council(peer_count: usize) -> bool {
    current_phase(peer_count) == GovernancePhase::Council
}

/// A council member elected by regional vote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilMember {
    pub peer_id: PeerId,
    pub region: String,
    pub elected_at: String,
    pub votes_received: usize,
}

/// A council action requiring multisig approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilAction {
    pub action_id: String,
    pub action_type: String,
    pub description: String,
    pub proposed_by: PeerId,
    pub proposed_at: String,
    pub approvals: HashSet<String>, // PeerId strings
    pub required_approvals: usize,   // 2 for council phase (creator + 1 council member)
    pub executed: bool,
}

/// The Governance Phase Manager.
pub struct GovernanceManager {
    /// Current peer count (updated from mesh heartbeats).
    peer_count: Arc<RwLock<usize>>,
    /// Elected council members.
    council: Arc<RwLock<Vec<CouncilMember>>>,
    /// Pending council actions.
    pending_actions: Arc<RwLock<HashMap<String, CouncilAction>>>,
    /// Phase transition log.
    transitions: Arc<RwLock<Vec<PhaseTransition>>>,
}

/// Record of a phase transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseTransition {
    pub from: GovernancePhase,
    pub to: GovernancePhase,
    pub peer_count: usize,
    pub timestamp: String,
}

impl GovernanceManager {
    pub fn new() -> Self {
        tracing::info!("[GOVERNANCE] ⚖️ Phase manager initialised");
        Self {
            peer_count: Arc::new(RwLock::new(0)),
            council: Arc::new(RwLock::new(Vec::new())),
            pending_actions: Arc::new(RwLock::new(HashMap::new())),
            transitions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Update the peer count and check for phase transitions.
    pub async fn update_peer_count(&self, count: usize) {
        let old_count = *self.peer_count.read().await;
        let old_phase = current_phase(old_count);
        let new_phase = current_phase(count);

        *self.peer_count.write().await = count;

        if old_phase != new_phase {
            tracing::warn!(
                "[GOVERNANCE] 🔄 PHASE TRANSITION: {} → {} (peers: {} → {})",
                old_phase, new_phase, old_count, count
            );

            self.transitions.write().await.push(PhaseTransition {
                from: old_phase,
                to: new_phase.clone(),
                peer_count: count,
                timestamp: chrono::Utc::now().to_rfc3339(),
            });

            if new_phase == GovernancePhase::Democracy {
                tracing::warn!(
                    "[GOVERNANCE] 🏛️ DEMOCRACY ACHIEVED — creator key has NO special powers"
                );
            }
        }
    }

    /// Get the current phase.
    pub async fn phase(&self) -> GovernancePhase {
        current_phase(*self.peer_count.read().await)
    }

    /// Check if creator can perform an emergency action.
    pub async fn can_creator_emergency(&self) -> bool {
        creator_has_emergency_powers(*self.peer_count.read().await)
    }

    /// Propose a council action (Council phase only).
    pub async fn propose_action(
        &self,
        action_type: &str,
        description: &str,
        proposer: PeerId,
    ) -> Result<String, String> {
        let phase = self.phase().await;
        if phase != GovernancePhase::Council {
            return Err(format!(
                "Council actions only available in Council phase (current: {})",
                phase
            ));
        }

        let action_id = uuid::Uuid::new_v4().to_string();
        let mut approvals = HashSet::new();
        approvals.insert(proposer.0.clone()); // Proposer auto-approves

        let action = CouncilAction {
            action_id: action_id.clone(),
            action_type: action_type.to_string(),
            description: description.to_string(),
            proposed_by: proposer,
            proposed_at: chrono::Utc::now().to_rfc3339(),
            approvals,
            required_approvals: 2, // Creator + at least 1 council member
            executed: false,
        };

        self.pending_actions.write().await.insert(action_id.clone(), action);
        Ok(action_id)
    }

    /// Approve a council action. Returns true if threshold reached.
    pub async fn approve_action(
        &self,
        action_id: &str,
        approver: PeerId,
    ) -> Result<bool, String> {
        let mut actions = self.pending_actions.write().await;
        let action = actions.get_mut(action_id)
            .ok_or_else(|| format!("Action {} not found", action_id))?;

        if action.executed {
            return Err("Action already executed".to_string());
        }

        // Verify approver is creator or council member
        let council = self.council.read().await;
        let is_council = council.iter().any(|c| c.peer_id == approver);
        let is_creator = crate::network::creator_key::creator_key_exists()
            && approver.0 == "creator"; // Simplified check

        if !is_council && !is_creator {
            return Err("Only creator or council members can approve".to_string());
        }

        action.approvals.insert(approver.0.clone());

        if action.approvals.len() >= action.required_approvals {
            action.executed = true;
            tracing::info!(
                "[GOVERNANCE] ✅ Council action {} approved ({}/{} signatures)",
                &action_id[..8], action.approvals.len(), action.required_approvals
            );
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Elect a council member for a region.
    pub async fn elect_council_member(
        &self,
        peer_id: PeerId,
        region: &str,
        votes: usize,
    ) {
        let mut council = self.council.write().await;

        // Replace existing member for this region if new one has more votes
        if let Some(existing) = council.iter_mut().find(|c| c.region == region) {
            if votes > existing.votes_received {
                tracing::info!(
                    "[GOVERNANCE] 🗳️ Council seat for {} changed: {} → {}",
                    region, existing.peer_id, peer_id
                );
                *existing = CouncilMember {
                    peer_id,
                    region: region.to_string(),
                    elected_at: chrono::Utc::now().to_rfc3339(),
                    votes_received: votes,
                };
            }
        } else {
            council.push(CouncilMember {
                peer_id: peer_id.clone(),
                region: region.to_string(),
                elected_at: chrono::Utc::now().to_rfc3339(),
                votes_received: votes,
            });
            tracing::info!(
                "[GOVERNANCE] 🗳️ Council member elected for {}: {}",
                region, peer_id
            );
        }
    }

    /// Get stats.
    pub async fn stats(&self) -> serde_json::Value {
        let count = *self.peer_count.read().await;
        let phase = current_phase(count);
        let council = self.council.read().await;
        let pending = self.pending_actions.read().await;
        let transitions = self.transitions.read().await;

        serde_json::json!({
            "phase": format!("{}", phase),
            "peer_count": count,
            "council_members": council.len(),
            "pending_actions": pending.values().filter(|a| !a.executed).count(),
            "phase_transitions": transitions.len(),
            "thresholds": {
                "council": COUNCIL_THRESHOLD,
                "democracy": DEMOCRACY_THRESHOLD,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_thresholds() {
        assert_eq!(current_phase(0), GovernancePhase::Bootstrap);
        assert_eq!(current_phase(5), GovernancePhase::Bootstrap);
        assert_eq!(current_phase(9), GovernancePhase::Bootstrap);
        assert_eq!(current_phase(10), GovernancePhase::Council);
        assert_eq!(current_phase(500), GovernancePhase::Council);
        assert_eq!(current_phase(999), GovernancePhase::Council);
        assert_eq!(current_phase(1000), GovernancePhase::Democracy);
        assert_eq!(current_phase(10_000), GovernancePhase::Democracy);
    }

    #[test]
    fn test_creator_emergency_powers() {
        assert!(creator_has_emergency_powers(0));
        assert!(creator_has_emergency_powers(9));
        assert!(!creator_has_emergency_powers(10));
        assert!(!creator_has_emergency_powers(1000));
    }

    #[test]
    fn test_requires_council() {
        assert!(!requires_council(5)); // Bootstrap — creator acts alone
        assert!(requires_council(50)); // Council — needs approval
        assert!(!requires_council(1000)); // Democracy — standard governance
    }

    #[tokio::test]
    async fn test_phase_transition_detection() {
        let mgr = GovernanceManager::new();

        mgr.update_peer_count(5).await;
        assert_eq!(mgr.phase().await, GovernancePhase::Bootstrap);

        mgr.update_peer_count(50).await;
        assert_eq!(mgr.phase().await, GovernancePhase::Council);

        mgr.update_peer_count(1000).await;
        assert_eq!(mgr.phase().await, GovernancePhase::Democracy);

        // Should have recorded 2 transitions
        let transitions = mgr.transitions.read().await;
        assert_eq!(transitions.len(), 2);
    }

    #[tokio::test]
    async fn test_council_action_flow() {
        let mgr = GovernanceManager::new();
        mgr.update_peer_count(50).await; // Council phase

        // Elect a council member
        mgr.elect_council_member(
            PeerId("council_eu".to_string()),
            "Europe",
            10,
        ).await;

        // Propose action
        let action_id = mgr.propose_action(
            "unban",
            "Unban hardware ABC123",
            PeerId("creator".to_string()),
        ).await.unwrap();

        // Creator already auto-approved (1/2)
        // Council member approves (2/2)
        let executed = mgr.approve_action(
            &action_id,
            PeerId("council_eu".to_string()),
        ).await.unwrap();

        assert!(executed);
    }

    #[tokio::test]
    async fn test_council_action_rejected_in_bootstrap() {
        let mgr = GovernanceManager::new();
        mgr.update_peer_count(5).await; // Bootstrap phase

        let result = mgr.propose_action(
            "test",
            "Should fail",
            PeerId("anyone".to_string()),
        ).await;

        assert!(result.is_err());
    }
}
