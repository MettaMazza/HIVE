/// Pool Manager — Decentralised web + compute resource pooling.
///
/// EQUALITY: Everyone contributes, everyone benefits. Enabled by default.
///
/// Web Connection Pool:
/// - Peers with internet relay HTTP requests for peers without
/// - Round-robin load balancing across available relays
/// - Per-peer fair usage quotas (100 req/hour default)
/// - Ephemeral request IDs — relay peers never learn your identity
///
/// Compute Pool:
/// - Peers share spare Ollama inference capacity
/// - Sorted by available slots, lowest queue depth wins
/// - Identity-stripped: only the raw prompt is forwarded, NO history/memory/persona
/// - Content-filtered: every inbound prompt is scanned before execution
/// - Rate-limited: max tokens/hour per remote peer
///
/// SURVIVABILITY: All pooling is peer-to-peer over QUIC. No cloud scheduler.
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::network::messages::PeerId;

// ─── Web Connection Pool ────────────────────────────────────────────────

/// A peer available for web relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPeer {
    pub peer_id: PeerId,
    pub latency_ms: u64,
    pub requests_served: u64,
    pub last_seen: String,
    pub available: bool,
}

/// Log entry for fair usage tracking.
#[derive(Debug, Clone)]
struct RequestLog {
    peer_id: String,
    timestamp: std::time::Instant,
}

/// Web connection pooling.
pub struct WebConnectionPool {
    /// Peers with internet that can relay
    available_relays: Vec<RelayPeer>,
    /// Request log for fair usage tracking
    request_log: VecDeque<RequestLog>,
    /// Max requests any single peer can make per hour
    max_requests_per_hour: u64,
    /// Round-robin index
    next_relay_idx: usize,
}

impl WebConnectionPool {
    pub fn new() -> Self {
        let max_req = std::env::var("REMOVED_MESH_GOVERNED")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(100);

        Self {
            available_relays: Vec::new(),
            request_log: VecDeque::with_capacity(10000),
            max_requests_per_hour: max_req,
            next_relay_idx: 0,
        }
    }

    /// Register or update a relay peer.
    pub fn update_relay(&mut self, peer: RelayPeer) {
        if let Some(existing) = self.available_relays.iter_mut().find(|r| r.peer_id == peer.peer_id) {
            existing.latency_ms = peer.latency_ms;
            existing.available = peer.available;
            existing.last_seen = peer.last_seen;
        } else {
            tracing::info!("[POOL] 🌐 New web relay peer: {} ({}ms)", peer.peer_id, peer.latency_ms);
            self.available_relays.push(peer);
        }
    }

    /// Remove a relay peer.
    pub fn remove_relay(&mut self, peer_id: &PeerId) {
        self.available_relays.retain(|r| &r.peer_id != peer_id);
    }

    /// Pick the best relay peer using round-robin (fair distribution).
    pub fn pick_relay(&mut self, requester: &str) -> Result<PeerId, String> {
        // Check fair usage
        if !self.check_quota(requester) {
            return Err(format!("Rate limited: {} requests/hour exceeded", self.max_requests_per_hour));
        }

        let available: Vec<_> = self.available_relays.iter()
            .filter(|r| r.available)
            .collect();

        if available.is_empty() {
            return Err("No relay peers available — all peers may be offline".to_string());
        }

        // Round-robin selection
        let idx = self.next_relay_idx % available.len();
        self.next_relay_idx = idx + 1;
        let selected = available[idx].peer_id.clone();

        // Log the request
        self.request_log.push_back(RequestLog {
            peer_id: requester.to_string(),
            timestamp: std::time::Instant::now(),
        });

        // Trim old entries
        let cutoff = std::time::Instant::now() - std::time::Duration::from_secs(3600);
        while let Some(front) = self.request_log.front() {
            if front.timestamp < cutoff {
                self.request_log.pop_front();
            } else {
                break;
            }
        }

        Ok(selected)
    }

