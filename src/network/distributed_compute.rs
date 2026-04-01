/// Distributed Compute — Intelligent job distribution for the HIVE mesh supercomputer.
///
/// This is the brain that turns 1,000 independent machines into a parallel compute cluster.
///
/// FLOW:
///   1. User submits inference request
///   2. Local Ollama available + not overloaded? → Run locally
///   3. Otherwise: scan ComputePool for best peer (model match, free slots,
///      lowest queue, same region preferred, lowest latency)
///   4. Send ComputeRequest to best peer
///   5. Peer runs inference → returns ComputeResponse
///   6. Peer earns HIVE Coin via algorithmic block reward
///
/// BATCH MODE:
///   For parallelisable tasks (e.g., embedding 1,000 docs), the job is split
///   into N chunks and fanned out to N peers simultaneously. An aggregation
///   layer collects and merges results.
///
/// FAILOVER:
///   If a peer doesn't respond within the timeout, the job is automatically
///   re-dispatched to the next best peer. No manual intervention needed.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::network::messages::PeerId;

/// A compute job request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeJob {
    pub job_id: String,
    pub job_type: JobType,
    pub model: String,
    pub payload: String,
    pub max_tokens: u32,
    pub requester: PeerId,
    pub requester_region: String,
    pub priority: f64,
    pub created_at: String,
    pub timeout_secs: u64,
}

/// Types of compute jobs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobType {
    /// Single inference request.
    Inference,
    /// Embedding generation (parallelisable).
    Embedding,
    /// LoRA fine-tuning task.
    LoRATraining,
    /// Generic data processing.
    DataProcessing,
}

/// A chunk of a batch job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchChunk {
    pub batch_id: String,
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub payload: String,
    pub assigned_peer: Option<PeerId>,
    pub status: ChunkStatus,
    pub result: Option<String>,
}

/// Status of a batch chunk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChunkStatus {
    Pending,
    Assigned,
    Running,
    Completed,
    Failed,
}

/// Result of compute routing decision.
#[derive(Debug, Clone)]
pub enum RoutingDecision {
    /// Run locally — we have capacity.
    RunLocal,
    /// Route to a specific peer.
    RouteToNode(PeerId, f64), // peer, score
    /// Split into batch and fan out.
    BatchFanOut(Vec<(PeerId, usize)>), // peer, chunk_index
    /// No capacity anywhere — queue for later.
    Queued,
    /// No peers available at all.
    NoPeers,
}

/// Peer scoring for intelligent routing.
#[derive(Debug, Clone)]
pub struct PeerScore {
    pub peer_id: PeerId,
    pub model_match: bool,
    pub available_slots: u32,
    pub queue_depth: u32,
    pub latency_ms: u64,
    pub same_region: bool,
    pub ram_gb: f64,
    pub score: f64,
}

impl PeerScore {
    /// Calculate weighted routing score.
    /// Higher = better candidate.
    pub fn calculate(&mut self) {
        let latency_factor = if self.latency_ms == 0 { 1.0 } else { 1000.0 / self.latency_ms as f64 };
        let slot_factor = self.available_slots as f64;
        let queue_factor = 1.0 / (self.queue_depth as f64 + 1.0);
        let region_bonus = if self.same_region { 1.5 } else { 1.0 };
        let model_bonus = if self.model_match { 2.0 } else { 0.5 };

        self.score = latency_factor * slot_factor * queue_factor * region_bonus * model_bonus;
    }
}

/// The Distributed Compute Engine — routes jobs to the best available peer.
pub struct DistributedCompute {
    /// Active batch jobs being tracked.
    batch_jobs: Arc<RwLock<HashMap<String, Vec<BatchChunk>>>>,
    /// Per-peer latency measurements (from heartbeat round-trips).
    peer_latencies: Arc<RwLock<HashMap<String, u64>>>,
    /// Local peer's region.
    local_region: String,
    /// Local peer's model.
    local_model: String,
    /// Local peer's max concurrent jobs.
    local_max_slots: u32,
    /// Current local job count.
    local_active_jobs: Arc<RwLock<u32>>,
}

