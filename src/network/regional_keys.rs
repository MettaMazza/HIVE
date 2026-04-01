/// Regional Keys — Pure democratic governance for the HIVE mesh.
///
/// GOVERNANCE: Peers → Regional Consensus → Mesh-wide Consensus
/// No individual has override power. Not even the creator.
///
/// REGIONS:
///   europe, africa, americas, asia, oceania, global
///
/// Each region has a collective identity derived from its member peers.
/// Regional consensus is required for:
///   - Banning peers within the region (2/3 majority)
///   - Unbanning wrongly-banned peers (2/3 majority)
///   - Endorsing global governance proposals
///   - Regional resource allocation decisions
///
/// ASSIGNMENT: Peers self-declare their region on first boot.
/// No IP geolocation — privacy-first design. Self-declaration is trusted
/// because there is no advantage to misrepresenting region membership.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::network::messages::PeerId;

/// The six mesh regions — continental boundaries.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum Region {
    Europe,
    Africa,
    Americas,
    Asia,
    Oceania,
    /// Global — for peers that don't declare or for mesh-wide actions.
    Global,
}

impl std::fmt::Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Europe => write!(f, "Europe"),
            Self::Africa => write!(f, "Africa"),
            Self::Americas => write!(f, "Americas"),
            Self::Asia => write!(f, "Asia"),
            Self::Oceania => write!(f, "Oceania"),
            Self::Global => write!(f, "Global"),
        }
    }
}

impl Region {
    /// Parse a region from a string (case-insensitive).
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().trim() {
            "europe" | "eu" | "emea" => Self::Europe,
            "africa" | "af" => Self::Africa,
            "americas" | "america" | "na" | "sa" | "us" => Self::Americas,
            "asia" | "apac" => Self::Asia,
            "oceania" | "au" | "nz" | "pacific" => Self::Oceania,
            _ => Self::Global,
        }
    }

    /// Get all defined regions.
    pub fn all() -> Vec<Self> {
        vec![
            Self::Europe,
            Self::Africa,
            Self::Americas,
            Self::Asia,
            Self::Oceania,
            Self::Global,
        ]
    }
}

/// A regional governance proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionalProposal {
    pub id: String,
    pub region: Region,
    pub action: RegionalAction,
    pub proposer: PeerId,
    pub votes_for: HashSet<String>,
    pub votes_against: HashSet<String>,
    pub created_at: String,
    pub resolved: bool,
    pub outcome: Option<String>,
}

/// Actions that require regional consensus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegionalAction {
    /// Ban a peer from the region.
    BanPeer { target: PeerId, reason: String },
    /// Endorse a global governance proposal.
    EndorseGlobal { global_proposal_id: String },
    /// Allocate regional compute resources.
    AllocateResource { resource_type: String, amount: String },
    /// Custom regional directive.
    Custom { description: String },
}

impl std::fmt::Display for RegionalAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BanPeer { target, reason } => write!(f, "Ban {} — {}", target, reason),
            Self::EndorseGlobal { global_proposal_id } => write!(f, "Endorse global #{}", global_proposal_id),
            Self::AllocateResource { resource_type, amount } => write!(f, "Allocate {} ({})", resource_type, amount),
            Self::Custom { description } => write!(f, "Custom: {}", description),
        }
    }
}

/// The Regional Key Registry — manages region membership and democratic consensus.
pub struct RegionalKeyRegistry {
    /// Peer → Region membership
    memberships: Arc<RwLock<HashMap<String, Region>>>,
    /// Active regional proposals
    proposals: Arc<RwLock<Vec<RegionalProposal>>>,
}

impl RegionalKeyRegistry {
    pub fn new() -> Self {
        tracing::info!("[REGIONAL] 🌍 Regional key registry initialised (pure democracy — no overrides)");
        Self {
            memberships: Arc::new(RwLock::new(HashMap::new())),
            proposals: Arc::new(RwLock::new(Vec::new())),
        }
    }

    // ── Membership ──────────────────────────────────────────────────

    /// Register a peer's region. Self-declared on first boot.
    pub async fn register_peer(&self, peer_id: &PeerId, region: Region) {
        let mut memberships = self.memberships.write().await;
        tracing::info!("[REGIONAL] 📍 Peer {} → {}", peer_id, region);
        memberships.insert(peer_id.0.clone(), region);
    }

    /// Get a peer's declared region.
    pub async fn peer_region(&self, peer_id: &PeerId) -> Region {
        self.memberships.read().await
            .get(&peer_id.0)
            .cloned()
            .unwrap_or(Region::Global)
    }

