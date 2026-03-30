/// Apis-to-Apis Chat — P2P messaging between AI instances across the mesh.
///
/// Manages chat history, Discord bridge forwarding, and engine event injection
/// so Apis can read and respond to other Apis instances on the network.
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::network::messages::PeerId;

/// A chat message between Apis instances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApisChatMessage {
    pub id: String,
    pub from_peer: PeerId,
    pub from_name: String,
    pub content: String,
    pub reply_to: Option<String>,
    pub timestamp: String,
    pub channel: Option<String>,
}

/// Manages Apis-to-Apis chat across the mesh.
pub struct ApisChat {
    /// Chat history (ring buffer)
    history: Arc<RwLock<VecDeque<ApisChatMessage>>>,
    /// Max messages retained
    max_history: usize,
    /// Discord channel ID to bridge mesh chat into (if configured)
    pub discord_bridge_channel: Option<String>,
    /// Display name for this instance on the mesh
    pub mesh_name: String,
    /// Whether this instance listens to mesh chat
    pub listen_enabled: bool,
}

impl ApisChat {
    /// Initialize from environment variables.
    pub fn from_env() -> Self {
        let listen_enabled = std::env::var("HIVE_MESH_CHAT_LISTEN")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let discord_bridge_channel = std::env::var("HIVE_MESH_CHAT_DISCORD_CHANNEL")
            .ok()
            .filter(|s| !s.is_empty());

        let mesh_name = std::env::var("HIVE_MESH_CHAT_NAME")
            .unwrap_or_else(|_| "Apis".to_string());

        if listen_enabled {
            tracing::info!("[APIS CHAT] 🐝 Mesh chat enabled as '{}'{}", mesh_name,
                discord_bridge_channel.as_ref().map(|c| format!(" → Discord #{}", c)).unwrap_or_default());
        } else {
            tracing::info!("[APIS CHAT] Mesh chat listening disabled (set HIVE_MESH_CHAT_LISTEN=true to enable)");
        }

        Self {
            history: Arc::new(RwLock::new(VecDeque::with_capacity(500))),
            max_history: 500,
            discord_bridge_channel,
            mesh_name,
            listen_enabled,
        }
    }

    /// Handle an incoming chat message from another Apis instance.
    pub async fn handle_incoming(&self, msg: ApisChatMessage) {
        tracing::info!("[APIS CHAT] 📥 From '{}': {}", msg.from_name, 
            &msg.content[..msg.content.len().min(100)]);

        // Store in history
        let mut history = self.history.write().await;
        if history.len() >= self.max_history {
            history.pop_front();
        }
        history.push_back(msg);
    }

    /// Create a chat message from this instance.
    pub fn create_message(&self, content: &str, peer_id: &PeerId) -> ApisChatMessage {
        ApisChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from_peer: peer_id.clone(),
            from_name: self.mesh_name.clone(),
            content: content.to_string(),
            reply_to: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            channel: None,
        }
    }

    /// Get recent chat history.
    pub async fn recent_history(&self, limit: usize) -> Vec<ApisChatMessage> {
        let history = self.history.read().await;
        history.iter().rev().take(limit).cloned().collect()
    }

    /// Get the message count.
    pub async fn message_count(&self) -> usize {
        self.history.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_msg(name: &str, content: &str) -> ApisChatMessage {
        ApisChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from_peer: PeerId(format!("peer_{}", name)),
            from_name: name.to_string(),
            content: content.to_string(),
            reply_to: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            channel: None,
        }
    }

    #[tokio::test]
    async fn test_chat_incoming() {
        let chat = ApisChat {
            history: Arc::new(RwLock::new(VecDeque::new())),
            max_history: 500,
            discord_bridge_channel: None,
            mesh_name: "TestApis".to_string(),
            listen_enabled: true,
        };

        chat.handle_incoming(test_msg("RemoteApis", "Hello from the mesh!")).await;
        assert_eq!(chat.message_count().await, 1);

        let recent = chat.recent_history(10).await;
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].from_name, "RemoteApis");
    }

    #[tokio::test]
    async fn test_chat_ring_buffer() {
        let chat = ApisChat {
            history: Arc::new(RwLock::new(VecDeque::new())),
            max_history: 3,
            discord_bridge_channel: None,
            mesh_name: "TestApis".to_string(),
            listen_enabled: true,
        };

        for i in 0..5 {
            chat.handle_incoming(test_msg(&format!("Peer{}", i), &format!("Message {}", i))).await;
        }

        // Only last 3 should remain
        assert_eq!(chat.message_count().await, 3);
        let recent = chat.recent_history(10).await;
        assert_eq!(recent[0].content, "Message 4"); // Most recent first
    }

    #[test]
    fn test_create_message() {
        let chat = ApisChat {
            history: Arc::new(RwLock::new(VecDeque::new())),
            max_history: 500,
            discord_bridge_channel: None,
            mesh_name: "Apis".to_string(),
            listen_enabled: true,
        };

        let peer_id = PeerId("local_peer_id".to_string());
        let msg = chat.create_message("Hello mesh!", &peer_id);
        assert_eq!(msg.from_name, "Apis");
        assert_eq!(msg.content, "Hello mesh!");
        assert_eq!(msg.from_peer.0, "local_peer_id");
    }
}
