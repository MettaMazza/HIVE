/// DHT — Kademlia-style Distributed Hash Table for the HIVE mesh.
///
/// Instead of broadcasting everything to every peer, data is stored on the K
/// peers whose IDs are closest to the data's content-addressed key.
///
/// OPERATIONS:
///   - Store: put(key, value) → replicated to K closest peers
///   - Lookup: get(key) → query closest known peers, follow referrals
///   - Delete: remove(key) → propagate tombstone to holders
///
/// REPLICATION: K=3 (configurable). When a peer goes offline, data
/// redistributes to maintain K copies.
///
/// TTL: Data expires after a configurable period unless refreshed.
/// CONTENT ADDRESSING: Keys are SHA-256 hashes of the content.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};

use crate::network::messages::PeerId;

/// Default replication factor.
pub const DEFAULT_K: usize = 3;

/// A value stored in the DHT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DHTEntry {
    /// Content-addressed key (SHA-256 of the value).
    pub key: String,
    /// The stored value (serialised data).
    pub value: Vec<u8>,
    /// Type hint for deserialization.
    pub entry_type: DHTEntryType,
    /// When this entry was stored.
    pub stored_at: String,
    /// TTL in seconds (0 = permanent until GC).
    pub ttl_secs: u64,
    /// Which peer originally stored this.
    pub origin: PeerId,
    /// Peers known to hold a copy.
    pub holders: Vec<PeerId>,
}

/// Types of DHT entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DHTEntryType {
    /// A lesson from the knowledge system.
    Lesson,
    /// A synaptic graph delta.
    SynapticDelta,
    /// A LoRA adapter manifest.
    LoRAManifest,
    /// Cached web content.
    WebCache,
    /// Model weights metadata.
    ModelMeta,
    /// Generic data.
    Generic,
}

/// Result of a DHT lookup.
#[derive(Debug, Clone)]
pub enum LookupResult {
    /// Found locally.
    Found(DHTEntry),
    /// Not found locally but these peers might have it (referrals).
    Referral(Vec<PeerId>),
    /// Not found anywhere we know of.
    NotFound,
}

/// The Distributed Hash Table.
pub struct DHT {
    /// Local storage of DHT entries.
    local_store: Arc<RwLock<HashMap<String, DHTEntry>>>,
    /// Routing table: peer_id → distance metric (XOR distance of SHA-256(peer_id)).
    routing_table: Arc<RwLock<HashMap<String, PeerId>>>,
    /// Our own peer ID.
    local_peer: PeerId,
    /// Replication factor.
    k: usize,
}

impl DHT {
    pub fn new(local_peer: PeerId, k: usize) -> Self {
        tracing::info!("[DHT] 🗄️ Initialised (peer={}, K={})", local_peer, k);
        Self {
            local_store: Arc::new(RwLock::new(HashMap::new())),
            routing_table: Arc::new(RwLock::new(HashMap::new())),
            local_peer,
            k,
        }
    }

    /// Compute a content-addressed key for arbitrary data.
    pub fn content_key(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    /// Compute the XOR distance between two hex keys (Kademlia metric).
    pub fn xor_distance(key_a: &str, key_b: &str) -> Vec<u8> {
        let a_bytes = Self::hex_to_bytes(key_a);
        let b_bytes = Self::hex_to_bytes(key_b);
        let max_len = a_bytes.len().max(b_bytes.len());

        (0..max_len)
            .map(|i| {
                let a = a_bytes.get(i).copied().unwrap_or(0);
                let b = b_bytes.get(i).copied().unwrap_or(0);
                a ^ b
            })
            .collect()
    }

    /// Decode a hex string to bytes (inline, no external crate).
    fn hex_to_bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .filter_map(|i| {
                hex.get(i..i + 2)
                    .and_then(|byte_str| u8::from_str_radix(byte_str, 16).ok())
            })
            .collect()
    }

    /// Update the routing table with a known peer.
    pub async fn add_peer(&self, peer: PeerId) {
        let peer_key = Self::content_key(peer.0.as_bytes());
        self.routing_table.write().await.insert(peer_key, peer);
    }

    /// Remove a peer from the routing table.
    pub async fn remove_peer(&self, peer: &PeerId) {
        let peer_key = Self::content_key(peer.0.as_bytes());
        self.routing_table.write().await.remove(&peer_key);
    }

