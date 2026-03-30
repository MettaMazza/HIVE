/// Universal Access Queue — Everyone uses the mesh, credits buy priority.
///
/// CORE PRINCIPLE: No one is denied access. Even with zero credits, you
/// can use compute and network resources. Credits determine your place
/// in the queue, not whether you can use the service at all.
///
/// QUEUE TIERS:
///   1. Priority — Has credits AND chose to spend them for immediate service
///   2. Standard — Has credits but didn't boost, served FIFO
///   3. Free     — Zero credits, served when capacity is available, fair share
///
/// NEEDS-BASED PRIORITY (within Free tier):
///   - Emergency alerts get instant access regardless of credits
///   - First-time users get served faster (community welcome)
///   - High reputation peers get slight priority boost
///
/// FAIRNESS:
///   - Free tier uses round-robin so no single peer can hog resources
///   - Max 3 concurrent requests per peer in Free tier
///   - Priority expires after 5 minutes (reverts to Standard)

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Queue priority tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum QueueTier {
    /// Emergency — always served immediately.
    Emergency = 0,
    /// Paid priority — spent credits for immediate service.
    Priority = 1,
    /// Standard — has credits, normal FIFO.
    Standard = 2,
    /// Free — zero credits, served when capacity allows.
    Free = 3,
}

impl std::fmt::Display for QueueTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueueTier::Emergency => write!(f, "emergency"),
            QueueTier::Priority => write!(f, "priority"),
            QueueTier::Standard => write!(f, "standard"),
            QueueTier::Free => write!(f, "free"),
        }
    }
}

/// Type of resource being requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestType {
    Compute,
    NetworkRelay,
}

/// A queued request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedRequest {
    pub id: String,
    pub peer_id: String,
    pub request_type: RequestType,
    pub tier: QueueTier,
    pub enqueued_at: String,
    /// For priority tier: when the boost expires.
    pub priority_expires_at: Option<String>,
    /// Peer's reputation score at time of enqueue (for free-tier ordering).
    pub reputation_score: f64,
    /// Whether this is the peer's first-ever request (new user boost).
    pub is_first_request: bool,
}

/// Queue statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    pub emergency_count: usize,
    pub priority_count: usize,
    pub standard_count: usize,
    pub free_count: usize,
    pub total: usize,
    pub avg_wait_seconds: f64,
}

/// Configuration for the access queue.
#[derive(Debug, Clone)]
pub struct QueueConfig {
    /// Max concurrent requests per peer in free tier.
    pub free_tier_max_concurrent: usize,
    /// Priority boost duration in seconds.
    pub priority_duration_secs: u64,
    /// Reputation threshold for free-tier boost (0.0–1.0).
    pub reputation_boost_threshold: f64,
}

impl QueueConfig {
    pub fn from_env() -> Self {
        Self {
            free_tier_max_concurrent: std::env::var("REMOVED_MESH_GOVERNED")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(3),
            priority_duration_secs: std::env::var("REMOVED_MESH_GOVERNED")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(300),
            reputation_boost_threshold: std::env::var("REMOVED_MESH_GOVERNED")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(0.8),
        }
    }
}

/// The Universal Access Queue.
pub struct AccessQueue {
    /// Queued requests, ordered by tier then enqueue time.
    queue: Arc<RwLock<VecDeque<QueuedRequest>>>,
    /// Active requests per peer (for concurrency limiting).
    active_counts: Arc<RwLock<HashMap<String, usize>>>,
    /// Peers that have ever made a request (for first-request detection).
    known_peers: Arc<RwLock<std::collections::HashSet<String>>>,
    /// Configuration.
    config: QueueConfig,
}

