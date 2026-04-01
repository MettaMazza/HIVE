/// Task Queue — Persistent distributed task queue for long-running mesh jobs.
///
/// Jobs survive peer disconnections — if a provider goes offline, the job is
/// automatically re-dispatched to another peer. No manual intervention.
///
/// PRIORITY: Jobs are ordered by a combination of mesh reputation and age.
/// DEDUP: Identical requests from different peers are merged.
/// PROGRESS: Providers send periodic `JobProgress` updates.
/// TIMEOUT: Jobs that exceed their timeout are automatically re-queued.
use std::collections::{HashMap, BinaryHeap};
use std::cmp::Ordering;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::network::messages::PeerId;

/// Status of a queued job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    /// Waiting in queue for a provider.
    Queued,
    /// Assigned to a provider, running.
    Running { provider: PeerId, started_at: String },
    /// Completed successfully.
    Completed { result: String, completed_at: String },
    /// Failed — will be re-queued if retries remain.
    Failed { reason: String, retries_left: u32 },
    /// Cancelled by the requester.
    Cancelled,
}

/// A task in the distributed queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedTask {
    pub task_id: String,
    pub job_type: String,
    pub model: String,
    pub payload: String,
    pub max_tokens: u32,
    pub requester: PeerId,
    pub priority: f64,
    pub status: TaskStatus,
    pub created_at: String,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub attempt: u32,
    pub progress_pct: f32,
    pub last_progress_at: Option<String>,
}

/// Priority wrapper for the heap (highest priority first).
#[derive(Debug, Clone)]
struct PriorityEntry {
    task_id: String,
    priority: f64,
    created_at: String,
}

impl PartialEq for PriorityEntry {
    fn eq(&self, other: &Self) -> bool {
        self.task_id == other.task_id
    }
}
impl Eq for PriorityEntry {}

impl PartialOrd for PriorityEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first, then older tasks first (FIFO within same priority)
        self.priority.partial_cmp(&other.priority)
            .unwrap_or(Ordering::Equal)
            .then_with(|| other.created_at.cmp(&self.created_at))
    }
}

/// The Distributed Task Queue.
pub struct TaskQueue {
    /// All tasks by ID.
    tasks: Arc<RwLock<HashMap<String, QueuedTask>>>,
    /// Priority queue for pending tasks.
    priority_queue: Arc<RwLock<BinaryHeap<PriorityEntry>>>,
    /// Payload hashes for deduplication.
    payload_hashes: Arc<RwLock<HashMap<String, String>>>, // hash → task_id
}

impl TaskQueue {
    pub fn new() -> Self {
        tracing::info!("[TASK QUEUE] 📋 Distributed task queue initialised");
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            priority_queue: Arc::new(RwLock::new(BinaryHeap::new())),
            payload_hashes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Submit a new task to the queue. Returns the task ID.
    /// If an identical task already exists, returns the existing task ID (dedup).
    pub async fn submit(
        &self,
        job_type: &str,
        model: &str,
        payload: &str,
        max_tokens: u32,
        requester: PeerId,
        priority: f64,
        timeout_secs: u64,
    ) -> String {
        // Deduplication check
        let payload_hash = {
            use sha2::{Sha256, Digest};
            let mut hasher = Sha256::new();
            hasher.update(model.as_bytes());
            hasher.update(payload.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        {
            let hashes = self.payload_hashes.read().await;
            if let Some(existing_id) = hashes.get(&payload_hash) {
                tracing::info!(
                    "[TASK QUEUE] 🔄 Dedup: identical task already queued as {}",
                    existing_id
                );
                return existing_id.clone();
            }
        }

        let task_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        let task = QueuedTask {
            task_id: task_id.clone(),
            job_type: job_type.to_string(),
            model: model.to_string(),
            payload: payload.to_string(),
            max_tokens,
            requester,
            priority,
            status: TaskStatus::Queued,
            created_at: now.clone(),
            timeout_secs,
            max_retries: 3,
            attempt: 0,
            progress_pct: 0.0,
            last_progress_at: None,
        };

        self.tasks.write().await.insert(task_id.clone(), task);
        self.payload_hashes.write().await.insert(payload_hash, task_id.clone());
        self.priority_queue.write().await.push(PriorityEntry {
            task_id: task_id.clone(),
            priority,
            created_at: now,
        });

        tracing::info!(
            "[TASK QUEUE] ➕ Task {} queued (type={}, priority={:.1})",
            &task_id[..8], job_type, priority
        );

        task_id
    }

    /// Dequeue the highest-priority task that's ready to run.
    pub async fn dequeue(&self) -> Option<QueuedTask> {
        let mut queue = self.priority_queue.write().await;
        let tasks = self.tasks.read().await;

        // Drain entries until we find one that's still queued
        while let Some(entry) = queue.pop() {
            if let Some(task) = tasks.get(&entry.task_id) {
                if task.status == TaskStatus::Queued {
                    return Some(task.clone());
                }
            }
        }
        None
    }

    /// Assign a task to a provider.
    pub async fn assign(&self, task_id: &str, provider: PeerId) -> Result<(), String> {
        let mut tasks = self.tasks.write().await;
        let task = tasks.get_mut(task_id)
            .ok_or_else(|| format!("Task {} not found", task_id))?;

        task.status = TaskStatus::Running {
            provider: provider.clone(),
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        task.attempt += 1;

        tracing::info!(
            "[TASK QUEUE] ▶️ Task {} assigned to {} (attempt {})",
            &task_id[..8], provider, task.attempt
        );
        Ok(())
    }

    /// Record progress on a running task.
    pub async fn update_progress(&self, task_id: &str, progress_pct: f32) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.progress_pct = progress_pct;
            task.last_progress_at = Some(chrono::Utc::now().to_rfc3339());
        }
    }

    /// Complete a task successfully.
    pub async fn complete(&self, task_id: &str, result: String) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = TaskStatus::Completed {
                result,
                completed_at: chrono::Utc::now().to_rfc3339(),
            };
            task.progress_pct = 100.0;

            tracing::info!(
                "[TASK QUEUE] ✅ Task {} completed (attempt {})",
                &task_id[..8], task.attempt
            );
        }
    }

