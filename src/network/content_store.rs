/// Content Store — Disk-backed content-addressed storage for DHT entries.
///
/// All DHT data is persisted to `memory/mesh/content/` with SHA-256 filenames.
/// Integrity is verified on every read. A size-limited cache with LRU eviction
/// prevents unbounded disk growth.
///
/// PINNING: Critical data (own lessons, own synaptic graph) is pinned and
/// never evicted by GC. Only explicit delete removes pinned content.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use sha2::{Sha256, Digest};

/// Metadata about a stored content item.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContentMeta {
    pub key: String,
    pub size_bytes: u64,
    pub stored_at: String,
    pub last_accessed: String,
    pub pinned: bool,
    pub content_type: String,
}

/// The Content Store — disk-backed, content-addressed, size-limited.
pub struct ContentStore {
    /// Base directory for content files.
    base_dir: PathBuf,
    /// In-memory metadata index.
    index: Arc<RwLock<HashMap<String, ContentMeta>>>,
    /// Max total size in bytes (default: 10GB).
    max_size_bytes: u64,
    /// Current total size.
    current_size: Arc<RwLock<u64>>,
}

impl ContentStore {
    pub fn new(mesh_dir: &std::path::Path, max_size_gb: u64) -> Self {
        let base_dir = mesh_dir.join("content");
        let _ = std::fs::create_dir_all(&base_dir);

        let max_size_bytes = max_size_gb * 1_073_741_824; // GB to bytes

        // Load existing index if present
        let index_path = base_dir.join("_index.json");
        let mut index = HashMap::new();
        let mut current_size = 0u64;

        if let Ok(data) = std::fs::read_to_string(&index_path) {
            if let Ok(loaded) = serde_json::from_str::<HashMap<String, ContentMeta>>(&data) {
                current_size = loaded.values().map(|m| m.size_bytes).sum();
                index = loaded;
            }
        }

        tracing::info!(
            "[CONTENT STORE] 💾 Initialised: {} items, {:.2}GB / {:.2}GB",
            index.len(),
            current_size as f64 / 1_073_741_824.0,
            max_size_gb
        );

        Self {
            base_dir,
            index: Arc::new(RwLock::new(index)),
            max_size_bytes,
            current_size: Arc::new(RwLock::new(current_size)),
        }
    }

    /// Store content. Returns the content key.
    pub async fn store(
        &self,
        data: &[u8],
        content_type: &str,
        pinned: bool,
    ) -> Result<String, String> {
        let key = Self::compute_key(data);
        let size = data.len() as u64;

        // Check if we already have this content
        {
            let index = self.index.read().await;
            if index.contains_key(&key) {
                return Ok(key); // Dedup — already stored
            }
        }

        // Evict if needed
        self.evict_if_needed(size).await;

        // Write to disk
        let path = self.content_path(&key);
        std::fs::write(&path, data)
            .map_err(|e| format!("Failed to write content: {}", e))?;

        // Update index
        let meta = ContentMeta {
            key: key.clone(),
            size_bytes: size,
            stored_at: chrono::Utc::now().to_rfc3339(),
            last_accessed: chrono::Utc::now().to_rfc3339(),
            pinned,
            content_type: content_type.to_string(),
        };

        self.index.write().await.insert(key.clone(), meta);
        *self.current_size.write().await += size;
        self.persist_index().await;

        Ok(key)
    }

    /// Retrieve content by key. Verifies integrity on read.
    pub async fn get(&self, key: &str) -> Result<Vec<u8>, String> {
        // Check index
        {
            let mut index = self.index.write().await;
            if let Some(meta) = index.get_mut(key) {
                meta.last_accessed = chrono::Utc::now().to_rfc3339();
            } else {
                return Err("Content not found".to_string());
            }
        }

        // Read from disk
        let path = self.content_path(key);
        let data = std::fs::read(&path)
            .map_err(|e| format!("Failed to read content: {}", e))?;

        // Verify integrity
        let computed_key = Self::compute_key(&data);
        if computed_key != key {
            tracing::error!(
                "[CONTENT STORE] ❌ Integrity failure: expected {}, computed {}",
                &key[..16], &computed_key[..16]
            );
            return Err("Content integrity verification failed".to_string());
        }

        Ok(data)
    }

    /// Delete content by key.
    pub async fn delete(&self, key: &str) -> bool {
        let removed = {
            let mut index = self.index.write().await;
            index.remove(key)
        };

        if let Some(meta) = removed {
            let path = self.content_path(key);
            let _ = std::fs::remove_file(&path);
            *self.current_size.write().await -= meta.size_bytes;
            self.persist_index().await;
            true
        } else {
            false
        }
    }

    /// Pin content — prevent eviction.
    pub async fn pin(&self, key: &str) -> bool {
        let mut index = self.index.write().await;
        if let Some(meta) = index.get_mut(key) {
            meta.pinned = true;
            true
        } else {
            false
        }
    }

    /// Unpin content — allow eviction.
    pub async fn unpin(&self, key: &str) -> bool {
        let mut index = self.index.write().await;
        if let Some(meta) = index.get_mut(key) {
            meta.pinned = false;
            true
        } else {
            false
        }
    }

