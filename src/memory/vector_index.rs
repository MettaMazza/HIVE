//! Vector Index — in-memory cosine similarity search with disk persistence.
//!
//! Zero external dependencies. Uses brute-force search (fast enough for <100K
//! entries on M3 Ultra). Persisted via rmp-serde (MessagePack) for compact storage.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::RwLock;

/// Source type for a vector entry — tracks where the embedding came from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SourceType {
    Timeline,
    Synaptic,
    Lesson,
}

/// A single entry in the vector index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorEntry {
    /// Unique identifier (e.g. "timeline:public_123:user_456:42")
    pub id: String,
    /// What type of memory this came from
    pub source: SourceType,
    /// First 200 chars of the original text for display
    pub text_preview: String,
    /// The embedding vector (768-dim for nomic-embed-text)
    pub vector: Vec<f32>,
    /// RFC3339 timestamp
    pub timestamp: String,
}

/// In-memory vector store with cosine similarity search.
#[derive(Debug)]
pub struct VectorIndex {
    pub(crate) entries: RwLock<Vec<VectorEntry>>,
    /// Set of known IDs for fast dedup checks
    pub(crate) known_ids: RwLock<std::collections::HashSet<String>>,
    dir: Option<PathBuf>,
}

impl Clone for VectorIndex {
    fn clone(&self) -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
            known_ids: RwLock::new(std::collections::HashSet::new()),
            dir: self.dir.clone(),
        }
    }
}

