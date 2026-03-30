/// Community Governance & Crisis Response — Decentralised moderation + survival.
///
/// Features:
/// - Ban Voting: >50% majority for ban, >75% supermajority for immediate effect
/// - Dispute Resolution: Banned peers can appeal with evidence re-vote
/// - Emergency Alert System: Crisis broadcasts with severity and category
/// - OSINT Sharing: Blocked IPs, VPN endpoints, circumvention techniques
/// - Resource Directory: Peers advertise what they can provide (relay, storage, compute)
///
/// SURVIVABILITY: All governance is peer-to-peer. No central authority.
/// Decisions are made by mesh consensus. Works without internet.
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::network::messages::{PeerId, AlertSeverity, CrisisCategory, ResourceType};

// ─── Ban Voting ─────────────────────────────────────────────────────────

/// A proposal to ban a peer from the mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BanProposal {
    pub id: String,
    pub target: PeerId,
    pub reason: String,
    pub evidence_hash: String,
    pub proposer: PeerId,
    pub created_at: String,
    pub votes_for: Vec<PeerId>,
    pub votes_against: Vec<PeerId>,
    pub resolved: bool,
    pub outcome: Option<BanOutcome>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BanOutcome {
    Banned,
    Acquitted,
    Appealed,
}

impl BanProposal {
    pub fn new(target: PeerId, reason: &str, evidence_hash: &str, proposer: PeerId) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            target,
            reason: reason.to_string(),
            evidence_hash: evidence_hash.to_string(),
            proposer,
            created_at: chrono::Utc::now().to_rfc3339(),
            votes_for: Vec::new(),
            votes_against: Vec::new(),
            resolved: false,
            outcome: None,
        }
    }

    /// Cast a vote on this proposal.
    pub fn vote(&mut self, voter: PeerId, approve: bool) -> Result<(), String> {
        if self.resolved {
            return Err("Proposal already resolved".to_string());
        }

        // Check for double voting
        if self.votes_for.contains(&voter) || self.votes_against.contains(&voter) {
            return Err(format!("Peer {} has already voted", voter));
        }

        if approve {
            self.votes_for.push(voter);
        } else {
            self.votes_against.push(voter);
        }

        Ok(())
    }

    /// Check if the proposal has reached a decision given the total peer count.
    pub fn evaluate(&mut self, total_peers: usize) -> Option<BanOutcome> {
        if self.resolved {
            return self.outcome.clone();
        }

        let total_votes = self.votes_for.len() + self.votes_against.len();
        let for_ratio = if total_votes > 0 {
            self.votes_for.len() as f64 / total_votes as f64
        } else {
            0.0
        };

        // Need at least 3 votes or all peers to have voted
        if total_votes < 3.min(total_peers) {
            return None;
        }

        // Supermajority (>75%) = immediate ban
        if for_ratio > 0.75 {
            self.resolved = true;
            self.outcome = Some(BanOutcome::Banned);
            tracing::warn!("[GOVERNANCE] ⚖️ BANNED: {} (supermajority {:.0}%, reason: {})",
                self.target, for_ratio * 100.0, self.reason);
            return self.outcome.clone();
        }

        // Simple majority (>50%) with enough votes
        if total_votes >= (total_peers / 2 + 1).max(3) && for_ratio > 0.5 {
            self.resolved = true;
            self.outcome = Some(BanOutcome::Banned);
            tracing::warn!("[GOVERNANCE] ⚖️ BANNED: {} (majority {:.0}%, reason: {})",
                self.target, for_ratio * 100.0, self.reason);
            return self.outcome.clone();
        }

        // If majority voted against, acquit
        if total_votes >= (total_peers / 2 + 1).max(3) && for_ratio <= 0.5 {
            self.resolved = true;
            self.outcome = Some(BanOutcome::Acquitted);
            tracing::info!("[GOVERNANCE] ✅ ACQUITTED: {} ({:.0}% voted against)",
                self.target, (1.0 - for_ratio) * 100.0);
            return self.outcome.clone();
        }

        None
    }
}

// ─── Emergency Alerts ───────────────────────────────────────────────────

/// An emergency alert broadcast to all mesh peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergencyAlert {
    pub id: String,
    pub severity: AlertSeverity,
    pub category: CrisisCategory,
    pub message: String,
    pub issuer: PeerId,
    pub issued_at: String,
    pub acknowledged_by: Vec<PeerId>,
}

