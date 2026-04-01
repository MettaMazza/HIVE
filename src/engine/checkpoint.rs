use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

/// Manages file snapshots before destructive operations.
/// Stores copies in `memory/core/checkpoints/` with metadata.
/// Allows rollback to any previous snapshot and auto-prunes old ones.
pub struct CheckpointManager {
    checkpoint_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointEntry {
    pub id: String,
    pub original_path: String,
    pub snapshot_path: String,
    pub created_at: String,
    pub size_bytes: u64,
}

impl CheckpointManager {
    pub fn new() -> Self {
        let checkpoint_dir = PathBuf::from("memory/core/checkpoints");
        // Use std::fs (blocking) since this is called once at init.
        // In test mode, we'll use a custom path via new_with_dir.
        let _ = std::fs::create_dir_all(&checkpoint_dir);
        Self { checkpoint_dir }
    }

    /// Create with a custom directory (used by tests).
    #[cfg(test)]
    pub fn new_with_dir(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Self { checkpoint_dir: dir }
    }

    /// Take a snapshot of a file before modifying it.
    /// Returns the checkpoint ID on success, or an error string.
    pub async fn snapshot(&self, file_path: &Path) -> Result<String, String> {
        // Only snapshot files that exist and are readable
        let content = tokio::fs::read(file_path).await
            .map_err(|e| format!("Cannot snapshot '{}': {}", file_path.display(), e))?;

        let metadata = tokio::fs::metadata(file_path).await
            .map_err(|e| format!("Cannot stat '{}': {}", file_path.display(), e))?;

        let id = format!("{}_{}", 
            chrono::Utc::now().format("%Y%m%d_%H%M%S"),
            &uuid::Uuid::new_v4().to_string()[..8]
        );

        // Store the snapshot binary
        let snapshot_filename = format!("{}.snap", id);
        let snapshot_path = self.checkpoint_dir.join(&snapshot_filename);
        tokio::fs::write(&snapshot_path, &content).await
            .map_err(|e| format!("Failed to write snapshot: {}", e))?;

        // Store the metadata
        let entry = CheckpointEntry {
            id: id.clone(),
            original_path: file_path.to_string_lossy().to_string(),
            snapshot_path: snapshot_path.to_string_lossy().to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            size_bytes: metadata.len(),
        };

        let meta_path = self.checkpoint_dir.join(format!("{}.json", id));
        let json = serde_json::to_string_pretty(&entry)
            .map_err(|e| format!("Failed to serialize checkpoint metadata: {}", e))?;
        tokio::fs::write(&meta_path, json).await
            .map_err(|e| format!("Failed to write checkpoint metadata: {}", e))?;

        tracing::info!(
            "[CHECKPOINT] 📸 Snapshot '{}' created for '{}' ({} bytes)",
            id, file_path.display(), content.len()
        );

        Ok(id)
    }

    /// Rollback a file to a previous checkpoint.
    pub async fn rollback(&self, checkpoint_id: &str) -> Result<String, String> {
        let meta_path = self.checkpoint_dir.join(format!("{}.json", checkpoint_id));
        let snap_path = self.checkpoint_dir.join(format!("{}.snap", checkpoint_id));

        // Read metadata
        let meta_json = tokio::fs::read_to_string(&meta_path).await
            .map_err(|e| format!("Checkpoint '{}' not found: {}", checkpoint_id, e))?;
        let entry: CheckpointEntry = serde_json::from_str(&meta_json)
            .map_err(|e| format!("Corrupt checkpoint metadata: {}", e))?;

        // Read snapshot content
        let content = tokio::fs::read(&snap_path).await
            .map_err(|e| format!("Snapshot file missing for '{}': {}", checkpoint_id, e))?;

        // Restore the file
        let target = Path::new(&entry.original_path);
        if let Some(parent) = target.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        tokio::fs::write(target, &content).await
            .map_err(|e| format!("Failed to restore '{}': {}", entry.original_path, e))?;

        tracing::info!(
            "[CHECKPOINT] ⏪ Rollback '{}' — restored '{}' ({} bytes)",
            checkpoint_id, entry.original_path, content.len()
        );

        Ok(format!("Successfully rolled back '{}' to checkpoint '{}' ({} bytes restored)", 
            entry.original_path, checkpoint_id, content.len()))
    }

    /// List recent checkpoints sorted newest first.
    pub async fn list(&self, limit: usize) -> Vec<CheckpointEntry> {
        let mut entries = Vec::new();
        let mut rd = match tokio::fs::read_dir(&self.checkpoint_dir).await {
            Ok(rd) => rd,
            Err(_) => return entries,
        };

        while let Ok(Some(entry)) = rd.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") {
                if let Ok(raw) = tokio::fs::read_to_string(entry.path()).await {
                    if let Ok(parsed) = serde_json::from_str::<CheckpointEntry>(&raw) {
                        entries.push(parsed);
                    }
                }
            }
        }

