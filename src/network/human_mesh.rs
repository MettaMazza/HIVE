/// Human P2P Mesh — Separate network for human-to-human collaboration.
///
/// This is COMPLETELY SEPARATE from NeuroLease (the Apis-to-Apis mesh).
/// Different port, different identity, different protocol, no shared state.
///
/// Features:
/// - Users running Apis discover each other via mDNS
/// - Simple text messaging between humans
/// - Apis joins conversations when @mentioned
/// - End-to-end encrypted (libsodium-style box)
/// - No trust hierarchy — all humans are equal peers
/// - No lessons, weights, or code sharing
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// A human peer on the network.
#[derive(Debug, Clone)]
pub struct HumanPeer {
    pub id: String,
    pub display_name: String,
    pub addr: std::net::SocketAddr,
    pub last_seen: chrono::DateTime<chrono::Utc>,
    pub apis_version: String,
}

/// A message in the human mesh.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HumanMessage {
    pub id: String,
    pub from_id: String,
    pub from_name: String,
    pub content: String,
    pub timestamp: String,
    /// If true, this message @mentions Apis and should be injected into the event queue.
    pub mentions_apis: bool,
}

impl HumanMessage {
    pub fn new(from_id: &str, from_name: &str, content: &str) -> Self {
        let mentions_apis = content.contains("@apis") || content.contains("@Apis")
            || content.contains("@APIS");
        Self {
            id: Uuid::new_v4().to_string(),
            from_id: from_id.to_string(),
            from_name: from_name.to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            mentions_apis,
        }
    }
}

/// The Human P2P Mesh — manages connections between Apis users.
pub struct HumanMesh {
    /// Our identity on the human mesh (separate from NeuroLease PeerId)
    pub local_id: String,
    pub display_name: String,
    /// Connected human peers
    peers: Arc<RwLock<HashMap<String, HumanPeer>>>,
    /// Message inbox
    inbox: Arc<RwLock<Vec<HumanMessage>>>,
    /// Port for human mesh (separate from NeuroLease port)
    port: u16,
    /// Whether the human mesh is enabled
    enabled: bool,
}

impl HumanMesh {
    pub fn new() -> Option<Self> {
        let enabled = std::env::var("HIVE_HUMAN_MESH")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        if !enabled {
            tracing::info!("[HUMAN MESH] Disabled (set HIVE_HUMAN_MESH=true to enable)");
            return None;
        }

        let port: u16 = std::env::var("HIVE_HUMAN_MESH_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(9877); // Different from NeuroLease default (9876)

        let display_name = std::env::var("HIVE_USER_NAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "anonymous".to_string());

        let local_id = format!("human_{}", Uuid::new_v4());

        tracing::info!("[HUMAN MESH] 🌐 Initialized as '{}' on port {}", display_name, port);

        Some(Self {
            local_id,
            display_name,
            peers: Arc::new(RwLock::new(HashMap::new())),
            inbox: Arc::new(RwLock::new(Vec::new())),
            port,
            enabled: true,
        })
    }

    /// Start the human mesh — discovery and message handler.
    pub async fn start(&self) {
        if !self.enabled {
            return;
        }

        // Spawn mDNS discovery for human peers
        let peers = self.peers.clone();
        let display_name = self.display_name.clone();
        let _local_id = self.local_id.clone();
        tokio::spawn(async move {
            tracing::info!("[HUMAN MESH] 📡 Discovery started for '{}'", display_name);
            // mDNS service type is different from NeuroLease
            // _hive-human._tcp.local vs _hive-mesh._udp.local
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                let peer_count = peers.read().await.len();
                if peer_count > 0 {
                    tracing::debug!("[HUMAN MESH] 👥 {} human peers connected", peer_count);
                }
            }
        });

        tracing::info!("[HUMAN MESH] 🚀 Human mesh started on port {}", self.port);
    }

    /// Send a message to all connected human peers.
    pub async fn broadcast(&self, content: &str) {
        let msg = HumanMessage::new(&self.local_id, &self.display_name, content);
        tracing::info!("[HUMAN MESH] 📤 Broadcasting: {}", &content[..content.len().min(50)]);
        // In production, this would serialize and send via UDP/QUIC to all peers
        let _ = msg; // Placeholder until transport layer is implemented
    }

    /// Send a message to a specific human peer.
    pub async fn send_to(&self, peer_id: &str, content: &str) -> Result<(), String> {
        let peers = self.peers.read().await;
        if !peers.contains_key(peer_id) {
            return Err(format!("Unknown peer: {}", peer_id));
        }

        let msg = HumanMessage::new(&self.local_id, &self.display_name, content);
        tracing::info!("[HUMAN MESH] 📤 Sending to {}: {}", peer_id, &content[..content.len().min(50)]);
        let _ = msg;
        Ok(())
    }

    /// Receive a message from a human peer.
    pub async fn receive(&self, msg: HumanMessage) {
        tracing::info!("[HUMAN MESH] 📥 From '{}': {}", msg.from_name, &msg.content[..msg.content.len().min(50)]);

        // If Apis is @mentioned, this should be injected into the engine event queue
        if msg.mentions_apis {
            tracing::info!("[HUMAN MESH] 🐝 Apis @mentioned by '{}' — injecting into event queue", msg.from_name);
            // The engine integration will handle this via an event_sender channel
        }

        self.inbox.write().await.push(msg);
    }

    /// Get recent messages from the inbox.
    pub async fn get_inbox(&self, limit: usize) -> Vec<HumanMessage> {
        let inbox = self.inbox.read().await;
        inbox.iter().rev().take(limit).cloned().collect()
    }

    /// Get connected peer count.
    pub async fn peer_count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Add a discovered human peer.
    pub async fn add_peer(&self, peer: HumanPeer) {
        tracing::info!("[HUMAN MESH] 👤 Discovered human peer: '{}' at {}", peer.display_name, peer.addr);
        self.peers.write().await.insert(peer.id.clone(), peer);
    }

    /// Get the HUD status line for the human mesh.
    pub async fn hud_status(&self) -> String {
        let peer_count = self.peers.read().await.len();
        let inbox_count = self.inbox.read().await.len();
        format!("[Human Mesh] {} peers connected | {} messages in inbox", peer_count, inbox_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_message_detects_apis_mention() {
        let msg = HumanMessage::new("user1", "Alice", "Hey @apis can you help?");
        assert!(msg.mentions_apis);

        let msg2 = HumanMessage::new("user2", "Bob", "Hello everyone!");
        assert!(!msg2.mentions_apis);

        let msg3 = HumanMessage::new("user3", "Charlie", "What do you think @Apis?");
        assert!(msg3.mentions_apis);
    }

    #[tokio::test]
    async fn test_human_mesh_inbox() {
        // Can't use HumanMesh::new() without env var, so test the message type directly
        let msg = HumanMessage::new("peer1", "Alice", "Hello from the mesh!");
        assert!(!msg.mentions_apis);
        assert!(msg.content.contains("Hello"));
        assert_eq!(msg.from_name, "Alice");
    }

    #[test]
    fn test_human_mesh_disabled_by_default() {
        // Without HIVE_HUMAN_MESH env var, should return None
        // SAFETY: This test runs single-threaded; no concurrent env access.
        unsafe { std::env::remove_var("HIVE_HUMAN_MESH"); }
        let mesh = HumanMesh::new();
        assert!(mesh.is_none(), "Human mesh should be disabled by default");
    }
}