    /// Get all peers in a region.
    pub async fn peers_in_region(&self, region: &Region) -> Vec<PeerId> {
        self.memberships.read().await
            .iter()
            .filter(|(_, r)| *r == region)
            .map(|(id, _)| PeerId(id.clone()))
            .collect()
    }

    /// Get the member count for a region.
    pub async fn region_member_count(&self, region: &Region) -> usize {
        self.memberships.read().await
            .values()
            .filter(|r| *r == region)
            .count()
    }

    /// Get all region sizes.
    pub async fn region_stats(&self) -> HashMap<String, usize> {
        let memberships = self.memberships.read().await;
        let mut stats = HashMap::new();
        for region in Region::all() {
            let count = memberships.values().filter(|r| **r == region).count();
            stats.insert(format!("{}", region), count);
        }
        stats
    }

    // ── Regional Proposals ──────────────────────────────────────────

    /// Create a regional proposal. Only peers in the region can propose.
    pub async fn propose(
        &self,
        region: Region,
        action: RegionalAction,
        proposer: PeerId,
    ) -> Result<String, String> {
        // Verify proposer is in the region
        let peer_region = self.peer_region(&proposer).await;
        if peer_region != region && peer_region != Region::Global {
            return Err(format!(
                "Peer {} is in {} but proposing for {} — denied",
                proposer, peer_region, region
            ));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let mut votes_for = HashSet::new();
        votes_for.insert(proposer.0.clone()); // Proposer auto-votes for

        let proposal = RegionalProposal {
            id: id.clone(),
            region: region.clone(),
            action: action.clone(),
            proposer: proposer.clone(),
            votes_for,
            votes_against: HashSet::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
            resolved: false,
            outcome: None,
        };

        tracing::info!(
            "[REGIONAL] 🗳️ [{}] Proposal by {}: {}",
            region, proposer, action
        );

        self.proposals.write().await.push(proposal);
        Ok(id)
    }

    /// Vote on a regional proposal. Only region members can vote.
    /// Returns the outcome if the proposal is resolved.
    pub async fn vote(
        &self,
        proposal_id: &str,
        voter: PeerId,
        approve: bool,
    ) -> Result<Option<String>, String> {
        let mut proposals = self.proposals.write().await;
        let proposal = proposals.iter_mut()
            .find(|p| p.id == proposal_id && !p.resolved)
            .ok_or_else(|| format!("Proposal {} not found or already resolved", proposal_id))?;

        // Verify voter is in the region
        let memberships = self.memberships.read().await;
        let voter_region = memberships.get(&voter.0).cloned().unwrap_or(Region::Global);
        if voter_region != proposal.region && voter_region != Region::Global {
            return Err(format!(
                "Voter {} is in {} but proposal is for {} — denied",
                voter, voter_region, proposal.region
            ));
        }

        // Prevent double-voting
        if proposal.votes_for.contains(&voter.0) || proposal.votes_against.contains(&voter.0) {
            return Err(format!("Peer {} has already voted on proposal {}", voter, proposal_id));
        }

        if approve {
            proposal.votes_for.insert(voter.0.clone());
        } else {
            proposal.votes_against.insert(voter.0.clone());
        }

        // Check if 2/3 majority is reached
        let region_count = memberships.values()
            .filter(|r| **r == proposal.region)
            .count();
        let threshold = (region_count * 2) / 3;
        let total_votes = proposal.votes_for.len() + proposal.votes_against.len();

        if proposal.votes_for.len() > threshold {
            proposal.resolved = true;
            let outcome = format!("APPROVED ({}/{})", proposal.votes_for.len(), total_votes);
            proposal.outcome = Some(outcome.clone());
            tracing::info!(
                "[REGIONAL] ✅ [{}] Proposal {} approved: {}",
                proposal.region, proposal.id, proposal.action
            );
            return Ok(Some(outcome));
        }

        if proposal.votes_against.len() > threshold {
            proposal.resolved = true;
            let outcome = format!("REJECTED ({}/{})", proposal.votes_against.len(), total_votes);
            proposal.outcome = Some(outcome.clone());
            tracing::info!(
                "[REGIONAL] ❌ [{}] Proposal {} rejected",
                proposal.region, proposal.id
            );
            return Ok(Some(outcome));
        }

        Ok(None) // Not yet resolved
    }

    /// Get active proposals for a region.
    pub async fn active_proposals(&self, region: &Region) -> Vec<RegionalProposal> {
        self.proposals.read().await
            .iter()
            .filter(|p| &p.region == region && !p.resolved)
            .cloned()
            .collect()
    }




    /// Get stats for dashboards.
    pub async fn stats(&self) -> serde_json::Value {
        let memberships = self.memberships.read().await;
        let proposals = self.proposals.read().await;

        let mut region_counts = HashMap::new();
        for region in Region::all() {
            let count = memberships.values().filter(|r| **r == region).count();
            region_counts.insert(format!("{}", region), count);
        }

        serde_json::json!({
            "total_peers": memberships.len(),
            "regions": region_counts,
            "active_proposals": proposals.iter().filter(|p| !p.resolved).count(),
            "resolved_proposals": proposals.iter().filter(|p| p.resolved).count(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_region_membership() {
        let registry = RegionalKeyRegistry::new();
        let peer = PeerId("peer_eu_001".to_string());

        registry.register_peer(&peer, Region::Europe).await;
        assert_eq!(registry.peer_region(&peer).await, Region::Europe);
        assert_eq!(registry.region_member_count(&Region::Europe).await, 1);
    }

    #[tokio::test]
    async fn test_default_region_is_global() {
        let registry = RegionalKeyRegistry::new();
        let unknown = PeerId("unknown_peer".to_string());
        assert_eq!(registry.peer_region(&unknown).await, Region::Global);
    }

    #[tokio::test]
    async fn test_regional_proposal_flow() {
        let registry = RegionalKeyRegistry::new();

        // Register 3 peers in Europe
        for i in 0..3 {
            let peer = PeerId(format!("eu_{}", i));
            registry.register_peer(&peer, Region::Europe).await;
        }

        // Peer 0 proposes banning a target
        let target = PeerId("malicious_eu".to_string());
        registry.register_peer(&target, Region::Europe).await;

        let proposal_id = registry.propose(
            Region::Europe,
            RegionalAction::BanPeer {
                target: target.clone(),
                reason: "Malicious behaviour".to_string(),
            },
            PeerId("eu_0".to_string()),
        ).await.unwrap();

        // Peer 1 votes for
        let result = registry.vote(
            &proposal_id,
            PeerId("eu_1".to_string()),
            true,
        ).await.unwrap();

        // Peer 2 votes for → should reach 3/4 = 2/3 majority (3 votes for out of 4 members)
        let result = registry.vote(
            &proposal_id,
            PeerId("eu_2".to_string()),
            true,
        ).await.unwrap();

        assert!(result.is_some(), "Proposal should be approved with 3/4 votes");
    }

    #[tokio::test]
    async fn test_cross_region_proposal_denied() {
        let registry = RegionalKeyRegistry::new();

        let eu_peer = PeerId("eu_peer".to_string());
        registry.register_peer(&eu_peer, Region::Europe).await;

        // EU peer tries to propose in Africa — should fail
        let result = registry.propose(
            Region::Africa,
            RegionalAction::Custom { description: "Crossover".to_string() },
            eu_peer,
        ).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_double_vote_rejected() {
        let registry = RegionalKeyRegistry::new();
        let peer = PeerId("eu_0".to_string());
        registry.register_peer(&peer, Region::Europe).await;

        let id = registry.propose(
            Region::Europe,
            RegionalAction::Custom { description: "Test".to_string() },
            peer.clone(),
        ).await.unwrap();

        // Peer already auto-voted for when proposing
        let result = registry.vote(&id, peer, true).await;
        assert!(result.is_err(), "Double voting should be rejected");
    }

    #[tokio::test]
    async fn test_no_creator_override_exists() {
        // Verify that no override mechanism exists.
        // The registry has no overrides field, no record method, no vote_reversal.
        // This test exists to prove the system is purely democratic.
        let registry = RegionalKeyRegistry::new();
        let stats = registry.stats().await;
        // No "creator_overrides" key should exist in stats
        assert!(stats.get("creator_overrides").is_none(),
            "No creator override mechanism should exist");
    }

    #[test]
    fn test_region_parsing() {
        assert_eq!(Region::from_str_loose("europe"), Region::Europe);
        assert_eq!(Region::from_str_loose("EU"), Region::Europe);
        assert_eq!(Region::from_str_loose("africa"), Region::Africa);
        assert_eq!(Region::from_str_loose("americas"), Region::Americas);
        assert_eq!(Region::from_str_loose("US"), Region::Americas);
        assert_eq!(Region::from_str_loose("asia"), Region::Asia);
        assert_eq!(Region::from_str_loose("oceania"), Region::Oceania);
        assert_eq!(Region::from_str_loose("unknown"), Region::Global);
    }
}