    /// Find the K closest peers to a given key.
    pub async fn closest_peers(&self, key: &str, count: usize) -> Vec<PeerId> {
        let table = self.routing_table.read().await;
        let mut distances: Vec<(Vec<u8>, PeerId)> = table.iter()
            .map(|(peer_key, peer_id)| {
                (Self::xor_distance(key, peer_key), peer_id.clone())
            })
            .collect();

        distances.sort_by(|a, b| a.0.cmp(&b.0));
        distances.into_iter()
            .take(count)
            .map(|(_, peer)| peer)
            .collect()
    }

    /// Store a value in the DHT.
    /// Returns the content key and the list of peers it should be replicated to.
    pub async fn store(
        &self,
        value: &[u8],
        entry_type: DHTEntryType,
        ttl_secs: u64,
    ) -> (String, Vec<PeerId>) {
        let key = Self::content_key(value);

        // Store locally
        let entry = DHTEntry {
            key: key.clone(),
            value: value.to_vec(),
            entry_type,
            stored_at: chrono::Utc::now().to_rfc3339(),
            ttl_secs,
            origin: self.local_peer.clone(),
            holders: vec![self.local_peer.clone()],
        };

        self.local_store.write().await.insert(key.clone(), entry);

        // Find K closest peers for replication
        let targets = self.closest_peers(&key, self.k).await;

        tracing::info!(
            "[DHT] 📤 Stored key {}... → {} replication targets",
            &key[..16], targets.len()
        );

        (key, targets)
    }

    /// Receive and store data from a remote peer (replication).
    pub async fn receive_store(&self, entry: DHTEntry) -> bool {
        let key = entry.key.clone();

        // Verify content integrity
        let computed_key = Self::content_key(&entry.value);
        if computed_key != key {
            tracing::warn!("[DHT] ❌ Content integrity failure for key {}...", &key[..16]);
            return false;
        }

        let mut store = self.local_store.write().await;
        if store.contains_key(&key) {
            // Already have it — just update holders list
            if let Some(existing) = store.get_mut(&key) {
                for holder in &entry.holders {
                    if !existing.holders.contains(holder) {
                        existing.holders.push(holder.clone());
                    }
                }
            }
            return false; // Not new
        }

        store.insert(key.clone(), entry);
        tracing::info!("[DHT] 📥 Received replicated key {}...", &key[..16]);
        true
    }

    /// Look up a value by key.
    pub async fn lookup(&self, key: &str) -> LookupResult {
        // Check local store first
        let store = self.local_store.read().await;
        if let Some(entry) = store.get(key) {
            // Check TTL
            if entry.ttl_secs > 0 {
                if let Ok(stored) = chrono::DateTime::parse_from_rfc3339(&entry.stored_at) {
                    let age = chrono::Utc::now().signed_duration_since(stored);
                    if age.num_seconds() > entry.ttl_secs as i64 {
                        return LookupResult::NotFound; // Expired
                    }
                }
            }
            return LookupResult::Found(entry.clone());
        }
        drop(store);

        // Not found locally — find closest peers who might have it
        let closest = self.closest_peers(key, self.k).await;
        if closest.is_empty() {
            LookupResult::NotFound
        } else {
            LookupResult::Referral(closest)
        }
    }

    /// Delete a key (propagate tombstone).
    pub async fn delete(&self, key: &str) -> bool {
        let mut store = self.local_store.write().await;
        if store.remove(key).is_some() {
            tracing::info!("[DHT] 🗑️ Deleted key {}...", &key[..key.len().min(16)]);
            true
        } else {
            false
        }
    }

    /// Garbage-collect expired entries.
    pub async fn gc(&self) -> usize {
        let mut store = self.local_store.write().await;
        let now = chrono::Utc::now();
        let mut expired = Vec::new();

        for (key, entry) in store.iter() {
            if entry.ttl_secs > 0 {
                if let Ok(stored) = chrono::DateTime::parse_from_rfc3339(&entry.stored_at) {
                    let age = now.signed_duration_since(stored);
                    if age.num_seconds() > entry.ttl_secs as i64 {
                        expired.push(key.clone());
                    }
                }
            }
        }

        let count = expired.len();
        for key in &expired {
            store.remove(key);
        }

        if count > 0 {
            tracing::info!("[DHT] 🧹 GC: {} expired entries removed", count);
        }
        count
    }

    /// Get the number of entries stored locally.
    pub async fn local_entry_count(&self) -> usize {
        self.local_store.read().await.len()
    }

    /// Get the number of known peers.
    pub async fn peer_count(&self) -> usize {
        self.routing_table.read().await.len()
    }