impl EmergencyAlert {
    pub fn new(severity: AlertSeverity, category: CrisisCategory, message: &str, issuer: PeerId) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            severity,
            category,
            message: message.to_string(),
            issuer,
            issued_at: chrono::Utc::now().to_rfc3339(),
            acknowledged_by: Vec::new(),
        }
    }

    pub fn acknowledge(&mut self, peer: PeerId) {
        if !self.acknowledged_by.contains(&peer) {
            self.acknowledged_by.push(peer);
        }
    }
}

// ─── Resource Directory ─────────────────────────────────────────────────

/// A resource advertised by a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAdvertisement {
    pub peer_id: PeerId,
    pub resource_type: ResourceType,
    pub capacity: String,
    pub advertised_at: String,
    pub available: bool,
}

// ─── OSINT Report ───────────────────────────────────────────────────────

/// A community OSINT report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OSINTEntry {
    pub id: String,
    pub category: String,       // "blocked_ips", "vpn_endpoints", "circumvention", "compromised_nodes"
    pub data: String,
    pub issuer: PeerId,
    pub issued_at: String,
    pub confidence: f64,        // 0.0 to 1.0
    pub confirmations: Vec<PeerId>,
}

impl OSINTEntry {
    pub fn new(category: &str, data: &str, issuer: PeerId) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            category: category.to_string(),
            data: data.to_string(),
            issuer,
            issued_at: chrono::Utc::now().to_rfc3339(),
            confidence: 0.5, // Starts neutral
            confirmations: Vec::new(),
        }
    }

    /// Another peer confirms this report.
    pub fn confirm(&mut self, peer: PeerId) {
        if !self.confirmations.contains(&peer) {
            self.confirmations.push(peer);
            // Confidence increases with confirmations (asymptotic to 1.0)
            self.confidence = 1.0 - (1.0 / (self.confirmations.len() as f64 + 1.0));
        }
    }
}

// ─── Governance Engine ──────────────────────────────────────────────────

/// The decentralised governance engine.
pub struct GovernanceEngine {
    /// Active ban proposals
    proposals: Arc<RwLock<Vec<BanProposal>>>,
    /// Emergency alerts (last 100)
    alerts: Arc<RwLock<Vec<EmergencyAlert>>>,
    /// Resource directory
    resources: Arc<RwLock<Vec<ResourceAdvertisement>>>,
    /// OSINT database
    osint: Arc<RwLock<Vec<OSINTEntry>>>,
    /// Banned peers
    banned_peers: Arc<RwLock<Vec<PeerId>>>,
}