impl DistributedCompute {
    pub fn new(local_region: &str, local_model: &str, local_max_slots: u32) -> Self {
        tracing::info!(
            "[DISTRIBUTED COMPUTE] 🖥️ Initialised (region={}, model={}, max_slots={})",
            local_region, local_model, local_max_slots
        );

        Self {
            batch_jobs: Arc::new(RwLock::new(HashMap::new())),
            peer_latencies: Arc::new(RwLock::new(HashMap::new())),
            local_region: local_region.to_string(),
            local_model: local_model.to_string(),
            local_max_slots,
            local_active_jobs: Arc::new(RwLock::new(0)),
        }
    }

    /// Record a latency measurement for a peer (from heartbeat round-trip).
    pub async fn record_latency(&self, peer_id: &PeerId, latency_ms: u64) {
        self.peer_latencies.write().await.insert(peer_id.0.clone(), latency_ms);
    }

    /// Decide where to route a compute job.
    pub async fn route_job(
        &self,
        job: &ComputeJob,
        available_peers: &[(PeerId, String, u32, f64, u32, String)], // id, model, slots, ram, queue, region
    ) -> RoutingDecision {
        // 1. Can we run locally?
        let local_active = *self.local_active_jobs.read().await;
        if local_active < self.local_max_slots && job.model == self.local_model {
            return RoutingDecision::RunLocal;
        }

        if available_peers.is_empty() {
            return RoutingDecision::NoPeers;
        }

        // 2. Score all available peers
        let latencies = self.peer_latencies.read().await;
        let mut scores: Vec<PeerScore> = available_peers.iter()
            .filter(|(_, _, slots, _, _, _)| *slots > 0)
            .map(|(id, model, slots, ram, queue, region)| {
                let mut ps = PeerScore {
                    peer_id: id.clone(),
                    model_match: *model == job.model,
                    available_slots: *slots,
                    queue_depth: *queue,
                    latency_ms: latencies.get(&id.0).copied().unwrap_or(500),
                    same_region: *region == job.requester_region || *region == self.local_region,
                    ram_gb: *ram,
                    score: 0.0,
                };
                ps.calculate();
                ps
            })
            .collect();

        if scores.is_empty() {
            return RoutingDecision::Queued;
        }

        // 3. Sort by score (descending)
        scores.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        let best = &scores[0];
        tracing::info!(
            "[DISTRIBUTED COMPUTE] 🎯 Routing job {} to peer {} (score={:.2}, model_match={}, latency={}ms, region_match={})",
            job.job_id, best.peer_id, best.score, best.model_match, best.latency_ms, best.same_region
        );

        RoutingDecision::RouteToNode(best.peer_id.clone(), best.score)
    }

    /// Route a batch job — fan out chunks to multiple peers.
    pub async fn route_batch(
        &self,
        batch_id: &str,
        chunks: Vec<String>,
        model: &str,
        available_peers: &[(PeerId, String, u32, f64, u32, String)],
    ) -> RoutingDecision {
        if available_peers.is_empty() {
            return RoutingDecision::NoPeers;
        }

        let latencies = self.peer_latencies.read().await;
        let mut scores: Vec<PeerScore> = available_peers.iter()
            .filter(|(_, _, slots, _, _, _)| *slots > 0)
            .map(|(id, m, slots, ram, queue, region)| {
                let mut ps = PeerScore {
                    peer_id: id.clone(),
                    model_match: *m == model,
                    available_slots: *slots,
                    queue_depth: *queue,
                    latency_ms: latencies.get(&id.0).copied().unwrap_or(500),
                    same_region: *region == self.local_region,
                    ram_gb: *ram,
                    score: 0.0,
                };
                ps.calculate();
                ps
            })
            .collect();

        scores.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // Assign chunks round-robin to top N peers
        let mut assignments: Vec<(PeerId, usize)> = Vec::new();
        let mut batch_chunks: Vec<BatchChunk> = Vec::new();

        for (i, _chunk) in chunks.iter().enumerate() {
            let peer_idx = i % scores.len();
            let assigned = &scores[peer_idx];

            assignments.push((assigned.peer_id.clone(), i));
            batch_chunks.push(BatchChunk {
                batch_id: batch_id.to_string(),
                chunk_index: i,
                total_chunks: chunks.len(),
                payload: chunks[i].clone(),
                assigned_peer: Some(assigned.peer_id.clone()),
                status: ChunkStatus::Assigned,
                result: None,
            });
        }

        // Store batch state
        self.batch_jobs.write().await.insert(batch_id.to_string(), batch_chunks);

        tracing::info!(
            "[DISTRIBUTED COMPUTE] 📦 Batch {} fanned out: {} chunks to {} peers",
            batch_id, chunks.len(), scores.len().min(chunks.len())
        );

        RoutingDecision::BatchFanOut(assignments)
    }

