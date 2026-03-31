#![allow(clippy::too_many_arguments, clippy::field_reassign_with_default)]
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};



pub mod evaluation;
pub mod generation;
pub mod sleep;

pub use evaluation::PreferencePair;
pub use generation::GoldenExample;
pub use sleep::{SleepCycle, SleepConfig, SleepReport};

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
    pub auto_train_enabled: Arc<std::sync::atomic::AtomicBool>,
}

impl Teacher {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        #[cfg(test)]
        let default_dir = std::env::temp_dir().join(format!("hive_mem_test_teacher_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        #[cfg(not(test))]
        let default_dir = PathBuf::from("./memory/teacher");

        let base = base_dir.unwrap_or(default_dir);
        if let Err(e) = std::fs::create_dir_all(&base) {
            tracing::error!("[TEACHER] ❌ Failed to create teacher directory '{}': {}", base.display(), e);
        }
        let archive = base.join("archive");
        if let Err(e) = std::fs::create_dir_all(&archive) {
            tracing::error!("[TEACHER] ❌ Failed to create archive directory '{}': {}", archive.display(), e);
        }

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
            auto_train_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)), // OFF until 122B model download complete
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
        match serde_json::to_string_pretty(manifest) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.manifest_path, json) {
                    tracing::error!("[TEACHER] ❌ Failed to write manifest '{}': {}", self.manifest_path.display(), e);
                }
            }
            Err(e) => tracing::error!("[TEACHER] ❌ Failed to serialize manifest: {}", e),
        }
    }

    /// Extract tool names from agent context string.
    pub fn extract_tools(agent_ctx: &str) -> Vec<String> {
        let mut tools = vec![];
        for line in agent_ctx.lines() {
            if line.starts_with("Tool:") || line.contains("tool_type") {
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
    use std::sync::atomic::Ordering;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_teacher_initialization() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().to_path_buf();
        
        let teacher = Teacher::new(Some(path.clone()));
        
        assert_eq!(teacher.get_counts(), (0, 0));
        assert!(path.join("archive").exists());
        
        // Write bogus data to the buffers to test restart counting
        std::fs::write(teacher.golden_path.clone(), "line1\nline2\n\nline3").unwrap();
        std::fs::write(teacher.preference_path.clone(), "line1\n").unwrap();
        
        let teacher_reloaded = Teacher::new(Some(path));
        assert_eq!(teacher_reloaded.get_counts(), (3, 1));
    }

    #[tokio::test]
    async fn test_training_locks() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));
        
        let acquired = teacher.try_acquire_training_lock().await;
        assert!(acquired);
        
        let acquired_again = teacher.try_acquire_training_lock().await;
        assert!(!acquired_again);
        
        teacher.release_training_lock().await;
        
        let acquired_third = teacher.try_acquire_training_lock().await;
        assert!(acquired_third);
    }

    #[tokio::test]
    async fn test_reset_counts() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));
        
        teacher.golden_count.store(5, Ordering::Relaxed);
        teacher.preference_count.store(10, Ordering::Relaxed);
        
        assert_eq!(teacher.get_counts(), (5, 10));
        
        teacher.reset_counts();
        assert_eq!(teacher.get_counts(), (0, 0));
    }

    #[test]
    fn test_manifest_handling() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));
        
        let default_manifest = teacher.load_manifest();
        assert_eq!(default_manifest.current, "qwen3.5:35b");
        assert_eq!(default_manifest.history.len(), 0);
        
        let custom = Manifest {
            current: "llama3".into(),
            base: "llama3".into(),
            history: vec![],
            retention: 3,
        };
        
        teacher.save_manifest(&custom);
        
        let loaded = teacher.load_manifest();
        assert_eq!(loaded.current, "llama3");
        assert_eq!(loaded.retention, 3);
    }

    #[test]
    fn test_extract_tools() {
        let ctx = "Tool: web_search\nOther log text\ntool_type: file_writer\nDone.";
        let tools = Teacher::extract_tools(ctx);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0], "Tool: web_search");
        assert_eq!(tools[1], "tool_type: file_writer");
    }

    #[test]
    fn test_get_archive_dir() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));
        let archive = teacher.get_archive_dir();
        assert!(archive.ends_with("archive"));
    }
}