impl VectorIndex {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        let dir = base_dir.map(|d| {
            let p = d.join("vectors");
            let _ = std::fs::create_dir_all(&p);
            p
        });
        Self {
            entries: RwLock::new(Vec::new()),
            known_ids: RwLock::new(std::collections::HashSet::new()),
            dir,
        }
    }

    /// Load the index from disk on startup.
    pub async fn load(&self) {
        if let Some(ref dir) = self.dir {
            let path = dir.join("index.bin");
            if path.exists() {
                match tokio::fs::read(&path).await {
                    Ok(data) => {
                        match rmp_serde::from_slice::<Vec<VectorEntry>>(&data) {
                            Ok(loaded) => {
                                let count = loaded.len();
                                let mut ids = self.known_ids.write().await;
                                for entry in &loaded {
                                    ids.insert(entry.id.clone());
                                }
                                *self.entries.write().await = loaded;
                                tracing::info!("[VECTOR] Loaded {} entries from disk.", count);
                            }
                            Err(e) => {
                                tracing::warn!("[VECTOR] Failed to parse index.bin: {}. Starting fresh.", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[VECTOR] Failed to read index.bin: {}. Starting fresh.", e);
                    }
                }
            } else {
                tracing::info!("[VECTOR] No index.bin found. Starting fresh.");
            }
        }
    }

    /// Persist the index to disk.
    pub async fn save(&self) {
        if let Some(ref dir) = self.dir {
            let entries = self.entries.read().await;
            match rmp_serde::to_vec(&*entries) {
                Ok(data) => {
                    let path = dir.join("index.bin");
                    if let Err(e) = tokio::fs::write(&path, &data).await {
                        tracing::error!("[VECTOR] Failed to write index.bin: {}", e);
                    } else {
                        tracing::debug!("[VECTOR] Persisted {} entries to disk ({} bytes).", entries.len(), data.len());
                    }
                }
                Err(e) => {
                    tracing::error!("[VECTOR] Failed to serialize index: {}", e);
                }
            }
        }
    }

    /// Insert a new entry. Skips if the ID already exists.
    pub async fn insert(&self, entry: VectorEntry) {
        {
            let ids = self.known_ids.read().await;
            if ids.contains(&entry.id) {
                return; // Already indexed
            }
        }

        {
            let mut ids = self.known_ids.write().await;
            ids.insert(entry.id.clone());
        }

        let mut entries = self.entries.write().await;
        entries.push(entry);

        // Auto-save every 100 inserts
        if entries.len() % 100 == 0 {
            drop(entries);
            self.save().await;
        }
    }

    /// Check if an ID is already indexed.
    pub async fn contains(&self, id: &str) -> bool {
        self.known_ids.read().await.contains(id)
    }

    /// Search for the top-K most similar entries to the query vector.
    /// Returns (similarity_score, entry) pairs sorted by descending similarity.
    pub async fn search(&self, query_vec: &[f32], top_k: usize, source_filter: Option<SourceType>) -> Vec<(f32, VectorEntry)> {
        let entries = self.entries.read().await;

        let mut scored: Vec<(f32, &VectorEntry)> = entries.iter()
            .filter(|e| {
                source_filter.as_ref().map_or(true, |sf| &e.source == sf)
            })
            .map(|e| (cosine_similarity(query_vec, &e.vector), e))
            .collect();

        // Sort by similarity descending
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        scored.into_iter()
            .take(top_k)
            .map(|(score, entry)| (score, entry.clone()))
            .collect()
    }

    /// Get total entry count.
    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    /// Check if empty.
    pub async fn is_empty(&self) -> bool {
        self.entries.read().await.is_empty()
    }

    /// Get counts by source type.
    pub async fn stats(&self) -> (usize, usize, usize) {
        let entries = self.entries.read().await;
        let timeline = entries.iter().filter(|e| e.source == SourceType::Timeline).count();
        let synaptic = entries.iter().filter(|e| e.source == SourceType::Synaptic).count();
        let lesson = entries.iter().filter(|e| e.source == SourceType::Lesson).count();
        (timeline, synaptic, lesson)
    }
}

/// Cosine similarity between two vectors.
/// Assumes vectors are normalized (Ollama normalizes embeddings by default).
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_identical() {
        let v = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_insert_and_search() {
        let index = VectorIndex::new(None);

        let entry1 = VectorEntry {
            id: "t1".into(),
            source: SourceType::Timeline,
            text_preview: "Hello world".into(),
            vector: vec![1.0, 0.0, 0.0],
            timestamp: "2026-01-01T00:00:00Z".into(),
        };

        let entry2 = VectorEntry {
            id: "t2".into(),
            source: SourceType::Timeline,
            text_preview: "Goodbye world".into(),
            vector: vec![0.9, 0.1, 0.0],
            timestamp: "2026-01-01T00:00:01Z".into(),
        };

        let entry3 = VectorEntry {
            id: "s1".into(),
            source: SourceType::Synaptic,
            text_preview: "Concept: Ethics".into(),
            vector: vec![0.0, 1.0, 0.0],
            timestamp: "2026-01-01T00:00:02Z".into(),
        };

        index.insert(entry1).await;
        index.insert(entry2).await;
        index.insert(entry3).await;

        assert_eq!(index.len().await, 3);

        // Search for something similar to entry1
        let query = vec![1.0, 0.0, 0.0];
        let results = index.search(&query, 2, None).await;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1.id, "t1"); // Most similar
        assert_eq!(results[1].1.id, "t2"); // Second most

        // Search with source filter
        let results = index.search(&query, 10, Some(SourceType::Synaptic)).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.id, "s1");
    }

    #[tokio::test]
    async fn test_dedup() {
        let index = VectorIndex::new(None);

        let entry = VectorEntry {
            id: "dup".into(),
            source: SourceType::Timeline,
            text_preview: "Test".into(),
            vector: vec![1.0, 0.0, 0.0],
            timestamp: "2026-01-01T00:00:00Z".into(),
        };

        index.insert(entry.clone()).await;
        index.insert(entry).await;
        assert_eq!(index.len().await, 1);
    }

    #[tokio::test]
    async fn test_persistence() {
        let tmp = std::env::temp_dir().join(format!("hive_vec_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        // Write
        {
            let index = VectorIndex::new(Some(tmp.clone()));
            index.insert(VectorEntry {
                id: "p1".into(),
                source: SourceType::Timeline,
                text_preview: "Persisted entry".into(),
                vector: vec![0.5, 0.5, 0.0],
                timestamp: "2026-01-01T00:00:00Z".into(),
            }).await;
            index.save().await;
        }

        // Read in a new instance
        {
            let index = VectorIndex::new(Some(tmp.clone()));
            index.load().await;
            assert_eq!(index.len().await, 1);
            assert!(index.contains("p1").await);
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