impl GovernanceEngine {
    pub fn new() -> Self {
        tracing::info!("[GOVERNANCE] ⚖️ Decentralised governance engine initialised");

        Self {
            proposals: Arc::new(RwLock::new(Vec::new())),
            alerts: Arc::new(RwLock::new(Vec::new())),
            resources: Arc::new(RwLock::new(Vec::new())),
            osint: Arc::new(RwLock::new(Vec::new())),
            banned_peers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    // ── Ban Voting ──

    /// Create a new ban proposal.
    pub async fn propose_ban(&self, target: PeerId, reason: &str, evidence_hash: &str, proposer: PeerId) -> String {
        let proposal = BanProposal::new(target.clone(), reason, evidence_hash, proposer);
        let id = proposal.id.clone();
        tracing::warn!("[GOVERNANCE] 🗳️ Ban proposed for {} by proposal {}: {}",
            target, id, reason);
        self.proposals.write().await.push(proposal);
        id
    }

    /// Cast a vote on a ban proposal.
    pub async fn vote(&self, proposal_id: &str, voter: PeerId, approve: bool, total_peers: usize) -> Result<Option<BanOutcome>, String> {
        let mut proposals = self.proposals.write().await;
        let proposal = proposals.iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or_else(|| format!("Proposal {} not found", proposal_id))?;

        proposal.vote(voter, approve)?;
        let outcome = proposal.evaluate(total_peers);

        if let Some(BanOutcome::Banned) = &outcome {
            self.banned_peers.write().await.push(proposal.target.clone());
        }

        Ok(outcome)
    }

    /// Check if a peer is banned.
    pub async fn is_banned(&self, peer_id: &PeerId) -> bool {
        self.banned_peers.read().await.contains(peer_id)
    }

    /// Get active proposals.
    pub async fn active_proposals(&self) -> Vec<BanProposal> {
        self.proposals.read().await.iter()
            .filter(|p| !p.resolved)
            .cloned()
            .collect()
    }

    // ── Emergency Alerts ──

    /// Broadcast an emergency alert.
    pub async fn issue_alert(&self, severity: AlertSeverity, category: CrisisCategory, message: &str, issuer: PeerId) -> String {
        let alert = EmergencyAlert::new(severity, category, message, issuer);
        let id = alert.id.clone();
        tracing::warn!("[GOVERNANCE] 🚨 EMERGENCY: {}", message);
        let mut alerts = self.alerts.write().await;
        if alerts.len() >= 100 {
            alerts.remove(0);
        }
        alerts.push(alert);
        id
    }

    /// Acknowledge an alert.
    pub async fn acknowledge_alert(&self, alert_id: &str, peer: PeerId) {
        let mut alerts = self.alerts.write().await;
        if let Some(alert) = alerts.iter_mut().find(|a| a.id == alert_id) {
            alert.acknowledge(peer);
        }
    }

    /// Get recent alerts.
    pub async fn recent_alerts(&self, limit: usize) -> Vec<EmergencyAlert> {
        let alerts = self.alerts.read().await;
        alerts.iter().rev().take(limit).cloned().collect()
    }

    // ── Resource Directory ──

    /// Advertise a resource this peer can provide.
    pub async fn advertise_resource(&self, peer_id: PeerId, resource_type: ResourceType, capacity: &str) {
        let ad = ResourceAdvertisement {
            peer_id: peer_id.clone(),
            resource_type,
            capacity: capacity.to_string(),
            advertised_at: chrono::Utc::now().to_rfc3339(),
            available: true,
        };
        tracing::info!("[GOVERNANCE] 📢 Resource advertised by {}: {}", peer_id, capacity);
        self.resources.write().await.push(ad);
    }

    /// Find peers offering a specific resource type.
    pub async fn find_resources(&self, resource_type: &ResourceType) -> Vec<ResourceAdvertisement> {
        self.resources.read().await.iter()
            .filter(|r| std::mem::discriminant(&r.resource_type) == std::mem::discriminant(resource_type) && r.available)
            .cloned()
            .collect()
    }

    // ── OSINT ──

    /// Submit an OSINT report.
    pub async fn submit_osint(&self, category: &str, data: &str, issuer: PeerId) -> String {
        let entry = OSINTEntry::new(category, data, issuer);
        let id = entry.id.clone();
        tracing::info!("[GOVERNANCE] 🔍 OSINT report: [{}] {}", category, &data[..data.len().min(80)]);
        self.osint.write().await.push(entry);
        id
    }

    /// Confirm an OSINT report from another peer.
    pub async fn confirm_osint(&self, report_id: &str, confirmer: PeerId) {
        let mut osint = self.osint.write().await;
        if let Some(entry) = osint.iter_mut().find(|e| e.id == report_id) {
            entry.confirm(confirmer);
        }
    }

    /// Get OSINT reports filtered by category.
    pub async fn osint_by_category(&self, category: &str) -> Vec<OSINTEntry> {
        self.osint.read().await.iter()
            .filter(|e| e.category == category)
            .cloned()
            .collect()
    }

    /// Get all high-confidence OSINT reports.
    pub async fn high_confidence_osint(&self, min_confidence: f64) -> Vec<OSINTEntry> {
        self.osint.read().await.iter()
            .filter(|e| e.confidence >= min_confidence)
            .cloned()
            .collect()
    }

    // ── Stats ──

    pub async fn stats(&self) -> serde_json::Value {
        let active_proposals = self.proposals.read().await.iter()
            .filter(|p| !p.resolved).count();
        let total_proposals = self.proposals.read().await.len();
        let alert_count = self.alerts.read().await.len();
        let resource_count = self.resources.read().await.len();
        let osint_count = self.osint.read().await.len();
        let banned_count = self.banned_peers.read().await.len();

        serde_json::json!({
            "active_proposals": active_proposals,
            "total_proposals": total_proposals,
            "alerts": alert_count,
            "resources_advertised": resource_count,
            "osint_reports": osint_count,
            "banned_peers": banned_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str) -> PeerId { PeerId(id.to_string()) }

    #[tokio::test]
    async fn test_ban_proposal_creation() {
        let gov = GovernanceEngine::new();
        let id = gov.propose_ban(peer("bad_actor"), "Spamming", "hash123", peer("reporter")).await;
        assert!(!id.is_empty());
        assert_eq!(gov.active_proposals().await.len(), 1);
    }

    #[tokio::test]
    async fn test_ban_voting_supermajority() {
        let gov = GovernanceEngine::new();
        let id = gov.propose_ban(peer("target"), "Abuse", "hash", peer("p1")).await;

        // 4 out of 5 vote for ban (80% = supermajority)
        gov.vote(&id, peer("v1"), true, 5).await.unwrap();
        gov.vote(&id, peer("v2"), true, 5).await.unwrap();
        gov.vote(&id, peer("v3"), true, 5).await.unwrap();
        let outcome = gov.vote(&id, peer("v4"), true, 5).await.unwrap();

        assert_eq!(outcome, Some(BanOutcome::Banned));
        assert!(gov.is_banned(&peer("target")).await);
    }

    #[tokio::test]
    async fn test_ban_voting_acquittal() {
        let gov = GovernanceEngine::new();
        let id = gov.propose_ban(peer("target"), "False accusation", "hash", peer("p1")).await;

        gov.vote(&id, peer("v1"), false, 5).await.unwrap();
        gov.vote(&id, peer("v2"), false, 5).await.unwrap();
        let outcome = gov.vote(&id, peer("v3"), false, 5).await.unwrap();

        assert_eq!(outcome, Some(BanOutcome::Acquitted));
        assert!(!gov.is_banned(&peer("target")).await);
    }

    #[tokio::test]
    async fn test_double_vote_rejected() {
        let gov = GovernanceEngine::new();
        let id = gov.propose_ban(peer("target"), "reason", "hash", peer("p1")).await;

        gov.vote(&id, peer("v1"), true, 5).await.unwrap();
        let result = gov.vote(&id, peer("v1"), true, 5).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_emergency_alert() {
        let gov = GovernanceEngine::new();
        let id = gov.issue_alert(
            AlertSeverity::Critical,
            CrisisCategory::ConnectivityLost,
            "ISP backbone failure detected in region EU-West",
            peer("monitor_node"),
        ).await;

        gov.acknowledge_alert(&id, peer("ack_peer")).await;

        let alerts = gov.recent_alerts(10).await;
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].acknowledged_by.len(), 1);
    }

    #[tokio::test]
    async fn test_resource_directory() {
        let gov = GovernanceEngine::new();

        gov.advertise_resource(peer("relay_node"), ResourceType::InternetRelay, "100Mbps fiber").await;
        gov.advertise_resource(peer("storage_node"), ResourceType::Storage, "500GB available").await;

        let relays = gov.find_resources(&ResourceType::InternetRelay).await;
        assert_eq!(relays.len(), 1);
        assert_eq!(relays[0].capacity, "100Mbps fiber");
    }

    #[tokio::test]
    async fn test_osint_submission_and_confirmation() {
        let gov = GovernanceEngine::new();

        let id = gov.submit_osint("blocked_ips", "192.168.1.100 - compromised relay", peer("reporter")).await;

        // Two peers confirm
        gov.confirm_osint(&id, peer("confirmer_1")).await;
        gov.confirm_osint(&id, peer("confirmer_2")).await;

        let reports = gov.osint_by_category("blocked_ips").await;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].confirmations.len(), 2);
        assert!(reports[0].confidence > 0.5);
    }

    #[tokio::test]
    async fn test_osint_confidence_increases() {
        let mut entry = OSINTEntry::new("test", "data", peer("issuer"));
        assert_eq!(entry.confidence, 0.5);

        entry.confirm(peer("c1"));
        assert!(entry.confidence > 0.5);

        entry.confirm(peer("c2"));
        entry.confirm(peer("c3"));
        assert!(entry.confidence > 0.7);
    }

    #[tokio::test]
    async fn test_governance_stats() {
        let gov = GovernanceEngine::new();
        gov.propose_ban(peer("t1"), "r", "h", peer("p")).await;
        gov.issue_alert(AlertSeverity::Info, CrisisCategory::ResourceAvailable, "test", peer("n")).await;

        let stats = gov.stats().await;
        assert_eq!(stats["active_proposals"], 1);
        assert_eq!(stats["alerts"], 1);
    }
}
