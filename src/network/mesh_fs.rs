/// Mesh File System — Distributed file sharing over the DHT.
///
/// Any file can be shared across the mesh. Large files are chunked
/// into 256KB pieces, each stored independently on different peers.
/// Retrieval pulls chunks in parallel from multiple peers.
///
/// PRIVACY: File names/paths are encrypted — peers storing chunks
/// cannot see what the file is or who created it.
///
/// PINNING: Users can pin files they want to always keep locally.
/// Unpinned chunks are subject to LRU eviction.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sha2::{Sha256, Digest};
use serde::{Deserialize, Serialize};

use crate::network::messages::PeerId;

/// Default chunk size: 256KB.
const CHUNK_SIZE: usize = 256 * 1024;

/// A file manifest — describes how to reassemble a chunked file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileManifest {
    /// Content hash of the complete file.
    pub file_hash: String,
    /// Ordered list of chunk content hashes.
    pub chunk_hashes: Vec<String>,
    /// Total file size in bytes.
    pub total_size: u64,
    /// Number of chunks.
    pub chunk_count: usize,
    /// Encrypted file metadata (name, path — encrypted with requester's key).
    pub encrypted_meta: Vec<u8>,
    /// Who shared this file (ephemeral ID).
    pub origin: PeerId,
    /// When the file was shared.
    pub shared_at: String,
}

/// A single chunk of a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunk {
    /// Content hash of this chunk.
    pub chunk_hash: String,
    /// The chunk data.
    pub data: Vec<u8>,
    /// Index in the file manifest.
    pub index: usize,
    /// Total chunks in the file.
    pub total: usize,
}

/// Status of a file retrieval operation.
#[derive(Debug, Clone, PartialEq)]
pub enum RetrievalStatus {
    /// All chunks retrieved.
    Complete(Vec<u8>),
    /// Some chunks still missing.
    Partial { received: usize, total: usize },
    /// Manifest not found.
    NotFound,
}

/// The Mesh File System.
pub struct MeshFS {
    /// File manifests by file hash.
    manifests: Arc<RwLock<HashMap<String, FileManifest>>>,
    /// Chunks stored locally by chunk hash.
    local_chunks: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    /// In-progress retrievals: file_hash → received chunks.
    retrievals: Arc<RwLock<HashMap<String, HashMap<usize, Vec<u8>>>>>,
    /// Local peer ID.
    local_peer: PeerId,
}

impl MeshFS {
    pub fn new(local_peer: PeerId) -> Self {
        tracing::info!("[MESH FS] 📂 Distributed file system initialised");
        Self {
            manifests: Arc::new(RwLock::new(HashMap::new())),
            local_chunks: Arc::new(RwLock::new(HashMap::new())),
            retrievals: Arc::new(RwLock::new(HashMap::new())),
            local_peer,
        }
    }