impl AccessQueue {
    pub fn new() -> Self {
        let config = QueueConfig::from_env();
        tracing::info!("[QUEUE] 🎫 Universal access queue initialised (free_max_concurrent={})",
            config.free_tier_max_concurrent);

        Self {
            queue: Arc::new(RwLock::new(VecDeque::new())),
            active_counts: Arc::new(RwLock::new(HashMap::new())),
            known_peers: Arc::new(RwLock::new(std::collections::HashSet::new())),
            config,
        }
    }

    /// Enqueue a request. Determines tier based on credits and options.
    ///
    /// - `credit_balance` — peer's current credit balance
    /// - `reputation` — peer's reputation score (0.0–1.0)
    /// - `wants_priority` — peer chose to spend credits for priority
    /// - `is_emergency` — this is an emergency request
    pub async fn enqueue(
        &self,
        peer_id: &str,
        request_type: RequestType,
        credit_balance: f64,
        reputation: f64,
        wants_priority: bool,
        is_emergency: bool,
    ) -> Result<QueuedRequest, String> {
        // Determine tier
        let tier = if is_emergency {
            QueueTier::Emergency
        } else if wants_priority && credit_balance >= 5.0 {
            QueueTier::Priority
        } else if credit_balance > 0.0 {
            QueueTier::Standard
        } else {
            QueueTier::Free
        };

        // Free tier concurrency check
        if tier == QueueTier::Free {
            let counts = self.active_counts.read().await;
            let active = counts.get(peer_id).copied().unwrap_or(0);
            if active >= self.config.free_tier_max_concurrent {
                return Err(format!(
                    "Free tier: max {} concurrent requests. Wait for one to complete.",
                    self.config.free_tier_max_concurrent
                ));
            }
        }

        // Check if first-ever request
        let is_first = {
            let mut known = self.known_peers.write().await;
            !known.contains(peer_id) && {
                known.insert(peer_id.to_string());
                true
            }
        };

        let now = chrono::Utc::now();
        let priority_expires = if tier == QueueTier::Priority {
            Some((now + chrono::Duration::seconds(self.config.priority_duration_secs as i64)).to_rfc3339())
        } else {
            None
        };

        let request = QueuedRequest {
            id: format!("req_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..16].to_string()),
            peer_id: peer_id.to_string(),
            request_type,
            tier,
            enqueued_at: now.to_rfc3339(),
            priority_expires_at: priority_expires,
            reputation_score: reputation,
            is_first_request: is_first,
        };

        // Insert into queue maintaining tier order
        let mut queue = self.queue.write().await;
        let insert_pos = self.find_insert_position(&queue, &request);
        queue.insert(insert_pos, request.clone());

        // Track active count
        {
            let mut counts = self.active_counts.write().await;
            *counts.entry(peer_id.to_string()).or_insert(0) += 1;
        }

        tracing::info!("[QUEUE] 🎫 {} enqueued: {} tier, {} type (pos: {}/{})",
            &peer_id[..peer_id.len().min(12)], tier,
            match request_type { RequestType::Compute => "compute", RequestType::NetworkRelay => "network" },
            insert_pos + 1, queue.len());

        Ok(request)
    }

    /// Find the correct position to insert a request, maintaining tier order.
    /// Within the same tier, respects FIFO ordering with reputation boost for Free tier.
    fn find_insert_position(&self, queue: &VecDeque<QueuedRequest>, request: &QueuedRequest) -> usize {
        for (i, existing) in queue.iter().enumerate() {
            // Insert before any request with a lower-priority (higher number) tier
            if existing.tier > request.tier {
                return i;
            }
            // Within Free tier, high-reputation and first-time users get a slight boost
            if existing.tier == request.tier && request.tier == QueueTier::Free {
                if request.is_first_request && !existing.is_first_request {
                    return i;
                }
                if request.reputation_score > self.config.reputation_boost_threshold
                    && existing.reputation_score <= self.config.reputation_boost_threshold
                    && !existing.is_first_request
                {
                    return i;
                }
            }
        }
        queue.len() // Append at end
    }

    /// Dequeue the next request to serve. Handles expired priority boosts.
    pub async fn dequeue(&self) -> Option<QueuedRequest> {
        let mut queue = self.queue.write().await;

        // Clean up expired priority boosts (revert to Standard)
        let now = chrono::Utc::now();
        for request in queue.iter_mut() {
            if request.tier == QueueTier::Priority {
                if let Some(expires) = &request.priority_expires_at {
                    if let Ok(exp_time) = chrono::DateTime::parse_from_rfc3339(expires) {
                        if now > exp_time {
                            request.tier = QueueTier::Standard;
                            request.priority_expires_at = None;
                        }
                    }
                }
            }
        }

        // Re-sort after any tier changes, then return the front item
        let mut sorted: Vec<_> = queue.drain(..).collect();
        sorted.sort_by(|a, b| a.tier.cmp(&b.tier));

        if sorted.is_empty() {
            return None;
        }

        // Take the first (highest priority) item to return
        let result = sorted.remove(0);

        // Put the rest back into the queue
        for item in sorted {
            queue.push_back(item);
        }

        Some(result)
    }

    /// Mark a request as completed. Decrements active count.
    pub async fn complete(&self, peer_id: &str) {
        let mut counts = self.active_counts.write().await;
        if let Some(count) = counts.get_mut(peer_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.remove(peer_id);
            }
        }
    }

