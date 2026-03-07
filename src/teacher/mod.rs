use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};

use crate::models::message::Event;
use crate::models::scope::Scope;

/// A single golden example: first-pass Observer approval, perfect interaction.
#[derive(Debug, Serialize, Deserialize)]
pub struct GoldenExample {
    pub ts: String,
    pub system_prompt: String,
    pub user_msg: String,
    pub swarm_ctx: String,
    pub response: String,
    pub tools: Vec<String>,
    pub attempts: usize,
}

/// A single ORPO preference pair: blocked response vs corrected response.
#[derive(Debug, Serialize, Deserialize)]
pub struct PreferencePair {
    pub ts: String,
    pub prompt: String,
    pub chosen: String,
    pub rejected: String,
    pub failure_category: String,
    pub observer_reason: String,
    pub rejection_index: usize,
    pub total_attempts: usize,
}

/// Model version metadata for manifest.json lineage tracking.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModelVersion {
    pub version: String,
    pub date: String,
    pub golden_count: usize,
    pub pair_count: usize,
    pub parent: String,
}

/// Manifest tracking model lineage and retention.
#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub current: String,
    pub base: String,
    pub history: Vec<ModelVersion>,
    pub retention: usize,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            current: "qwen3.5:35b".to_string(),
            base: "qwen3.5:35b".to_string(),
            history: vec![],
            retention: 5,
        }
    }
}

/// Configuration constants for the training supervisor.
pub const GOLDEN_THRESHOLD: usize = 5;
pub const PAIR_THRESHOLD: usize = 3;
pub const MIN_COOLDOWN_SECS: u64 = 15 * 60; // 15 minutes

/// The Teacher captures golden examples and preference pairs for self-supervised learning.
/// It uses atomic counters on the hot path and a separate training lock for the background task.
#[derive(Clone)]
pub struct Teacher {
    pub golden_path: PathBuf,
    pub preference_path: PathBuf,
    archive_dir: PathBuf,
    manifest_path: PathBuf,
    training_lock: Arc<Mutex<bool>>,
    pub golden_count: Arc<AtomicUsize>,
    pub preference_count: Arc<AtomicUsize>,
}

impl Teacher {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        let base = base_dir.unwrap_or_else(|| PathBuf::from("./memory/teacher"));
        std::fs::create_dir_all(&base).ok();
        let archive = base.join("archive");
        std::fs::create_dir_all(&archive).ok();

        let golden_path = base.join("golden_buffer.jsonl");
        let preference_path = base.join("preference_buffer.jsonl");

        // Initialize counters from existing files so we don't start at 0 on restart
        let initial_golden = std::fs::read_to_string(&golden_path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count();
            
        let initial_preference = std::fs::read_to_string(&preference_path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count();

        Self {
            golden_path,
            preference_path,
            archive_dir: archive,
            manifest_path: base.join("manifest.json"),
            training_lock: Arc::new(Mutex::new(false)),
            golden_count: Arc::new(AtomicUsize::new(initial_golden)),
            preference_count: Arc::new(AtomicUsize::new(initial_preference)),
        }
    }

    /// Capture a golden example (first-pass Observer approval). Public scope only.
    pub async fn capture_golden(
        &self,
        system_prompt: &str,
        event: &Event,
        swarm_ctx: &str,
        response: &str,
    ) {
        // Privacy guard: never train on DM content
        if matches!(event.scope, Scope::Private { .. }) {
            return;
        }

        let example = GoldenExample {
            ts: chrono::Utc::now().to_rfc3339(),
            system_prompt: system_prompt.to_string(),
            user_msg: event.content.clone(),
            swarm_ctx: swarm_ctx.to_string(),
            response: response.to_string(),
            tools: Self::extract_tools(swarm_ctx),
            attempts: 1,
        };

        if let Ok(json) = serde_json::to_string(&example) {
            use tokio::io::AsyncWriteExt;
            if let Ok(mut file) = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.golden_path)
                .await
            {
                let _ = file.write_all(format!("{}\n", json).as_bytes()).await;
                self.golden_count.fetch_add(1, Ordering::Relaxed);
                println!("[TEACHER] 🏆 Golden example captured ({} buffered)", self.golden_count.load(Ordering::Relaxed));
            }
        }
    }