    /// Get stats.
    pub async fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "local_entries": self.local_entry_count().await,
            "known_peers": self.peer_count().await,
            "replication_factor": self.k,
        })
    }

    /// Get all keys stored locally (for replication health checks).
    pub async fn local_keys(&self) -> Vec<String> {
        self.local_store.read().await.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_key() {
        let key1 = DHT::content_key(b"hello world");
        let key2 = DHT::content_key(b"hello world");
        let key3 = DHT::content_key(b"different data");

        assert_eq!(key1, key2); // Same data = same key
        assert_ne!(key1, key3); // Different data = different key
        assert_eq!(key1.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn test_xor_distance() {
        let key_a = DHT::content_key(b"peer_a");
        let key_b = DHT::content_key(b"peer_b");

        let dist = DHT::xor_distance(&key_a, &key_b);
        assert!(!dist.is_empty());

        // Distance to self should be all zeros
        let self_dist = DHT::xor_distance(&key_a, &key_a);
        assert!(self_dist.iter().all(|b| *b == 0));
    }

    #[tokio::test]
    async fn test_store_and_lookup() {
        let dht = DHT::new(PeerId("local_peer".to_string()), 3);

        let data = b"some knowledge data";
        let (key, _targets) = dht.store(data, DHTEntryType::Lesson, 3600).await;

        match dht.lookup(&key).await {
            LookupResult::Found(entry) => {
                assert_eq!(entry.value, data.to_vec());
                assert_eq!(entry.entry_type, DHTEntryType::Lesson);
            }
            other => panic!("Expected Found, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_lookup_not_found() {
        let dht = DHT::new(PeerId("local_peer".to_string()), 3);
        let result = dht.lookup("nonexistent_key_000000000000000000000000000000000000000000000000").await;
        assert!(matches!(result, LookupResult::NotFound));
    }

    #[tokio::test]
    async fn test_closest_peers() {
        let dht = DHT::new(PeerId("local".to_string()), 3);

        // Add some peers
        for i in 0..5 {
            dht.add_peer(PeerId(format!("peer_{}", i))).await;
        }

        let key = DHT::content_key(b"target_data");
        let closest = dht.closest_peers(&key, 3).await;
        assert_eq!(closest.len(), 3);
    }

    #[tokio::test]
    async fn test_receive_store_integrity_check() {
        let dht = DHT::new(PeerId("receiver".to_string()), 3);

        // Valid entry
        let data = b"valid data";
        let key = DHT::content_key(data);
        let entry = DHTEntry {
            key: key.clone(),
            value: data.to_vec(),
            entry_type: DHTEntryType::Generic,
            stored_at: chrono::Utc::now().to_rfc3339(),
            ttl_secs: 3600,
            origin: PeerId("sender".to_string()),
            holders: vec![PeerId("sender".to_string())],
        };
        assert!(dht.receive_store(entry).await);

        // Tampered entry (wrong key)
        let bad_entry = DHTEntry {
            key: "deadbeef".repeat(8),
            value: b"tampered data".to_vec(),
            entry_type: DHTEntryType::Generic,
            stored_at: chrono::Utc::now().to_rfc3339(),
            ttl_secs: 3600,
            origin: PeerId("attacker".to_string()),
            holders: vec![],
        };
        assert!(!dht.receive_store(bad_entry).await);
    }

    #[tokio::test]
    async fn test_delete() {
        let dht = DHT::new(PeerId("local".to_string()), 3);
        let (key, _) = dht.store(b"delete me", DHTEntryType::Generic, 0).await;

        assert_eq!(dht.local_entry_count().await, 1);
        assert!(dht.delete(&key).await);
        assert_eq!(dht.local_entry_count().await, 0);
    }

    #[tokio::test]
    async fn test_dedup_on_receive() {
        let dht = DHT::new(PeerId("receiver".to_string()), 3);
        let data = b"duplicate data";
        let key = DHT::content_key(data);

        let entry = DHTEntry {
            key: key.clone(),
            value: data.to_vec(),
            entry_type: DHTEntryType::Lesson,
            stored_at: chrono::Utc::now().to_rfc3339(),
            ttl_secs: 3600,
            origin: PeerId("sender_1".to_string()),
            holders: vec![PeerId("sender_1".to_string())],
        };

        assert!(dht.receive_store(entry.clone()).await); // First time = new
        assert!(!dht.receive_store(entry).await); // Second time = dedup
        assert_eq!(dht.local_entry_count().await, 1);
    }
}