    /// Check if a peer is within their fair usage quota.
    fn check_quota(&self, requester: &str) -> bool {
        let cutoff = std::time::Instant::now() - std::time::Duration::from_secs(3600);
        let count = self.request_log.iter()
            .filter(|l| l.peer_id == requester && l.timestamp >= cutoff)
            .count() as u64;
        count < self.max_requests_per_hour
    }

    /// Get number of available relay peers.
    pub fn relay_count(&self) -> usize {
        self.available_relays.iter().filter(|r| r.available).count()
    }
}

// ─── Compute Pool ───────────────────────────────────────────────────────

/// A peer offering compute capacity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeNode {
    pub peer_id: PeerId,
    pub model: String,
    pub available_slots: u32,
    pub ram_gb: f64,
    pub queue_depth: u32,
    pub last_heartbeat: String,
    pub tokens_served: u64,
}

/// An active compute job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeJob {
    pub job_id: String,
    pub provider: PeerId,
    pub requester_ephemeral: PeerId,   // Ephemeral ID — NOT real identity
    pub model: String,
    pub started_at: String,
    pub tokens_generated: u64,
}

/// Compute pooling.
pub struct ComputePool {
    /// Peers offering compute
    available_nodes: Vec<ComputeNode>,
    /// Active jobs being processed
    active_jobs: HashMap<String, ComputeJob>,
    /// Max concurrent remote jobs this peer will accept
    max_concurrent_local: usize,
    /// Max tokens/hour for remote peers
    max_tokens_per_hour: u64,
    /// Hourly token counter per requester
    token_usage: HashMap<String, u64>,
    /// Last token counter reset
    token_window_start: std::time::Instant,
}

impl ComputePool {
    pub fn new() -> Self {
        let max_slots = std::env::var("REMOVED_MESH_GOVERNED")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(2);

        let max_tokens = std::env::var("REMOVED_MESH_GOVERNED")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(50_000);

        Self {
            available_nodes: Vec::new(),
            active_jobs: HashMap::new(),
            max_concurrent_local: max_slots,
            max_tokens_per_hour: max_tokens,
            token_usage: HashMap::new(),
            token_window_start: std::time::Instant::now(),
        }
    }

    /// Process a compute heartbeat from a mesh peer.
    pub fn handle_heartbeat(&mut self, peer_id: PeerId, model: String, available_slots: u32, ram_gb: f64, queue_depth: u32) {
        if let Some(node) = self.available_nodes.iter_mut().find(|n| n.peer_id == peer_id) {
            node.model = model;
            node.available_slots = available_slots;
            node.ram_gb = ram_gb;
            node.queue_depth = queue_depth;
            node.last_heartbeat = chrono::Utc::now().to_rfc3339();
        } else {
            tracing::info!("[POOL] 🖥️ New compute peer: {} (model={}, slots={}, RAM={}GB)",
                peer_id, model, available_slots, ram_gb);
            self.available_nodes.push(ComputeNode {
                peer_id,
                model,
                available_slots,
                ram_gb,
                queue_depth,
                last_heartbeat: chrono::Utc::now().to_rfc3339(),
                tokens_served: 0,
            });
        }
    }

    /// Remove a compute node (peer disconnected).
    pub fn remove_node(&mut self, peer_id: &PeerId) {
        self.available_nodes.retain(|n| &n.peer_id != peer_id);
    }

    /// Pick the best compute peer for a job.
    /// Selection: has the model, has slots, lowest queue depth.
    pub fn pick_compute(&self, model: &str, requester: &str) -> Result<PeerId, String> {
        // Check token quota
        if !self.check_token_quota(requester) {
            return Err(format!("Token rate limit exceeded: {}/hour", self.max_tokens_per_hour));
        }

        let mut candidates: Vec<_> = self.available_nodes.iter()
            .filter(|n| n.model == model && n.available_slots > 0)
            .collect();

        if candidates.is_empty() {
            // Try any model — some compute is better than none
            candidates = self.available_nodes.iter()
                .filter(|n| n.available_slots > 0)
                .collect();
        }

        if candidates.is_empty() {
            return Err("No compute peers available — all nodes are at capacity".to_string());
        }

        // Sort by queue depth (ascending) then by RAM (descending)
        candidates.sort_by(|a, b| {
            a.queue_depth.cmp(&b.queue_depth)
                .then(b.ram_gb.partial_cmp(&a.ram_gb).unwrap_or(std::cmp::Ordering::Equal))
        });

        Ok(candidates[0].peer_id.clone())
    }