    /// Record a chunk completion for a batch job.
    pub async fn record_chunk_result(
        &self,
        batch_id: &str,
        chunk_index: usize,
        result: String,
    ) -> Option<Vec<String>> {
        let mut jobs = self.batch_jobs.write().await;
        if let Some(chunks) = jobs.get_mut(batch_id) {
            if let Some(chunk) = chunks.get_mut(chunk_index) {
                chunk.status = ChunkStatus::Completed;
                chunk.result = Some(result);
            }

            // Check if all chunks are complete
            let all_complete = chunks.iter().all(|c| c.status == ChunkStatus::Completed);
            if all_complete {
                let results: Vec<String> = chunks.iter()
                    .filter_map(|c| c.result.clone())
                    .collect();

                tracing::info!(
                    "[DISTRIBUTED COMPUTE] ✅ Batch {} complete: {} chunks aggregated",
                    batch_id, results.len()
                );

                jobs.remove(batch_id);
                return Some(results);
            }
        }
        None
    }

    /// Mark a chunk as failed and return a peer for retry.
    pub async fn record_chunk_failure(
        &self,
        batch_id: &str,
        chunk_index: usize,
    ) {
        let mut jobs = self.batch_jobs.write().await;
        if let Some(chunks) = jobs.get_mut(batch_id) {
            if let Some(chunk) = chunks.get_mut(chunk_index) {
                chunk.status = ChunkStatus::Failed;
                chunk.assigned_peer = None; // Clear for reassignment
                tracing::warn!(
                    "[DISTRIBUTED COMPUTE] ⚠️ Batch {} chunk {} failed — available for retry",
                    batch_id, chunk_index
                );
            }
        }
    }

    /// Increment local active job count.
    pub async fn start_local_job(&self) {
        let mut count = self.local_active_jobs.write().await;
        *count += 1;
    }

    /// Decrement local active job count.
    pub async fn complete_local_job(&self) {
        let mut count = self.local_active_jobs.write().await;
        *count = count.saturating_sub(1);
    }

