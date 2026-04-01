/// WebSocket Hub — Bidirectional real-time communication for the HIVE mesh.
///
/// Replaces SSE with full-duplex WebSocket connections. Enables:
/// - Typing indicators (bidirectional)
/// - Message edits/deletes broadcast
/// - Presence (online/offline/idle)
/// - Unread tracking
/// - Future: voice channel signalling
///
/// All platforms can use this hub for real-time events.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A real-time event sent over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsEvent {
    /// Event type: "message", "edit", "delete", "typing", "presence", "reaction", etc.
    #[serde(rename = "type")]
    pub event_type: String,
    /// The event payload (varies by type).
    #[serde(flatten)]
    pub data: Value,
}

/// Tracks typing state for a channel.
#[derive(Debug, Clone)]
struct TypingState {
    /// peer_id -> when they started typing
    typers: HashMap<String, std::time::Instant>,
}

/// Channel presence — who is connected to each channel.
#[derive(Debug, Clone, Default)]
struct ChannelPresence {
    /// peer_ids currently viewing this channel
    viewers: HashSet<String>,
}

/// The WebSocket Hub — manages connections, broadcasts, and state.
pub struct WsHub {
    /// Broadcast sender for all events (subscribers filter by channel_id).
    tx: broadcast::Sender<WsEvent>,
    /// Typing state per channel.
    typing: RwLock<HashMap<String, TypingState>>,
    /// Presence per channel.
    presence: RwLock<HashMap<String, ChannelPresence>>,
    /// Unread counts: peer_id -> (channel_id -> count).
    unread: RwLock<HashMap<String, HashMap<String, usize>>>,
    /// Connected peers (for presence tracking).
    connected_peers: RwLock<HashSet<String>>,
}