    /// Get position of a peer's oldest request in the queue.
    pub async fn position(&self, peer_id: &str) -> Option<usize> {
        let queue = self.queue.read().await;
        queue.iter().position(|r| r.peer_id == peer_id)
            .map(|p| p + 1) // 1-indexed for display
    }

    /// Get queue statistics.
    pub async fn stats(&self) -> QueueStats {
        let queue = self.queue.read().await;
        let now = chrono::Utc::now();

        let mut emergency = 0;
        let mut priority = 0;
        let mut standard = 0;
        let mut free = 0;
        let mut total_wait: f64 = 0.0;

        for request in queue.iter() {
            match request.tier {
                QueueTier::Emergency => emergency += 1,
                QueueTier::Priority => priority += 1,
                QueueTier::Standard => standard += 1,
                QueueTier::Free => free += 1,
            }
            if let Ok(enqueued) = chrono::DateTime::parse_from_rfc3339(&request.enqueued_at) {
                let enqueued_utc: chrono::DateTime<chrono::Utc> = enqueued.into();
                total_wait += (now - enqueued_utc).num_seconds() as f64;
            }
        }

        let total = queue.len();
        let avg_wait = if total > 0 { total_wait / total as f64 } else { 0.0 };

        QueueStats {
            emergency_count: emergency,
            priority_count: priority,
            standard_count: standard,
            free_count: free,
            total,
            avg_wait_seconds: avg_wait,
        }
    }

    /// Get queue stats as JSON.
    pub async fn stats_json(&self) -> serde_json::Value {
        let stats = self.stats().await;
        serde_json::json!({
            "emergency": stats.emergency_count,
            "priority": stats.priority_count,
            "standard": stats.standard_count,
            "free": stats.free_count,
            "total": stats.total,
            "avg_wait_seconds": format!("{:.1}", stats.avg_wait_seconds),
        })
    }