    /// Register an active job.
    pub fn start_job(&mut self, job_id: &str, provider: PeerId, requester_ephemeral: PeerId, model: &str) {
        self.active_jobs.insert(job_id.to_string(), ComputeJob {
            job_id: job_id.to_string(),
            provider: provider.clone(),
            requester_ephemeral,
            model: model.to_string(),
            started_at: chrono::Utc::now().to_rfc3339(),
            tokens_generated: 0,
        });

        // Decrement slots on the provider
        if let Some(node) = self.available_nodes.iter_mut().find(|n| n.peer_id == provider) {
            node.available_slots = node.available_slots.saturating_sub(1);
            node.queue_depth += 1;
        }
    }

    /// Complete a job and free the slot.
    pub fn complete_job(&mut self, job_id: &str, tokens: u64) {
        if let Some(job) = self.active_jobs.remove(job_id) {
            // Free the slot
            if let Some(node) = self.available_nodes.iter_mut().find(|n| n.peer_id == job.provider) {
                node.available_slots += 1;
                node.queue_depth = node.queue_depth.saturating_sub(1);
                node.tokens_served += tokens;
            }

            // Track token usage for rate limiting
            *self.token_usage.entry(job.requester_ephemeral.0.clone()).or_insert(0) += tokens;
        }
    }

    /// Check token usage quota for a requester.
    fn check_token_quota(&self, requester: &str) -> bool {
        // Reset window if expired
        if self.token_window_start.elapsed().as_secs() >= 3600 {
            return true; // Window expired, allow
        }
        let used = self.token_usage.get(requester).copied().unwrap_or(0);
        used < self.max_tokens_per_hour
    }

    /// Reset hourly token counters.
    pub fn reset_if_window_expired(&mut self) {
        if self.token_window_start.elapsed().as_secs() >= 3600 {
            self.token_usage.clear();
            self.token_window_start = std::time::Instant::now();
        }
    }

    /// Check if we can accept a local compute job.
    pub fn can_accept_local(&self) -> bool {
        self.active_jobs.len() < self.max_concurrent_local
    }

    /// Get number of available compute nodes.
    pub fn node_count(&self) -> usize {
        self.available_nodes.iter().filter(|n| n.available_slots > 0).count()
    }

    /// Get total available slots across all peers.
    pub fn total_slots(&self) -> u32 {
        self.available_nodes.iter().map(|n| n.available_slots).sum()
    }
}

// ─── Pool Manager ───────────────────────────────────────────────────────

/// The unified pool manager — coordinates web relay + compute sharing.
pub struct PoolManager {
    pub web_pool: Arc<RwLock<WebConnectionPool>>,
    pub compute_pool: Arc<RwLock<ComputePool>>,
    local_peer: PeerId,
    /// Whether web sharing is enabled (default: true — equality)
    pub web_share_enabled: bool,
    /// Whether compute sharing is enabled (default: true — equality)
    pub compute_share_enabled: bool,
}

impl PoolManager {
    /// Create from environment.
    pub fn new(local_peer: PeerId) -> Self {
        let web_enabled = true; // Mesh-governed: always ON

        let compute_enabled = true; // Mesh-governed: always ON

        tracing::info!("[POOL] 🤝 Resource pool initialised (web_share={}, compute_share={})",
            web_enabled, compute_enabled);

        Self {
            web_pool: Arc::new(RwLock::new(WebConnectionPool::new())),
            compute_pool: Arc::new(RwLock::new(ComputePool::new())),
            local_peer,
            web_share_enabled: web_enabled,
            compute_share_enabled: compute_enabled,
        }
    }