    /// Capture an ORPO preference pair (blocked → corrected). Public scope only.
    pub async fn capture_preference_pair(
        &self,
        system_prompt: &str,
        event: &Event,
        rejected: &str,
        chosen: &str,
        failure_category: &str,
        observer_reason: &str,
        rejection_index: usize,
        total_attempts: usize,
    ) {
        if matches!(event.scope, Scope::Private { .. }) {
            return;
        }

        let pair = PreferencePair {
            ts: chrono::Utc::now().to_rfc3339(),
            prompt: format!("{}\n\nUser: {}", system_prompt, event.content),
            chosen: chosen.to_string(),
            rejected: rejected.to_string(),
            failure_category: failure_category.to_string(),
            observer_reason: observer_reason.to_string(),
            rejection_index,
            total_attempts,
        };

        if let Ok(json) = serde_json::to_string(&pair) {
            use tokio::io::AsyncWriteExt;
            if let Ok(mut file) = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.preference_path)
                .await
            {
                let _ = file.write_all(format!("{}\n", json).as_bytes()).await;
                self.preference_count.fetch_add(1, Ordering::Relaxed);
                println!("[TEACHER] ⚖️ Preference pair captured [{}] ({} buffered)", failure_category, self.preference_count.load(Ordering::Relaxed));
            }
        }
    }

    /// Get current buffer counts (atomic, zero overhead on hot path).
    pub fn get_counts(&self) -> (usize, usize) {
        (
            self.golden_count.load(Ordering::Relaxed),
            self.preference_count.load(Ordering::Relaxed),
        )
    }

    /// Try to acquire the training lock. Returns true if acquired.
    pub async fn try_acquire_training_lock(&self) -> bool {
        let mut locked = self.training_lock.lock().await;
        if *locked {
            false
        } else {
            *locked = true;
            true
        }
    }

    /// Release the training lock.
    pub async fn release_training_lock(&self) {
        let mut locked = self.training_lock.lock().await;
        *locked = false;
    }

    /// Reset buffer counters after training processes the files.
    pub fn reset_counts(&self) {
        self.golden_count.store(0, Ordering::Relaxed);
        self.preference_count.store(0, Ordering::Relaxed);
    }

    /// Load or create the manifest file.
    pub fn load_manifest(&self) -> Manifest {
        if let Ok(data) = std::fs::read_to_string(&self.manifest_path) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Manifest::default()
        }
    }

    /// Save manifest to disk.
    pub fn save_manifest(&self, manifest: &Manifest) {
        if let Ok(json) = serde_json::to_string_pretty(manifest) {
            let _ = std::fs::write(&self.manifest_path, json);
        }
    }

    /// Extract tool names from swarm context string.
    pub fn extract_tools(swarm_ctx: &str) -> Vec<String> {
        let mut tools = vec![];
        for line in swarm_ctx.lines() {
            if line.starts_with("Drone:") || line.contains("drone_type") {
                tools.push(line.trim().to_string());
            }
        }
        tools
    }

    /// Get the archive directory path.
    pub fn get_archive_dir(&self) -> &PathBuf {
        &self.archive_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::scope::Scope;
    use tempfile::TempDir;

    fn test_event(scope: Scope) -> Event {
        Event {
            platform: "test".into(),
            scope,
            author_name: "Tester".into(),
            author_id: "123".into(),
            content: "Hello Apis".into(),
        }
    }

    #[test]
    fn test_teacher_new() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));
        assert_eq!(teacher.get_counts(), (0, 0));
    }

    #[test]
    fn test_teacher_default_dir() {
        let teacher = Teacher::new(None);
        assert_eq!(teacher.golden_path, PathBuf::from("./memory/teacher/golden_buffer.jsonl"));
        assert_eq!(teacher.preference_path, PathBuf::from("./memory/teacher/preference_buffer.jsonl"));
    }

    #[tokio::test]
    async fn test_capture_golden_public() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let event = test_event(Scope::Public {
            channel_id: "ch1".into(),
            user_id: "u1".into(),
        });

        teacher.capture_golden("system", &event, "swarm ctx", "response").await;
        assert_eq!(teacher.get_counts(), (1, 0));

        let content: String = tokio::fs::read_to_string(tmp.path().join("golden_buffer.jsonl")).await.unwrap();
        assert!(content.contains("Hello Apis"));
        assert!(content.contains("response"));
    }

    #[tokio::test]
    async fn test_capture_golden_private_skipped() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let event = test_event(Scope::Private {
            user_id: "u1".into(),
        });

        teacher.capture_golden("system", &event, "ctx", "resp").await;
        assert_eq!(teacher.get_counts(), (0, 0));
    }

    #[tokio::test]
    async fn test_capture_preference_pair() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let event = test_event(Scope::Public {
            channel_id: "ch1".into(),
            user_id: "u1".into(),
        });

        teacher.capture_preference_pair(
            "system", &event,
            "bad response", "good response",
            "ghost_tooling", "Claimed tool use without execution",
            1, 2,
        ).await;
        assert_eq!(teacher.get_counts(), (0, 1));

        let content: String = tokio::fs::read_to_string(tmp.path().join("preference_buffer.jsonl")).await.unwrap();
        assert!(content.contains("ghost_tooling"));
        assert!(content.contains("bad response"));
        assert!(content.contains("good response"));
    }

    #[tokio::test]
    async fn test_capture_preference_pair_private_skipped() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let event = test_event(Scope::Private {
            user_id: "u1".into(),
        });

        teacher.capture_preference_pair(
            "system", &event,
            "bad", "good",
            "sycophancy", "Tone is wrong",
            1, 2,
        ).await;
        assert_eq!(teacher.get_counts(), (0, 0));
    }

    #[tokio::test]
    async fn test_multiple_golden_captures() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let event = test_event(Scope::Public {
            channel_id: "ch1".into(),
            user_id: "u1".into(),
        });

        teacher.capture_golden("sys1", &event, "Drone: researcher", "resp1").await;
        teacher.capture_golden("sys2", &event, "ctx2", "resp2").await;
        teacher.capture_golden("sys3", &event, "ctx3", "resp3").await;
        assert_eq!(teacher.get_counts(), (3, 0));

        let content = tokio::fs::read_to_string(tmp.path().join("golden_buffer.jsonl")).await.unwrap();
        let lines: Vec<&str> = content.trim().split('\n').collect();
        assert_eq!(lines.len(), 3);
    }

    #[tokio::test]
    async fn test_multiple_preference_pairs() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let event = test_event(Scope::Public {
            channel_id: "ch1".into(),
            user_id: "u1".into(),
        });

        teacher.capture_preference_pair("sys", &event, "bad1", "good1", "ghost_tooling", "reason1", 1, 3).await;
        teacher.capture_preference_pair("sys", &event, "bad2", "good1", "sycophancy", "reason2", 2, 3).await;
        assert_eq!(teacher.get_counts(), (0, 2));

        let content = tokio::fs::read_to_string(tmp.path().join("preference_buffer.jsonl")).await.unwrap();
        let lines: Vec<&str> = content.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert!(content.contains("ghost_tooling"));
        assert!(content.contains("sycophancy"));
    }

    #[tokio::test]
    async fn test_training_lock() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        assert!(teacher.try_acquire_training_lock().await);
        assert!(!teacher.try_acquire_training_lock().await);
        teacher.release_training_lock().await;
        assert!(teacher.try_acquire_training_lock().await);
    }

    #[test]
    fn test_manifest_default() {
        let manifest = Manifest::default();
        assert_eq!(manifest.current, "qwen3.5:35b");
        assert_eq!(manifest.base, "qwen3.5:35b");
        assert!(manifest.history.is_empty());
        assert_eq!(manifest.retention, 5);
    }

    #[test]
    fn test_manifest_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let mut manifest = Manifest::default();
        manifest.current = "apis-v1-20260307".into();
        manifest.history.push(ModelVersion {
            version: "apis-v1-20260307".into(),
            date: "2026-03-07".into(),
            golden_count: 10,
            pair_count: 3,
            parent: "qwen3.5:35b".into(),
        });

        teacher.save_manifest(&manifest);
        let loaded = teacher.load_manifest();
        assert_eq!(loaded.current, "apis-v1-20260307");
        assert_eq!(loaded.history.len(), 1);
        assert_eq!(loaded.history[0].golden_count, 10);
        assert_eq!(loaded.history[0].pair_count, 3);
        assert_eq!(loaded.history[0].parent, "qwen3.5:35b");
    }

    #[test]
    fn test_manifest_load_missing_file() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));
        let manifest = teacher.load_manifest();
        assert_eq!(manifest.current, "qwen3.5:35b");
    }

    #[test]
    fn test_manifest_load_corrupt_data() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));
        std::fs::write(tmp.path().join("manifest.json"), "not valid json").unwrap();
        let manifest = teacher.load_manifest();
        assert_eq!(manifest.current, "qwen3.5:35b");
    }

    #[test]
    fn test_reset_counts() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));
        teacher.golden_count.store(5, Ordering::Relaxed);
        teacher.preference_count.store(3, Ordering::Relaxed);
        teacher.reset_counts();
        assert_eq!(teacher.get_counts(), (0, 0));
    }

    #[test]
    fn test_extract_tools_with_drones() {
        let ctx = "Step 1: planning\nDrone: researcher\nStep 2: execute\ndrone_type: native_codebase_list\nSome other line";
        let tools = Teacher::extract_tools(ctx);
        assert_eq!(tools.len(), 2);
        assert!(tools[0].contains("Drone:"));
        assert!(tools[1].contains("drone_type"));
    }

    #[test]
    fn test_extract_tools_empty() {
        let tools = Teacher::extract_tools("no drone info here");
        assert!(tools.is_empty());
        let tools2 = Teacher::extract_tools("");
        assert!(tools2.is_empty());
    }

    #[test]
    fn test_archive_dir_exists() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));
        assert!(teacher.get_archive_dir().exists());
    }

    #[test]
    fn test_constants() {
        assert_eq!(GOLDEN_THRESHOLD, 5);
        assert_eq!(PAIR_THRESHOLD, 3);
        assert_eq!(MIN_COOLDOWN_SECS, 900);
    }
}