impl WsHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            tx,
            typing: RwLock::new(HashMap::new()),
            presence: RwLock::new(HashMap::new()),
            unread: RwLock::new(HashMap::new()),
            connected_peers: RwLock::new(HashSet::new()),
        }
    }

    // ─── Broadcasting ─────────────────────────────────────────────────

    /// Broadcast an event to all connected clients.
    pub fn broadcast(&self, event: WsEvent) {
        let _ = self.tx.send(event);
    }

    /// Subscribe to the event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<WsEvent> {
        self.tx.subscribe()
    }

    /// Broadcast a new message event.
    pub fn broadcast_message(&self, channel_id: &str, message: &Value) {
        self.broadcast(WsEvent {
            event_type: "message".to_string(),
            data: json!({
                "channel_id": channel_id,
                "message": message,
            }),
        });
    }

    /// Broadcast a message edit event.
    pub fn broadcast_edit(&self, channel_id: &str, msg_id: &str, new_content: &str) {
        self.broadcast(WsEvent {
            event_type: "edit".to_string(),
            data: json!({
                "channel_id": channel_id,
                "message_id": msg_id,
                "content": new_content,
            }),
        });
    }

    /// Broadcast a message delete event.
    pub fn broadcast_delete(&self, channel_id: &str, msg_id: &str) {
        self.broadcast(WsEvent {
            event_type: "delete".to_string(),
            data: json!({
                "channel_id": channel_id,
                "message_id": msg_id,
            }),
        });
    }

    /// Broadcast a reaction event.
    pub fn broadcast_reaction(&self, channel_id: &str, msg_id: &str, emoji: &str, peer_id: &str) {
        self.broadcast(WsEvent {
            event_type: "reaction".to_string(),
            data: json!({
                "channel_id": channel_id,
                "message_id": msg_id,
                "emoji": emoji,
                "peer_id": peer_id,
            }),
        });
    }

    // ─── Typing Indicators ────────────────────────────────────────────

    /// Mark a user as typing in a channel.
    pub async fn set_typing(&self, channel_id: &str, peer_id: &str, display_name: &str) {
        {
            let mut typing = self.typing.write().await;
            let state = typing.entry(channel_id.to_string()).or_insert_with(|| TypingState {
                typers: HashMap::new(),
            });
            state.typers.insert(peer_id.to_string(), std::time::Instant::now());
        }

        // Broadcast typing event
        self.broadcast(WsEvent {
            event_type: "typing".to_string(),
            data: json!({
                "channel_id": channel_id,
                "peer_id": peer_id,
                "display_name": display_name,
            }),
        });
    }

    /// Clear typing status for a user (called when message is sent or timeout).
    pub async fn clear_typing(&self, channel_id: &str, peer_id: &str) {
        let mut typing = self.typing.write().await;
        if let Some(state) = typing.get_mut(channel_id) {
            state.typers.remove(peer_id);
        }
    }

    /// Get users currently typing in a channel (within last 5 seconds).
    pub async fn get_typing(&self, channel_id: &str) -> Vec<String> {
        let typing = self.typing.read().await;
        if let Some(state) = typing.get(channel_id) {
            let threshold = std::time::Instant::now() - std::time::Duration::from_secs(5);
            state.typers.iter()
                .filter(|(_, when)| **when > threshold)
                .map(|(id, _)| id.clone())
                .collect()
        } else {
            Vec::new()
        }
    }

    // ─── Presence ─────────────────────────────────────────────────────

    /// Register a peer as connected.
    pub async fn peer_connected(&self, peer_id: &str) {
        self.connected_peers.write().await.insert(peer_id.to_string());
        self.broadcast(WsEvent {
            event_type: "presence".to_string(),
            data: json!({
                "peer_id": peer_id,
                "status": "online",
            }),
        });
    }

    /// Register a peer as disconnected.
    pub async fn peer_disconnected(&self, peer_id: &str) {
        self.connected_peers.write().await.remove(peer_id);

        // Remove from all channel presences
        let mut presence = self.presence.write().await;
        for channel in presence.values_mut() {
            channel.viewers.remove(peer_id);
        }

        self.broadcast(WsEvent {
            event_type: "presence".to_string(),
            data: json!({
                "peer_id": peer_id,
                "status": "offline",
            }),
        });
    }

    /// Join a channel (track presence).
    pub async fn join_channel(&self, channel_id: &str, peer_id: &str) {
        let mut presence = self.presence.write().await;
        presence.entry(channel_id.to_string())
            .or_default()
            .viewers.insert(peer_id.to_string());
    }

    /// Leave a channel.
    pub async fn leave_channel(&self, channel_id: &str, peer_id: &str) {
        let mut presence = self.presence.write().await;
        if let Some(channel) = presence.get_mut(channel_id) {
            channel.viewers.remove(peer_id);
        }
    }

    /// Get connected peer count.
    pub async fn online_count(&self) -> usize {
        self.connected_peers.read().await.len()
    }

    // ─── Unread Tracking ──────────────────────────────────────────────

    /// Increment unread count for all peers except the sender.
    pub async fn increment_unread(&self, channel_id: &str, sender_peer_id: &str) {
        let mut unread = self.unread.write().await;
        let connected = self.connected_peers.read().await;

        for peer_id in connected.iter() {
            if peer_id != sender_peer_id {
                let peer_counts = unread.entry(peer_id.clone()).or_default();
                *peer_counts.entry(channel_id.to_string()).or_default() += 1;
            }
        }
    }

    /// Get unread counts for a peer.
    pub async fn get_unread(&self, peer_id: &str) -> HashMap<String, usize> {
        let unread = self.unread.read().await;
        unread.get(peer_id).cloned().unwrap_or_default()
    }

    /// Mark a channel as read for a peer.
    pub async fn mark_read(&self, channel_id: &str, peer_id: &str) {
        let mut unread = self.unread.write().await;
        if let Some(peer_counts) = unread.get_mut(peer_id) {
            peer_counts.remove(channel_id);
        }
    }

    /// Clear all unread for a peer.
    pub async fn clear_unread(&self, peer_id: &str) {
        let mut unread = self.unread.write().await;
        unread.remove(peer_id);
    }

    // ─── Typing Cleanup Daemon ────────────────────────────────────────

    /// Spawn a background task that cleans up stale typing indicators.
    pub fn spawn_typing_cleanup(self: &Arc<Self>) {
        let hub = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                let mut typing = hub.typing.write().await;
                let threshold = std::time::Instant::now() - std::time::Duration::from_secs(5);
                for state in typing.values_mut() {
                    state.typers.retain(|_, when| *when > threshold);
                }
                // Remove empty channels
                typing.retain(|_, state| !state.typers.is_empty());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ws_hub_broadcast() {
        let hub = WsHub::new();
        let mut rx = hub.subscribe();

        hub.broadcast_message("ch1", &json!({"content": "hello"}));

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type, "message");
    }

    #[tokio::test]
    async fn test_ws_hub_typing() {
        let hub = WsHub::new();

        hub.set_typing("ch1", "peer1", "Alice").await;

        let typers = hub.get_typing("ch1").await;
        assert_eq!(typers.len(), 1);
        assert_eq!(typers[0], "peer1");

        // Empty channel
        let empty = hub.get_typing("ch2").await;
        assert!(empty.is_empty());

        // Clear
        hub.clear_typing("ch1", "peer1").await;
        let after = hub.get_typing("ch1").await;
        assert!(after.is_empty());
    }

    #[tokio::test]
    async fn test_ws_hub_presence() {
        let hub = WsHub::new();

        hub.peer_connected("peer1").await;
        hub.peer_connected("peer2").await;
        assert_eq!(hub.online_count().await, 2);

        hub.peer_disconnected("peer1").await;
        assert_eq!(hub.online_count().await, 1);
    }

    #[tokio::test]
    async fn test_ws_hub_unread() {
        let hub = WsHub::new();

        // Register two peers
        hub.peer_connected("alice").await;
        hub.peer_connected("bob").await;

        // Alice sends a message in ch1
        hub.increment_unread("ch1", "alice").await;

        // Bob should have 1 unread in ch1
        let bob_unread = hub.get_unread("bob").await;
        assert_eq!(*bob_unread.get("ch1").unwrap_or(&0), 1);

        // Alice should have 0 unread (she sent the message)
        let alice_unread = hub.get_unread("alice").await;
        assert_eq!(*alice_unread.get("ch1").unwrap_or(&0), 0);

        // Mark read
        hub.mark_read("ch1", "bob").await;
        let bob_after = hub.get_unread("bob").await;
        assert_eq!(*bob_after.get("ch1").unwrap_or(&0), 0);
    }

    #[tokio::test]
    async fn test_ws_hub_edit_broadcast() {
        let hub = WsHub::new();
        let mut rx = hub.subscribe();

        hub.broadcast_edit("ch1", "msg1", "updated content");

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type, "edit");
        assert_eq!(event.data["message_id"], "msg1");
        assert_eq!(event.data["content"], "updated content");
    }

    #[tokio::test]
    async fn test_ws_hub_delete_broadcast() {
        let hub = WsHub::new();
        let mut rx = hub.subscribe();

        hub.broadcast_delete("ch1", "msg1");

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type, "delete");
        assert_eq!(event.data["message_id"], "msg1");
    }

    #[tokio::test]
    async fn test_ws_hub_channel_presence() {
        let hub = WsHub::new();

        hub.join_channel("ch1", "alice").await;
        hub.join_channel("ch1", "bob").await;

        hub.leave_channel("ch1", "alice").await;

        // Just verify no panic — internal state tracking
    }

    #[test]
    fn test_ws_event_serde() {
        let event = WsEvent {
            event_type: "message".to_string(),
            data: json!({"channel_id": "ch1", "content": "hello"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"message\""));
        assert!(json.contains("\"channel_id\":\"ch1\""));
    }
}