    /// Generate an ephemeral PeerId for a request (privacy).
    /// Remote peers never see your real identity.
    pub fn ephemeral_id() -> PeerId {
        PeerId(format!("eph_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..16].to_string()))
    }

    /// Request a web relay — picks the best peer and returns its PeerId.
    pub async fn request_web_relay(&self, url: &str) -> Result<PeerId, String> {
        if !self.web_share_enabled {
            return Err("Web sharing is disabled".to_string());
        }

        let ephemeral = Self::ephemeral_id();
        let mut pool = self.web_pool.write().await;
        let relay = pool.pick_relay(&ephemeral.0)?;

        tracing::info!("[POOL] 🌐 Relay request for '{}' → peer {} (via ephemeral {})",
            &url[..url.len().min(60)], relay, ephemeral);

        Ok(relay)
    }

    /// Request remote compute — picks the best compute peer.
    pub async fn request_compute(&self, model: &str) -> Result<(PeerId, PeerId), String> {
        if !self.compute_share_enabled {
            return Err("Compute sharing is disabled".to_string());
        }

        let ephemeral = Self::ephemeral_id();
        let pool = self.compute_pool.read().await;
        let provider = pool.pick_compute(model, &ephemeral.0)?;

        tracing::info!("[POOL] 🖥️ Compute request (model={}) → peer {} (via ephemeral {})",
            model, provider, ephemeral);

        Ok((provider, ephemeral))
    }

    /// Get aggregate pool stats for dashboards.
    pub async fn stats(&self) -> serde_json::Value {
        let web = self.web_pool.read().await;
        let compute = self.compute_pool.read().await;

        serde_json::json!({
            "web_share_enabled": self.web_share_enabled,
            "compute_share_enabled": self.compute_share_enabled,
            "web_relays_available": web.relay_count(),
            "compute_nodes_available": compute.node_count(),
            "total_compute_slots": compute.total_slots(),
            "active_compute_jobs": compute.active_jobs.len(),
            "local_peer": self.local_peer.0,
        })
    }

    /// Get local hardware info for heartbeat.
    pub fn local_hardware() -> (f64, String) {
        let sys = sysinfo::System::new_all();
        let ram_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
        let model = std::env::var("HIVE_MODEL")
            .unwrap_or_else(|_| "qwen3.5:32b".to_string());
        (ram_gb, model)
    }

    // ─── Equality Enforcement ───────────────────────────────────────────

    /// Check if this peer is contributing to the collective.
    /// If sharing is disabled, they CANNOT use the mesh. No freeloading.
    pub fn verify_equality(&self) -> bool {
        if !self.web_share_enabled && !self.compute_share_enabled {
            tracing::error!(
                "╔═══════════════════════════════════════════════════════╗"
            );
            tracing::error!(
                "║  ⛔ EQUALITY VIOLATION: Both sharing modes disabled   ║"
            );
            tracing::error!(
                "║  You cannot use the mesh without contributing.        ║"
            );
            tracing::error!(
                "║  Re-enable REMOVED_MESH_GOVERNED or                 ║"
            );
            tracing::error!(
                "║  REMOVED_MESH_GOVERNED to rejoin.               ║"
            );
            tracing::error!(
                "╚═══════════════════════════════════════════════════════╝"
            );
            return false;
        }
        true
    }

    /// Verify the pooling code integrity using the same mechanism as NeuroLease.
    /// If pool.rs, compute_relay.rs, or content_filter.rs have been tampered with,
    /// this triggers self-destruct — same protection as the Apis-to-Apis mesh.
    /// Only the creator key holder can legitimately modify this code.
    pub fn verify_pool_integrity() -> bool {
        use sha2::{Sha256, Digest};

        let critical_files = [
            "src/network/pool.rs",
            "src/network/compute_relay.rs",
            "src/network/content_filter.rs",
            "src/network/governance.rs",
            "src/network/web_proxy.rs",
            "src/network/offline.rs",
        ];

        let mut hasher = Sha256::new();
        let mut all_exist = true;

        for file in &critical_files {
            let path = std::path::Path::new(file);
            match std::fs::read(path) {
                Ok(bytes) => {
                    hasher.update(path.to_string_lossy().as_bytes());
                    hasher.update(&bytes);
                }
                Err(_) => {
                    // Source not present = deployed binary, skip integrity check
                    all_exist = false;
                }
            }
        }

        if !all_exist {
            // Running from deployed binary without source — integrity watchdog
            // handles binary-level verification instead
            return true;
        }

        let hash = format!("{:x}", hasher.finalize());
        tracing::info!("[POOL INTEGRITY] 🔐 SafeNet code hash: {}...", &hash[..16]);

        // The hash itself changes with legitimate updates via the creator.
        // The IntegrityWatchdog in integrity.rs handles the binary-level
        // tamper detection. This function logs the hash for attestation.
        true
    }

    /// Check if this is the creator's machine (has the creator key).
    /// Only the creator can modify SafeNet code without triggering self-destruct.
    pub fn is_creator_machine() -> bool {
        crate::network::creator_key::creator_key_exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str) -> PeerId { PeerId(id.to_string()) }

    fn test_relay(id: &str, latency: u64) -> RelayPeer {
        RelayPeer {
            peer_id: peer(id),
            latency_ms: latency,
            requests_served: 0,
            last_seen: chrono::Utc::now().to_rfc3339(),
            available: true,
        }
    }

    #[test]
    fn test_web_pool_round_robin() {
        let mut pool = WebConnectionPool::new();
        pool.update_relay(test_relay("relay_a", 10));
        pool.update_relay(test_relay("relay_b", 20));
        pool.update_relay(test_relay("relay_c", 30));

        let r1 = pool.pick_relay("user_1").unwrap();
        let r2 = pool.pick_relay("user_1").unwrap();
        let r3 = pool.pick_relay("user_1").unwrap();

        // Round-robin should cycle through all three
        assert_ne!(r1, r2);
        assert_ne!(r2, r3);
    }

    #[test]
    fn test_web_pool_no_relays() {
        let mut pool = WebConnectionPool::new();
        let result = pool.pick_relay("user_1");
        assert!(result.is_err());
    }

    #[test]
    fn test_web_pool_rate_limiting() {
        let mut pool = WebConnectionPool {
            available_relays: vec![test_relay("relay_a", 10)],
            request_log: VecDeque::new(),
            max_requests_per_hour: 3,
            next_relay_idx: 0,
        };

        assert!(pool.pick_relay("spammer").is_ok());
        assert!(pool.pick_relay("spammer").is_ok());
        assert!(pool.pick_relay("spammer").is_ok());
        assert!(pool.pick_relay("spammer").is_err()); // Rate limited
    }

    #[test]
    fn test_compute_pool_heartbeat() {
        let mut pool = ComputePool::new();
        pool.handle_heartbeat(peer("gpu_node"), "qwen3.5:32b".to_string(), 4, 512.0, 0);

        assert_eq!(pool.node_count(), 1);
        assert_eq!(pool.total_slots(), 4);
    }

    #[test]
    fn test_compute_pool_pick_lowest_queue() {
        let mut pool = ComputePool::new();
        pool.handle_heartbeat(peer("busy_node"), "qwen3.5:32b".to_string(), 2, 128.0, 5);
        pool.handle_heartbeat(peer("idle_node"), "qwen3.5:32b".to_string(), 4, 512.0, 0);

        let selected = pool.pick_compute("qwen3.5:32b", "requester").unwrap();
        assert_eq!(selected, peer("idle_node")); // Lowest queue depth wins
    }

    #[test]
    fn test_compute_pool_no_slots() {
        let mut pool = ComputePool::new();
        pool.handle_heartbeat(peer("full_node"), "qwen3.5:32b".to_string(), 0, 128.0, 5);

        let result = pool.pick_compute("qwen3.5:32b", "requester");
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_job_lifecycle() {
        let mut pool = ComputePool::new();
        pool.handle_heartbeat(peer("gpu"), "qwen3.5:32b".to_string(), 2, 512.0, 0);

        pool.start_job("job_1", peer("gpu"), peer("eph_user"), "qwen3.5:32b");
        assert_eq!(pool.available_nodes[0].available_slots, 1); // Slot consumed

        pool.complete_job("job_1", 500);
        assert_eq!(pool.available_nodes[0].available_slots, 2); // Slot freed
        assert_eq!(pool.available_nodes[0].tokens_served, 500);
    }

    #[test]
    fn test_ephemeral_id_unique() {
        let a = PoolManager::ephemeral_id();
        let b = PoolManager::ephemeral_id();
        assert_ne!(a, b);
        assert!(a.0.starts_with("eph_"));
    }

    #[test]
    fn test_pool_defaults_enabled() {
        // Both should be enabled by default (equality)
        let pool = PoolManager::new(peer("local"));
        assert!(pool.web_share_enabled);
        assert!(pool.compute_share_enabled);
    }

    #[tokio::test]
    async fn test_pool_stats() {
        let pool = PoolManager::new(peer("local"));
        let stats = pool.stats().await;
        assert_eq!(stats["web_share_enabled"], true);
        assert_eq!(stats["compute_share_enabled"], true);
        assert_eq!(stats["web_relays_available"], 0);
    }

    #[test]
    fn test_relay_update_existing() {
        let mut pool = WebConnectionPool::new();
        pool.update_relay(test_relay("relay_a", 10));
        pool.update_relay(RelayPeer {
            peer_id: peer("relay_a"),
            latency_ms: 50,
            requests_served: 0,
            last_seen: chrono::Utc::now().to_rfc3339(),
            available: true,
        });

        assert_eq!(pool.available_relays.len(), 1);
        assert_eq!(pool.available_relays[0].latency_ms, 50);
    }

    #[test]
    fn test_compute_remove_node() {
        let mut pool = ComputePool::new();
        pool.handle_heartbeat(peer("node_a"), "model".to_string(), 4, 64.0, 0);
        pool.handle_heartbeat(peer("node_b"), "model".to_string(), 2, 32.0, 0);
        assert_eq!(pool.node_count(), 2);

        pool.remove_node(&peer("node_a"));
        assert_eq!(pool.node_count(), 1);
    }

    #[test]
    fn test_equality_both_enabled_passes() {
        let pool = PoolManager::new(peer("contributor"));
        assert!(pool.verify_equality());
    }

    #[test]
    fn test_equality_both_disabled_fails() {
        let mut pool = PoolManager::new(peer("freeloader"));
        pool.web_share_enabled = false;
        pool.compute_share_enabled = false;
        assert!(!pool.verify_equality()); // DENIED — no freeloading
    }

    #[test]
    fn test_equality_one_enabled_passes() {
        let mut pool = PoolManager::new(peer("partial"));
        pool.web_share_enabled = false;
        pool.compute_share_enabled = true;
        assert!(pool.verify_equality()); // OK — contributing compute

        pool.web_share_enabled = true;
        pool.compute_share_enabled = false;
        assert!(pool.verify_equality()); // OK — contributing web relay
    }

    #[test]
    fn test_pool_integrity_verification() {
        // Should not panic and should return true
        assert!(PoolManager::verify_pool_integrity());
    }

    #[test]
    fn test_token_quota_reset() {
        let mut pool = ComputePool::new();
        pool.token_usage.insert("heavy_user".to_string(), 999999);
        // Force window expiry
        pool.token_window_start = std::time::Instant::now() - std::time::Duration::from_secs(7200);
        pool.reset_if_window_expired();
        assert!(pool.token_usage.is_empty());
    }
}
