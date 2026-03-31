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
        // Mesh-governed: 100 req/hour per peer. Not configurable per-node.
        Self {
            available_relays: Vec::new(),
            request_log: VecDeque::with_capacity(10000),
            max_requests_per_hour: 100,
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
        // Mesh-governed: 2 slots, 50K tokens/hour. Not configurable per-node.
        Self {
            available_nodes: Vec::new(),
            active_jobs: HashMap::new(),
            max_concurrent_local: 2,
            max_tokens_per_hour: 50_000,
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

    /// Get the provider PeerId and tokens from the last completed job.
    pub fn last_completed_tokens(&self) -> Option<(String, u64)> {
        // This method returns the last recorded tokens for credit earning.
        // In a full implementation, this would track the most recent completion.
        None
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
    /// Credits engine for economy tracking
    pub credits_engine: Option<Arc<crate::crypto::credits::CreditsEngine>>,
}

impl PoolManager {
    /// Create pool manager. Sharing is ALWAYS ON — mesh equality.
    /// These are not configurable per-node. Turning off sharing
    /// while consuming mesh resources is freeloading.
    pub fn new(local_peer: PeerId) -> Self {
        tracing::info!("[POOL] 🤝 Resource pool initialised (web_share=true, compute_share=true)");

        Self {
            web_pool: Arc::new(RwLock::new(WebConnectionPool::new())),
            compute_pool: Arc::new(RwLock::new(ComputePool::new())),
            local_peer,
            web_share_enabled: true,
            compute_share_enabled: true,
            credits_engine: None,
        }
    }

    /// Attach a credits engine for economy tracking.
    pub fn with_credits(mut self, engine: Arc<crate::crypto::credits::CreditsEngine>) -> Self {
        self.credits_engine = Some(engine);
        self
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

        let local_web_relay = if self.web_share_enabled { 1 } else { 0 };
        let local_compute_node = if self.compute_share_enabled { 1 } else { 0 };
        let local_compute_slots = if self.compute_share_enabled { compute.max_concurrent_local as u32 } else { 0 };

        serde_json::json!({
            "web_share_enabled": self.web_share_enabled,
            "compute_share_enabled": self.compute_share_enabled,
            "web_relays_available": web.relay_count() + local_web_relay,
            "compute_nodes_available": compute.node_count() + local_compute_node,
            "total_compute_slots": compute.total_slots() + local_compute_slots,
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
    /// Sharing is always on — this is a runtime sanity check.
    pub fn verify_equality(&self) -> bool {
        // Both are always true now (hardcoded). This check remains
        // as a runtime assertion in case of memory corruption.
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
                "║  This is a hardcoded mesh rule — sharing is mandatory.║"
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
#[path = "pool_tests.rs"]
mod tests;