    /// Current queue depth.
    pub async fn depth(&self) -> usize {
        self.queue.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_enqueue_with_credits() {
        let queue = AccessQueue::new();
        let req = queue.enqueue("peer_a", RequestType::Compute, 50.0, 0.5, false, false).await.unwrap();
        assert_eq!(req.tier, QueueTier::Standard);
    }

    #[tokio::test]
    async fn test_enqueue_zero_credits() {
        let queue = AccessQueue::new();
        let req = queue.enqueue("peer_b", RequestType::Compute, 0.0, 0.5, false, false).await.unwrap();
        assert_eq!(req.tier, QueueTier::Free);
    }

    #[tokio::test]
    async fn test_enqueue_priority() {
        let queue = AccessQueue::new();
        let req = queue.enqueue("peer_c", RequestType::Compute, 100.0, 0.5, true, false).await.unwrap();
        assert_eq!(req.tier, QueueTier::Priority);
        assert!(req.priority_expires_at.is_some());
    }

    #[tokio::test]
    async fn test_enqueue_emergency() {
        let queue = AccessQueue::new();
        let req = queue.enqueue("peer_d", RequestType::Compute, 0.0, 0.5, false, true).await.unwrap();
        assert_eq!(req.tier, QueueTier::Emergency);
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let queue = AccessQueue::new();

        // Enqueue in reverse priority order
        queue.enqueue("free_peer", RequestType::Compute, 0.0, 0.5, false, false).await.unwrap();
        queue.enqueue("standard_peer", RequestType::Compute, 50.0, 0.5, false, false).await.unwrap();
        queue.enqueue("emergency_peer", RequestType::Compute, 0.0, 0.5, false, true).await.unwrap();

        // Emergency should come first
        let first = queue.dequeue().await.unwrap();
        assert_eq!(first.peer_id, "emergency_peer");
    }

    #[tokio::test]
    async fn test_free_tier_concurrency_limit() {
        let queue = AccessQueue::new();

        // Should allow 3 (default max)
        for i in 0..3 {
            let result = queue.enqueue("greedy_peer", RequestType::Compute, 0.0, 0.5, false, false).await;
            assert!(result.is_ok(), "Request {} should succeed", i);
        }

        // 4th should fail
        let result = queue.enqueue("greedy_peer", RequestType::Compute, 0.0, 0.5, false, false).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max"));
    }

    #[tokio::test]
    async fn test_complete_frees_slot() {
        let queue = AccessQueue::new();

        for _ in 0..3 {
            queue.enqueue("peer_e", RequestType::Compute, 0.0, 0.5, false, false).await.unwrap();
        }

        // Free a slot
        queue.complete("peer_e").await;

        // Should now allow another
        let result = queue.enqueue("peer_e", RequestType::Compute, 0.0, 0.5, false, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_first_request_boost() {
        let queue = AccessQueue::new();

        // Make old_peer a known peer (enqueue + complete a prior request)
        queue.enqueue("old_peer", RequestType::Compute, 0.0, 0.3, false, false).await.unwrap();
        queue.dequeue().await; // remove it
        queue.complete("old_peer").await;

        // Now enqueue old_peer again (no longer first request)
        queue.enqueue("old_peer", RequestType::Compute, 0.0, 0.3, false, false).await.unwrap();
        // New peer — should get boosted ahead since it's their first request
        queue.enqueue("new_peer", RequestType::Compute, 0.0, 0.3, false, false).await.unwrap();

        let first = queue.dequeue().await.unwrap();
        assert_eq!(first.peer_id, "new_peer"); // New peer boosted
    }

    #[tokio::test]
    async fn test_stats() {
        let queue = AccessQueue::new();
        queue.enqueue("p1", RequestType::Compute, 100.0, 0.5, true, false).await.unwrap();
        queue.enqueue("p2", RequestType::Compute, 50.0, 0.5, false, false).await.unwrap();
        queue.enqueue("p3", RequestType::Compute, 0.0, 0.5, false, false).await.unwrap();

        let stats = queue.stats().await;
        assert_eq!(stats.priority_count, 1);
        assert_eq!(stats.standard_count, 1);
        assert_eq!(stats.free_count, 1);
        assert_eq!(stats.total, 3);
    }

    #[tokio::test]
    async fn test_position() {
        let queue = AccessQueue::new();
        queue.enqueue("p1", RequestType::Compute, 100.0, 0.5, true, false).await.unwrap();
        queue.enqueue("p2", RequestType::Compute, 0.0, 0.5, false, false).await.unwrap();

        let pos = queue.position("p2").await;
        assert_eq!(pos, Some(2)); // Behind priority peer
    }
}