    /// Get active batch jobs count.
    pub async fn active_batches(&self) -> usize {
        self.batch_jobs.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_peers() -> Vec<(PeerId, String, u32, f64, u32, String)> {
        vec![
            (PeerId("eu_fast".to_string()), "qwen3.5:35b".to_string(), 2, 512.0, 0, "Europe".to_string()),
            (PeerId("eu_busy".to_string()), "qwen3.5:35b".to_string(), 1, 256.0, 3, "Europe".to_string()),
            (PeerId("us_big".to_string()), "qwen3.5:35b".to_string(), 4, 1024.0, 0, "Americas".to_string()),
            (PeerId("asia_small".to_string()), "llama3:8b".to_string(), 2, 64.0, 0, "Asia".to_string()),
        ]
    }

    fn test_job() -> ComputeJob {
        ComputeJob {
            job_id: "test_job_1".to_string(),
            job_type: JobType::Inference,
            model: "qwen3.5:35b".to_string(),
            payload: "What is the meaning of life?".to_string(),
            max_tokens: 2048,
            requester: PeerId("requester_1".to_string()),
            requester_region: "Europe".to_string(),
            priority: 1.0,
            created_at: chrono::Utc::now().to_rfc3339(),
            timeout_secs: 120,
        }
    }

    #[tokio::test]
    async fn test_route_to_local_when_available() {
        let dc = DistributedCompute::new("Europe", "qwen3.5:35b", 2);
        let job = test_job();
        let peers = test_peers();

        let decision = dc.route_job(&job, &peers).await;
        assert!(matches!(decision, RoutingDecision::RunLocal));
    }

    #[tokio::test]
    async fn test_route_to_peer_when_local_full() {
        let dc = DistributedCompute::new("Europe", "qwen3.5:35b", 2);

        // Fill local slots
        dc.start_local_job().await;
        dc.start_local_job().await;

        let job = test_job();
        let peers = test_peers();

        // Record latencies — EU fast has lowest latency
        dc.record_latency(&PeerId("eu_fast".to_string()), 10).await;
        dc.record_latency(&PeerId("eu_busy".to_string()), 20).await;
        dc.record_latency(&PeerId("us_big".to_string()), 100).await;
        dc.record_latency(&PeerId("asia_small".to_string()), 200).await;

        let decision = dc.route_job(&job, &peers).await;
        match decision {
            RoutingDecision::RouteToNode(peer, _score) => {
                // Should pick a European peer (region bonus) with model match
                assert!(peer.0 == "eu_fast" || peer.0 == "us_big",
                    "Should prefer eu_fast (region+model) or us_big (slots+model), got {}", peer.0);
            }
            other => panic!("Expected RouteToNode, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_no_peers_returns_no_peers() {
        let dc = DistributedCompute::new("Europe", "qwen3.5:35b", 0);
        let job = test_job();
        let decision = dc.route_job(&job, &[]).await;
        assert!(matches!(decision, RoutingDecision::NoPeers));
    }

    #[tokio::test]
    async fn test_batch_fan_out() {
        let dc = DistributedCompute::new("Europe", "qwen3.5:35b", 2);
        let peers = test_peers();
        let chunks = vec!["chunk_0".to_string(), "chunk_1".to_string(), "chunk_2".to_string()];

        let decision = dc.route_batch("batch_1", chunks, "qwen3.5:35b", &peers).await;
        match decision {
            RoutingDecision::BatchFanOut(assignments) => {
                assert_eq!(assignments.len(), 3);
            }
            other => panic!("Expected BatchFanOut, got {:?}", other),
        }

        assert_eq!(dc.active_batches().await, 1);
    }

    #[tokio::test]
    async fn test_batch_aggregation() {
        let dc = DistributedCompute::new("Europe", "qwen3.5:35b", 2);
        let peers = test_peers();
        let chunks = vec!["a".to_string(), "b".to_string()];

        dc.route_batch("batch_2", chunks, "qwen3.5:35b", &peers).await;

        // First chunk completes — batch not done yet
        let result = dc.record_chunk_result("batch_2", 0, "result_a".to_string()).await;
        assert!(result.is_none());

        // Second chunk completes — batch is done
        let result = dc.record_chunk_result("batch_2", 1, "result_b".to_string()).await;
        assert!(result.is_some());
        let results = result.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], "result_a");
        assert_eq!(results[1], "result_b");

        // Batch removed after completion
        assert_eq!(dc.active_batches().await, 0);
    }

    #[tokio::test]
    async fn test_chunk_failure_and_retry() {
        let dc = DistributedCompute::new("Europe", "qwen3.5:35b", 2);
        let peers = test_peers();
        let chunks = vec!["x".to_string(), "y".to_string()];

        dc.route_batch("batch_3", chunks, "qwen3.5:35b", &peers).await;
        dc.record_chunk_failure("batch_3", 0).await;

        // Batch still active (chunk 0 failed, chunk 1 pending)
        assert_eq!(dc.active_batches().await, 1);
    }

    #[test]
    fn test_peer_scoring() {
        let mut score = PeerScore {
            peer_id: PeerId("test".to_string()),
            model_match: true,
            available_slots: 4,
            queue_depth: 0,
            latency_ms: 10,
            same_region: true,
            ram_gb: 512.0,
            score: 0.0,
        };
        score.calculate();
        assert!(score.score > 0.0);

        // Same peer but wrong model and different region should score lower
        let mut worse = PeerScore {
            peer_id: PeerId("test2".to_string()),
            model_match: false,
            available_slots: 1,
            queue_depth: 5,
            latency_ms: 200,
            same_region: false,
            ram_gb: 64.0,
            score: 0.0,
        };
        worse.calculate();
        assert!(score.score > worse.score);
    }
}