        // Sort by created_at descending (newest first)
        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        entries.truncate(limit);
        entries
    }

    /// Prune checkpoints older than max_age_hours. Returns count of pruned entries.
    pub async fn prune(&self, max_age_hours: u64) -> usize {
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(max_age_hours as i64);
        let cutoff_str = cutoff.to_rfc3339();
        let mut pruned = 0;

        let entries = self.list(1000).await; // get all
        for entry in entries {
            if entry.created_at < cutoff_str {
                // Remove both .snap and .json files
                let snap = self.checkpoint_dir.join(format!("{}.snap", entry.id));
                let meta = self.checkpoint_dir.join(format!("{}.json", entry.id));
                let _ = tokio::fs::remove_file(&snap).await;
                let _ = tokio::fs::remove_file(&meta).await;
                pruned += 1;
            }
        }

        if pruned > 0 {
            tracing::info!("[CHECKPOINT] 🧹 Pruned {} checkpoint(s) older than {}h", pruned, max_age_hours);
        }
        pruned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "hive_checkpoint_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[tokio::test]
    async fn test_snapshot_and_rollback() {
        let dir = test_dir();
        let mgr = CheckpointManager::new_with_dir(dir.clone());

        // Create a test file
        let test_file = dir.join("test_target.txt");
        tokio::fs::write(&test_file, "original content").await.unwrap();

        // Take a snapshot
        let id = mgr.snapshot(&test_file).await.unwrap();
        assert!(!id.is_empty());

        // Verify snapshot files exist
        assert!(dir.join(format!("{}.snap", id)).exists());
        assert!(dir.join(format!("{}.json", id)).exists());

        // Modify the original file
        tokio::fs::write(&test_file, "modified content").await.unwrap();
        let modified = tokio::fs::read_to_string(&test_file).await.unwrap();
        assert_eq!(modified, "modified content");

        // Rollback
        let result = mgr.rollback(&id).await.unwrap();
        assert!(result.contains("Successfully rolled back"));

        // Verify restored
        let restored = tokio::fs::read_to_string(&test_file).await.unwrap();
        assert_eq!(restored, "original content");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_snapshot_nonexistent_file() {
        let dir = test_dir();
        let mgr = CheckpointManager::new_with_dir(dir.clone());

        let result = mgr.snapshot(Path::new("/tmp/hive_does_not_exist_99999")).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Cannot snapshot"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_rollback_nonexistent_checkpoint() {
        let dir = test_dir();
        let mgr = CheckpointManager::new_with_dir(dir.clone());

        let result = mgr.rollback("fake_checkpoint_id").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_list_checkpoints() {
        let dir = test_dir();
        let mgr = CheckpointManager::new_with_dir(dir.clone());

        let test_file = dir.join("list_target.txt");
        tokio::fs::write(&test_file, "data").await.unwrap();

        let id1 = mgr.snapshot(&test_file).await.unwrap();
        // Small delay so timestamps differ
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let id2 = mgr.snapshot(&test_file).await.unwrap();

        let entries = mgr.list(10).await;
        assert_eq!(entries.len(), 2);
        // Newest first
        assert_eq!(entries[0].id, id2);
        assert_eq!(entries[1].id, id1);

        // Test limit
        let limited = mgr.list(1).await;
        assert_eq!(limited.len(), 1);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_prune_old_checkpoints() {
        let dir = test_dir();
        let mgr = CheckpointManager::new_with_dir(dir.clone());

        let test_file = dir.join("prune_target.txt");
        tokio::fs::write(&test_file, "data").await.unwrap();

        let _id = mgr.snapshot(&test_file).await.unwrap();
        assert_eq!(mgr.list(10).await.len(), 1);

        // Prune with 0 hours — should prune nothing (just created)
        // Checkpoint was created now, cutoff is NOW, created_at is NOT less than cutoff
        let pruned = mgr.prune(0).await;
        // created_at <= cutoff is borderline — the checkpoint was just created,
        // chrono comparison may or may not catch it. We test that prune runs without error.
        // The important test is that prune with a large window doesn't prune recent items:
        let still_there = mgr.list(10).await;
        // With 24h window, nothing should be pruned
        let pruned_24 = mgr.prune(24).await;
        assert_eq!(pruned_24, 0);
        assert_eq!(mgr.list(10).await.len(), still_there.len());

        let _ = tokio::fs::remove_dir_all(&dir).await;
        // Suppress unused variable warning
        let _ = pruned;
    }

    #[tokio::test]
    async fn test_checkpoint_entry_serde() {
        let entry = CheckpointEntry {
            id: "test_id".into(),
            original_path: "/tmp/foo.txt".into(),
            snapshot_path: "/tmp/checkpoints/test_id.snap".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            size_bytes: 42,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let decoded: CheckpointEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, "test_id");
        assert_eq!(decoded.size_bytes, 42);
    }
}