    /// Share a file — chunk it and return the manifest + chunks for DHT storage.
    pub async fn share_file(
        &self,
        data: &[u8],
        encrypted_meta: Vec<u8>,
    ) -> (FileManifest, Vec<FileChunk>) {
        let file_hash = Self::content_hash(data);

        // Split into chunks
        let mut chunks = Vec::new();
        let mut chunk_hashes = Vec::new();

        for (i, chunk_data) in data.chunks(CHUNK_SIZE).enumerate() {
            let chunk_hash = Self::content_hash(chunk_data);
            chunk_hashes.push(chunk_hash.clone());

            chunks.push(FileChunk {
                chunk_hash: chunk_hash.clone(),
                data: chunk_data.to_vec(),
                index: i,
                total: (data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE,
            });

            // Store locally
            self.local_chunks.write().await
                .insert(chunk_hash, chunk_data.to_vec());
        }

        let manifest = FileManifest {
            file_hash: file_hash.clone(),
            chunk_hashes,
            total_size: data.len() as u64,
            chunk_count: chunks.len(),
            encrypted_meta,
            origin: self.local_peer.clone(),
            shared_at: chrono::Utc::now().to_rfc3339(),
        };

        // Store manifest locally
        self.manifests.write().await
            .insert(file_hash.clone(), manifest.clone());

        tracing::info!(
            "[MESH FS] 📤 Shared file {}... ({} bytes, {} chunks)",
            &file_hash[..16], data.len(), chunks.len(),
        );

        (manifest, chunks)
    }

    /// Receive a file manifest from a remote peer.
    pub async fn receive_manifest(&self, manifest: FileManifest) {
        let hash = manifest.file_hash.clone();
        self.manifests.write().await.insert(hash.clone(), manifest);
        // Initialise retrieval tracking
        self.retrievals.write().await.insert(hash, HashMap::new());
    }

    /// Receive a chunk — either from local store or remote peer.
    /// Returns the complete file if all chunks are now available.
    pub async fn receive_chunk(
        &self,
        file_hash: &str,
        chunk: FileChunk,
    ) -> RetrievalStatus {
        // Verify chunk integrity
        let computed = Self::content_hash(&chunk.data);
        if computed != chunk.chunk_hash {
            tracing::warn!(
                "[MESH FS] ❌ Chunk integrity failure for file {}...",
                &file_hash[..16]
            );
            return RetrievalStatus::NotFound;
        }

        // Store the chunk
        self.local_chunks.write().await
            .insert(chunk.chunk_hash.clone(), chunk.data.clone());

        // Track retrieval progress
        let mut retrievals = self.retrievals.write().await;
        if let Some(received) = retrievals.get_mut(file_hash) {
            received.insert(chunk.index, chunk.data);

            let manifest = self.manifests.read().await;
            if let Some(m) = manifest.get(file_hash) {
                if received.len() == m.chunk_count {
                    // All chunks received — reassemble
                    let mut file_data = Vec::with_capacity(m.total_size as usize);
                    for i in 0..m.chunk_count {
                        if let Some(chunk_data) = received.get(&i) {
                            file_data.extend_from_slice(chunk_data);
                        } else {
                            return RetrievalStatus::Partial {
                                received: received.len(),
                                total: m.chunk_count,
                            };
                        }
                    }

                    // Verify complete file integrity
                    let file_hash_computed = Self::content_hash(&file_data);
                    if file_hash_computed != file_hash {
                        tracing::error!(
                            "[MESH FS] ❌ File integrity failure: expected {}, got {}",
                            &file_hash[..16], &file_hash_computed[..16]
                        );
                        return RetrievalStatus::NotFound;
                    }

                    tracing::info!(
                        "[MESH FS] ✅ File {}... complete ({} bytes, {} chunks)",
                        &file_hash[..16], file_data.len(), m.chunk_count
                    );

                    // Clean up retrieval tracking
                    drop(manifest);
                    retrievals.remove(file_hash);

                    return RetrievalStatus::Complete(file_data);
                } else {
                    return RetrievalStatus::Partial {
                        received: received.len(),
                        total: m.chunk_count,
                    };
                }
            }
        }

        RetrievalStatus::NotFound
    }

    /// Get the list of chunk hashes needed for a file (those not stored locally).
    pub async fn missing_chunks(&self, file_hash: &str) -> Vec<String> {
        let manifests = self.manifests.read().await;
        let chunks = self.local_chunks.read().await;

        if let Some(manifest) = manifests.get(file_hash) {
            manifest.chunk_hashes.iter()
                .filter(|h| !chunks.contains_key(*h))
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Check if a chunk is stored locally.
    pub async fn has_chunk(&self, chunk_hash: &str) -> bool {
        self.local_chunks.read().await.contains_key(chunk_hash)
    }

    /// Store a raw chunk by its hash (used when receiving chunks from other peers).
    pub async fn store_raw_chunk(&self, chunk_hash: String, data: Vec<u8>) {
        self.local_chunks.write().await.insert(chunk_hash, data);
    }

    /// Get a locally stored chunk.
    pub async fn get_chunk(&self, chunk_hash: &str) -> Option<Vec<u8>> {
        self.local_chunks.read().await.get(chunk_hash).cloned()
    }

    /// Get stats.
    pub async fn stats(&self) -> serde_json::Value {
        let manifests = self.manifests.read().await;
        let chunks = self.local_chunks.read().await;
        let retrievals = self.retrievals.read().await;
        let total_bytes: usize = chunks.values().map(|c| c.len()).sum();

        serde_json::json!({
            "files_shared": manifests.len(),
            "chunks_stored": chunks.len(),
            "total_stored_mb": total_bytes as f64 / (1024.0 * 1024.0),
            "active_retrievals": retrievals.len(),
        })
    }

    // ─── Internal ───────────────────────────────────────────────────

    fn content_hash(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_share_small_file() {
        let fs = MeshFS::new(PeerId("local".to_string()));
        let data = b"Hello, mesh world!";
        let (manifest, chunks) = fs.share_file(data, vec![]).await;

        assert_eq!(manifest.total_size, data.len() as u64);
        assert_eq!(chunks.len(), 1); // Small file = 1 chunk
        assert_eq!(manifest.chunk_count, 1);
    }

    #[tokio::test]
    async fn test_share_large_file_chunking() {
        let fs = MeshFS::new(PeerId("local".to_string()));
        // 1MB file = 4 chunks of 256KB
        let data = vec![42u8; 1024 * 1024];
        let (manifest, chunks) = fs.share_file(&data, vec![]).await;

        assert_eq!(chunks.len(), 4);
        assert_eq!(manifest.chunk_count, 4);
        assert_eq!(manifest.total_size, 1024 * 1024);
    }

    #[tokio::test]
    async fn test_full_retrieval_flow() {
        let sender = MeshFS::new(PeerId("sender".to_string()));
        let receiver = MeshFS::new(PeerId("receiver".to_string()));

        // Sender shares a file
        let data = b"Complete file for retrieval test";
        let (manifest, chunks) = sender.share_file(data, vec![]).await;

        // Receiver gets the manifest
        let file_hash = manifest.file_hash.clone();
        receiver.receive_manifest(manifest).await;

        // Receiver receives all chunks
        for chunk in chunks {
            let status = receiver.receive_chunk(&file_hash, chunk).await;
            if let RetrievalStatus::Complete(retrieved_data) = status {
                assert_eq!(retrieved_data, data.to_vec());
                return;
            }
        }

        panic!("Should have gotten Complete status");
    }

    #[tokio::test]
    async fn test_partial_retrieval() {
        let sender = MeshFS::new(PeerId("sender".to_string()));
        let receiver = MeshFS::new(PeerId("receiver".to_string()));

        // Large file — multiple chunks
        let data = vec![99u8; CHUNK_SIZE * 3]; // 3 chunks
        let (manifest, chunks) = sender.share_file(&data, vec![]).await;

        let file_hash = manifest.file_hash.clone();
        receiver.receive_manifest(manifest).await;

        // Send only the first chunk
        let status = receiver.receive_chunk(&file_hash, chunks[0].clone()).await;
        match status {
            RetrievalStatus::Partial { received, total } => {
                assert_eq!(received, 1);
                assert_eq!(total, 3);
            }
            other => panic!("Expected Partial, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_chunk_integrity_check() {
        let receiver = MeshFS::new(PeerId("receiver".to_string()));

        let manifest = FileManifest {
            file_hash: "fake_hash".to_string(),
            chunk_hashes: vec!["real_hash".to_string()],
            total_size: 10,
            chunk_count: 1,
            encrypted_meta: vec![],
            origin: PeerId("sender".to_string()),
            shared_at: chrono::Utc::now().to_rfc3339(),
        };
        receiver.receive_manifest(manifest).await;

        // Tampered chunk — wrong hash
        let bad_chunk = FileChunk {
            chunk_hash: "wrong_hash_doesnt_match".to_string(),
            data: b"tampered data".to_vec(),
            index: 0,
            total: 1,
        };

        let status = receiver.receive_chunk("fake_hash", bad_chunk).await;
        assert_eq!(status, RetrievalStatus::NotFound);
    }

    #[tokio::test]
    async fn test_missing_chunks() {
        let fs = MeshFS::new(PeerId("local".to_string()));
        let data = vec![77u8; CHUNK_SIZE * 2]; // 2 chunks
        let (manifest, _chunks) = fs.share_file(&data, vec![]).await;

        // All chunks stored locally — none missing
        let missing = fs.missing_chunks(&manifest.file_hash).await;
        assert_eq!(missing.len(), 0);
    }

    #[tokio::test]
    async fn test_stats() {
        let fs = MeshFS::new(PeerId("local".to_string()));
        fs.share_file(b"test data", vec![]).await;

        let stats = fs.stats().await;
        assert_eq!(stats["files_shared"], 1);
        assert_eq!(stats["chunks_stored"], 1);
    }
}