    /// Fail a task — re-queue if retries remain.
    pub async fn fail(&self, task_id: &str, reason: &str) -> bool {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            if task.attempt < task.max_retries {
                // Re-queue for retry
                task.status = TaskStatus::Queued;
                task.progress_pct = 0.0;

                // Re-add to priority queue
                drop(tasks); // Release write lock before acquiring another
                self.priority_queue.write().await.push(PriorityEntry {
                    task_id: task_id.to_string(),
                    priority: self.tasks.read().await.get(task_id)
                        .map(|t| t.priority).unwrap_or(0.0),
                    created_at: chrono::Utc::now().to_rfc3339(),
                });

                tracing::warn!(
                    "[TASK QUEUE] ⚠️ Task {} failed ({}), re-queued (attempt {}/{})",
                    &task_id[..8], reason, 
                    self.tasks.read().await.get(task_id).map(|t| t.attempt).unwrap_or(0),
                    self.tasks.read().await.get(task_id).map(|t| t.max_retries).unwrap_or(0)
                );
                return true; // Re-queued
            } else {
                task.status = TaskStatus::Failed {
                    reason: reason.to_string(),
                    retries_left: 0,
                };

                tracing::error!(
                    "[TASK QUEUE] ❌ Task {} permanently failed after {} attempts: {}",
                    &task_id[..8], task.attempt, reason
                );
                return false; // Permanently failed
            }
        }
        false
    }

    /// Cancel a task.
    pub async fn cancel(&self, task_id: &str, requester: &PeerId) -> Result<(), String> {
        let mut tasks = self.tasks.write().await;
        let task = tasks.get_mut(task_id)
            .ok_or_else(|| format!("Task {} not found", task_id))?;

        if task.requester != *requester {
            return Err("Only the requester can cancel a task".to_string());
        }

        task.status = TaskStatus::Cancelled;
        tracing::info!("[TASK QUEUE] 🚫 Task {} cancelled", &task_id[..8]);
        Ok(())
    }

    /// Check for timed-out tasks and re-queue them.
    pub async fn check_timeouts(&self) -> usize {
        let mut timed_out = Vec::new();

        {
            let tasks = self.tasks.read().await;
            for (id, task) in tasks.iter() {
                if let TaskStatus::Running { started_at, .. } = &task.status {
                    if let Ok(started) = chrono::DateTime::parse_from_rfc3339(started_at) {
                        let elapsed = chrono::Utc::now().signed_duration_since(started);
                        if elapsed.num_seconds() > task.timeout_secs as i64 {
                            timed_out.push(id.clone());
                        }
                    }
                }
            }
        }

        let count = timed_out.len();
        for id in timed_out {
            self.fail(&id, "Timeout exceeded").await;
        }

        if count > 0 {
            tracing::warn!("[TASK QUEUE] ⏰ {} tasks timed out and re-queued", count);
        }
        count
    }

    /// Get queue stats.
    pub async fn stats(&self) -> serde_json::Value {
        let tasks = self.tasks.read().await;
        let queued = tasks.values().filter(|t| t.status == TaskStatus::Queued).count();
        let running = tasks.values().filter(|t| matches!(t.status, TaskStatus::Running { .. })).count();
        let completed = tasks.values().filter(|t| matches!(t.status, TaskStatus::Completed { .. })).count();
        let failed = tasks.values().filter(|t| matches!(t.status, TaskStatus::Failed { .. })).count();

        serde_json::json!({
            "total": tasks.len(),
            "queued": queued,
            "running": running,
            "completed": completed,
            "failed": failed,
        })
    }

    /// Get queue depth (queued + running).
    pub async fn depth(&self) -> usize {
        let tasks = self.tasks.read().await;
        tasks.values().filter(|t| {
            t.status == TaskStatus::Queued || matches!(t.status, TaskStatus::Running { .. })
        }).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_submit_and_dequeue() {
        let queue = TaskQueue::new();

        let id = queue.submit(
            "Inference", "qwen3.5:35b", "Hello world",
            2048, PeerId("requester_1".to_string()), 1.0, 120
        ).await;

        assert_eq!(queue.depth().await, 1);

        let task = queue.dequeue().await;
        assert!(task.is_some());
        assert_eq!(task.unwrap().task_id, id);
    }

    #[tokio::test]
    async fn test_dedup() {
        let queue = TaskQueue::new();

        let id1 = queue.submit(
            "Inference", "qwen3.5:35b", "Same prompt",
            2048, PeerId("user_a".to_string()), 1.0, 120
        ).await;

        let id2 = queue.submit(
            "Inference", "qwen3.5:35b", "Same prompt",
            2048, PeerId("user_b".to_string()), 1.0, 120
        ).await;

        // Should return same task ID (dedup)
        assert_eq!(id1, id2);
        assert_eq!(queue.depth().await, 1);
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let queue = TaskQueue::new();

        let _low_id = queue.submit(
            "Inference", "model", "low priority",
            100, PeerId("u".to_string()), 0.1, 60
        ).await;

        let high_id = queue.submit(
            "Inference", "model", "high priority",
            100, PeerId("u".to_string()), 9.0, 60
        ).await;

        // High priority should dequeue first
        let first = queue.dequeue().await.unwrap();
        assert_eq!(first.task_id, high_id);
    }

    #[tokio::test]
    async fn test_assign_and_complete() {
        let queue = TaskQueue::new();

        let id = queue.submit(
            "Inference", "model", "test",
            100, PeerId("u".to_string()), 1.0, 60
        ).await;

        queue.assign(&id, PeerId("provider_1".to_string())).await.unwrap();
        queue.update_progress(&id, 50.0).await;
        queue.complete(&id, "answer!".to_string()).await;

        let stats = queue.stats().await;
        assert_eq!(stats["completed"], 1);
    }

    #[tokio::test]
    async fn test_fail_and_retry() {
        let queue = TaskQueue::new();

        let id = queue.submit(
            "Inference", "model", "retry test",
            100, PeerId("u".to_string()), 1.0, 60
        ).await;

        queue.assign(&id, PeerId("p1".to_string())).await.unwrap();

        // First failure — should re-queue
        let requeued = queue.fail(&id, "timeout").await;
        assert!(requeued);

        // Task should be dequeue-able again
        let task = queue.dequeue().await;
        assert!(task.is_some());
    }

    #[tokio::test]
    async fn test_cancel() {
        let queue = TaskQueue::new();
        let requester = PeerId("owner".to_string());

        let id = queue.submit(
            "Inference", "model", "cancel me",
            100, requester.clone(), 1.0, 60
        ).await;

        queue.cancel(&id, &requester).await.unwrap();

        let stats = queue.stats().await;
        assert_eq!(stats["queued"], 0);
    }

    #[tokio::test]
    async fn test_wrong_user_cannot_cancel() {
        let queue = TaskQueue::new();

        let id = queue.submit(
            "Inference", "model", "mine",
            100, PeerId("owner".to_string()), 1.0, 60
        ).await;

        let result = queue.cancel(&id, &PeerId("hacker".to_string())).await;
        assert!(result.is_err());
    }
}