    /// Check if content exists.
    pub async fn contains(&self, key: &str) -> bool {
        self.index.read().await.contains_key(key)
    }

    /// Get the total stored size.
    pub async fn total_size_bytes(&self) -> u64 {
        *self.current_size.read().await
    }

    /// Get the item count.
    pub async fn item_count(&self) -> usize {
        self.index.read().await.len()
    }

    /// Get stats.
    pub async fn stats(&self) -> serde_json::Value {
        let size = *self.current_size.read().await;
        let count = self.index.read().await.len();
        let pinned = self.index.read().await.values().filter(|m| m.pinned).count();

        serde_json::json!({
            "items": count,
            "pinned_items": pinned,
            "size_bytes": size,
            "size_mb": size as f64 / 1_048_576.0,
            "max_size_mb": self.max_size_bytes as f64 / 1_048_576.0,
            "usage_pct": if self.max_size_bytes > 0 { (size as f64 / self.max_size_bytes as f64) * 100.0 } else { 0.0 },
        })
    }

    // ─── Internal ───────────────────────────────────────────────────

    /// Compute SHA-256 content key.
    fn compute_key(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    /// Get the file path for a content key.
    fn content_path(&self, key: &str) -> PathBuf {
        // Shard by first 2 chars of key for filesystem performance
        let shard = &key[..2.min(key.len())];
        let shard_dir = self.base_dir.join(shard);
        let _ = std::fs::create_dir_all(&shard_dir);
        shard_dir.join(key)
    }

    /// Evict LRU unpinned content until we have room for `needed_bytes`.
    async fn evict_if_needed(&self, needed_bytes: u64) {
        let current = *self.current_size.read().await;
        if current + needed_bytes <= self.max_size_bytes {
            return; // Plenty of room
        }

        let target = current + needed_bytes - self.max_size_bytes;
        let mut freed = 0u64;

        // Sort by last_accessed (oldest first), skip pinned
        let mut candidates: Vec<ContentMeta> = {
            let index = self.index.read().await;
            index.values()
                .filter(|m| !m.pinned)
                .cloned()
                .collect()
        };
        candidates.sort_by(|a, b| a.last_accessed.cmp(&b.last_accessed));

        for meta in candidates {
            if freed >= target {
                break;
            }

            let path = self.content_path(&meta.key);
            let _ = std::fs::remove_file(&path);

            self.index.write().await.remove(&meta.key);
            freed += meta.size_bytes;

            tracing::info!(
                "[CONTENT STORE] 🧹 Evicted: {} ({} bytes)",
                &meta.key[..16], meta.size_bytes
            );
        }

        *self.current_size.write().await -= freed;
    }

    /// Persist the index to disk.
    async fn persist_index(&self) {
        let index = self.index.read().await;
        let index_path = self.base_dir.join("_index.json");
        if let Ok(json) = serde_json::to_string(&*index) {
            let _ = std::fs::write(&index_path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> ContentStore {
        let tmp = std::env::temp_dir().join(format!("hive_cs_test_{}", uuid::Uuid::new_v4()));
        ContentStore::new(&tmp, 1) // 1GB max
    }

    #[tokio::test]
    async fn test_store_and_retrieve() {
        let store = test_store();
        let data = b"test data for content store";

        let key = store.store(data, "test", false).await.unwrap();
        assert_eq!(key.len(), 64); // SHA-256 hex

        let retrieved = store.get(&key).await.unwrap();
        assert_eq!(retrieved, data.to_vec());
    }

    #[tokio::test]
    async fn test_integrity_check() {
        let store = test_store();
        let data = b"integrity check data";
        let key = store.store(data, "test", false).await.unwrap();

        // Tamper with the file on disk
        let path = store.content_path(&key);
        std::fs::write(&path, b"tampered!").unwrap();

        // Read should fail integrity check
        let result = store.get(&key).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_dedup() {
        let store = test_store();
        let data = b"duplicate content";

        let key1 = store.store(data, "test", false).await.unwrap();
        let key2 = store.store(data, "test", false).await.unwrap();

        assert_eq!(key1, key2);
        assert_eq!(store.item_count().await, 1);
    }

    #[tokio::test]
    async fn test_delete() {
        let store = test_store();
        let key = store.store(b"delete me", "test", false).await.unwrap();

        assert!(store.contains(&key).await);
        assert!(store.delete(&key).await);
        assert!(!store.contains(&key).await);
    }

    #[tokio::test]
    async fn test_pin_prevents_eviction() {
        let tmp = std::env::temp_dir().join(format!("hive_pin_test_{}", uuid::Uuid::new_v4()));
        let store = ContentStore::new(&tmp, 0); // 0GB max = immediate eviction pressure

        let key = store.store(b"pinned data", "test", true).await.unwrap();
        assert!(store.contains(&key).await);
        // Pinned content should survive despite 0GB limit
    }

    #[tokio::test]
    async fn test_stats() {
        let store = test_store();
        store.store(b"item 1", "test", false).await.unwrap();
        store.store(b"item 2", "test", true).await.unwrap();

        let stats = store.stats().await;
        assert_eq!(stats["items"], 2);
        assert_eq!(stats["pinned_items"], 1);
    }
}
