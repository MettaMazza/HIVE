/// HiveChat — Decentralised Discord Clone.
///
/// A 1:1 Discord replacement on the mesh. Servers, channels, DMs,
/// real-time messaging, reactions, threads. Works without internet.
///
/// Served on localhost:3034 (configurable via REMOVED_MESH_GOVERNED).
use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::{State, Query, Path},
    response::{Html, Sse, sse},
};
use std::sync::Arc;
use std::collections::HashMap;
use std::convert::Infallible;
use tokio::sync::{RwLock, broadcast};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use futures::stream::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

// ─── Data Model ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatServer {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub owner_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChannel {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub topic: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub channel_id: String,
    pub author_id: String,
    pub author_name: String,
    pub content: String,
    pub timestamp: String,
    pub reactions: HashMap<String, Vec<String>>,
    pub reply_to: Option<String>,
    pub edited: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectMessage {
    pub id: String,
    pub from_id: String,
    pub from_name: String,
    pub to_id: String,
    pub content: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberInfo {
    pub peer_id: String,
    pub display_name: String,
    pub discord_link: Option<String>,
    pub status: String, // online, idle, offline
    pub joined_at: String,
}

/// Chat store — all state for the Discord clone.
pub struct ChatStore {
    servers: RwLock<Vec<ChatServer>>,
    channels: RwLock<Vec<ChatChannel>>,
    messages: RwLock<HashMap<String, Vec<ChatMessage>>>, // channel_id -> messages
    dms: RwLock<Vec<DirectMessage>>,
    members: RwLock<HashMap<String, Vec<MemberInfo>>>, // server_id -> members
    tx: broadcast::Sender<Value>,
    max_messages: usize,
}

impl ChatStore {
    pub fn new() -> Self {
        let max = std::env::var("HIVE_CHAT_MAX_MESSAGES")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(5000);
        let (tx, _) = broadcast::channel(256);

        let mut servers = vec![];
        let mut channels = vec![];
        let mut messages = HashMap::new();
        let mut members = HashMap::new();

        // Seed with default HIVE server
        let default_server = ChatServer {
            id: "hive-main".to_string(),
            name: "🐝 HIVE".to_string(),
            icon: "🐝".to_string(),
            owner_id: "system".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        servers.push(default_server);

        let default_channels = vec![
            ("general", "General discussion — say hi!"),
            ("dev", "Development updates and coding"),
            ("mesh-status", "Mesh network status and alerts"),
            ("off-topic", "Everything else"),
        ];

        for (name, topic) in &default_channels {
            let ch = ChatChannel {
                id: format!("hive-{}", name),
                server_id: "hive-main".to_string(),
                name: name.to_string(),
                topic: topic.to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            messages.insert(ch.id.clone(), vec![]);
            channels.push(ch);
        }

        // Welcome message in general
        let welcome = ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            channel_id: "hive-general".to_string(),
            author_id: "system".to_string(),
            author_name: "🐝 Apis".to_string(),
            content: "Welcome to HiveChat! This is the decentralised replacement for Discord. Every message here is peer-to-peer — no corporate servers, no data mining, no censorship.\n\nCreate new servers, invite peers, and chat freely. Your conversations belong to you.".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reactions: HashMap::new(),
            reply_to: None,
            edited: false,
        };
        messages.get_mut("hive-general").unwrap().push(welcome);

        // Default member
        let local_name = std::env::var("HIVE_USER_NAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "Anonymous".to_string());
        let local_peer = MemberInfo {
            peer_id: "local".to_string(),
            display_name: local_name,
            discord_link: None,
            status: "online".to_string(),
            joined_at: chrono::Utc::now().to_rfc3339(),
        };
        members.insert("hive-main".to_string(), vec![local_peer]);

        tracing::info!("[HIVECHAT] 💬 ChatStore ready (max_messages={}, servers=1, channels={})", max, default_channels.len());

        Self {
            servers: RwLock::new(servers),
            channels: RwLock::new(channels),
            messages: RwLock::new(messages),
            dms: RwLock::new(vec![]),
            members: RwLock::new(members),
            tx,
            max_messages: max,
        }
    }

    pub async fn create_server(&self, name: &str, icon: &str, owner: &str) -> ChatServer {
        let server = ChatServer {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            icon: icon.to_string(),
            owner_id: owner.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        // Auto-create #general
        let general = ChatChannel {
            id: format!("{}-general", server.id),
            server_id: server.id.clone(),
            name: "general".to_string(),
            topic: "General discussion".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        self.messages.write().await.insert(general.id.clone(), vec![]);
        self.channels.write().await.push(general);
        self.members.write().await.insert(server.id.clone(), vec![]);
        self.servers.write().await.push(server.clone());
        server
    }

    pub async fn create_channel(&self, server_id: &str, name: &str, topic: &str) -> Option<ChatChannel> {
        let servers = self.servers.read().await;
        if !servers.iter().any(|s| s.id == server_id) { return None; }
        drop(servers);

        let channel = ChatChannel {
            id: uuid::Uuid::new_v4().to_string(),
            server_id: server_id.to_string(),
            name: name.to_string(),
            topic: topic.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        self.messages.write().await.insert(channel.id.clone(), vec![]);
        self.channels.write().await.push(channel.clone());
        Some(channel)
    }

    pub async fn send_message(&self, channel_id: &str, author_id: &str, author_name: &str, content: &str, reply_to: Option<String>) -> Option<ChatMessage> {
        let mut messages = self.messages.write().await;
        let msgs = messages.get_mut(channel_id)?;

        let msg = ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            channel_id: channel_id.to_string(),
            author_id: author_id.to_string(),
            author_name: author_name.to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reactions: HashMap::new(),
            reply_to,
            edited: false,
        };

        // Ring buffer
        if msgs.len() >= self.max_messages {
            msgs.remove(0);
        }
        msgs.push(msg.clone());

        let _ = self.tx.send(json!({
            "type": "message",
            "channel_id": channel_id,
            "message": serde_json::to_value(&msg).unwrap_or_default()
        }));

        Some(msg)
    }

    pub async fn react(&self, channel_id: &str, msg_id: &str, emoji: &str, peer_id: &str) -> bool {
        let mut messages = self.messages.write().await;
        if let Some(msgs) = messages.get_mut(channel_id) {
            if let Some(msg) = msgs.iter_mut().find(|m| m.id == msg_id) {
                let voters = msg.reactions.entry(emoji.to_string()).or_default();
                if !voters.contains(&peer_id.to_string()) {
                    voters.push(peer_id.to_string());
                }
                return true;
            }
        }
        false
    }

    pub async fn send_dm(&self, from_id: &str, from_name: &str, to_id: &str, content: &str) -> DirectMessage {
        let dm = DirectMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from_id: from_id.to_string(),
            from_name: from_name.to_string(),
            to_id: to_id.to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        self.dms.write().await.push(dm.clone());

        let _ = self.tx.send(json!({
            "type": "dm", "dm": serde_json::to_value(&dm).unwrap_or_default()
        }));

        dm
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.tx.subscribe()
    }
}

// ─── Server Setup ───────────────────────────────────────────────────────

#[derive(Clone)]
struct HiveChatState {
    store: Arc<ChatStore>,
    local_peer_id: String,
    local_display_name: String,
}

#[derive(Deserialize)]
struct CreateServerReq { name: String, icon: Option<String> }
#[derive(Deserialize)]
struct CreateChannelReq { name: String, topic: Option<String> }
#[derive(Deserialize)]
struct SendMessageReq { content: String, reply_to: Option<String> }
#[derive(Deserialize)]
struct ReactReq { emoji: String }
#[derive(Deserialize)]
struct SendDmReq { to_id: String, content: String }
#[derive(Deserialize)]
struct LinkDiscordReq { discord_username: String }
#[derive(Deserialize)]
struct MessagesQuery { limit: Option<usize> }

pub async fn spawn_hive_chat_server() {
    let port: u16 = std::env::var("REMOVED_MESH_GOVERNED")
        .ok().and_then(|v| v.parse().ok())
        .unwrap_or(3034);

    let local_peer_id = std::env::var("HIVE_MESH_CHAT_NAME")
        .unwrap_or_else(|_| "local".to_string());
    let local_display_name = std::env::var("HIVE_USER_NAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "Anonymous".to_string());

    let store = Arc::new(ChatStore::new());
    let state = HiveChatState { store, local_peer_id, local_display_name };

    tokio::spawn(async move {
        tracing::info!("[HIVECHAT] 💬 Starting on http://127.0.0.1:{}", port);

        let app = Router::new()
            .route("/api/servers", get(api_servers).post(api_create_server))
            .route("/api/server/{server_id}/channels", get(api_channels).post(api_create_channel))
            .route("/api/server/{server_id}/members", get(api_members))
            .route("/api/channel/{channel_id}/messages", get(api_messages))
            .route("/api/channel/{channel_id}/message", post(api_send_message))
            .route("/api/message/{channel_id}/{msg_id}/react", post(api_react_msg))
            .route("/api/dms", get(api_dms))
            .route("/api/dm", post(api_send_dm))
            .route("/api/link-discord", post(api_link_discord))
            .route("/api/stream", get(api_stream))
            .route("/api/status", get(api_chat_status))
            .fallback(get(serve_chat_spa))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let addr = format!("127.0.0.1:{}", port);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                tracing::info!("[HIVECHAT] 💬 Bound on {}", addr);
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("[HIVECHAT] ❌ Server error: {}", e);
                }
            }
            Err(e) => tracing::error!("[HIVECHAT] ❌ Failed to bind {}: {}", addr, e),
        }
    });
}

// ─── API Endpoints ──────────────────────────────────────────────────────

async fn api_servers(State(state): State<HiveChatState>) -> Json<Value> {
    let servers = state.store.servers.read().await;
    Json(json!({"servers": *servers}))
}

async fn api_create_server(State(state): State<HiveChatState>, Json(req): Json<CreateServerReq>) -> Json<Value> {
    if req.name.trim().is_empty() { return Json(json!({"error": "Server name required"})); }
    let server = state.store.create_server(&req.name, req.icon.as_deref().unwrap_or("🌐"), &state.local_peer_id).await;
    Json(json!({"ok": true, "server": server}))
}

async fn api_channels(State(state): State<HiveChatState>, Path(server_id): Path<String>) -> Json<Value> {
    let channels = state.store.channels.read().await;
    let filtered: Vec<_> = channels.iter().filter(|c| c.server_id == server_id).cloned().collect();
    Json(json!({"channels": filtered}))
}

async fn api_create_channel(State(state): State<HiveChatState>, Path(server_id): Path<String>, Json(req): Json<CreateChannelReq>) -> Json<Value> {
    if req.name.trim().is_empty() { return Json(json!({"error": "Channel name required"})); }
    match state.store.create_channel(&server_id, &req.name, req.topic.as_deref().unwrap_or("")).await {
        Some(ch) => Json(json!({"ok": true, "channel": ch})),
        None => Json(json!({"error": "Server not found"})),
    }
}

async fn api_messages(State(state): State<HiveChatState>, Path(channel_id): Path<String>, Query(params): Query<MessagesQuery>) -> Json<Value> {
    let limit = params.limit.unwrap_or(100).min(500);
    let messages = state.store.messages.read().await;
    let msgs = messages.get(&channel_id).map(|m| {
        m.iter().rev().take(limit).cloned().collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>()
    }).unwrap_or_default();
    Json(json!({"messages": msgs, "count": msgs.len()}))
}

async fn api_send_message(State(state): State<HiveChatState>, Path(channel_id): Path<String>, Json(req): Json<SendMessageReq>) -> Json<Value> {
    if req.content.trim().is_empty() { return Json(json!({"error": "Message cannot be empty"})); }

    // Content filter
    let filter = crate::network::content_filter::ContentFilter::new();
    let peer_id = crate::network::messages::PeerId(state.local_peer_id.clone());
    let scan = filter.scan(&peer_id, &req.content).await;
    if scan != crate::network::content_filter::ScanResult::Clean {
        return Json(json!({"error": "Message blocked by content filter", "reason": format!("{:?}", scan)}));
    }

    match state.store.send_message(&channel_id, &state.local_peer_id, &state.local_display_name, &req.content, req.reply_to).await {
        Some(msg) => Json(json!({"ok": true, "message": msg})),
        None => Json(json!({"error": "Channel not found"})),
    }
}

async fn api_react_msg(State(state): State<HiveChatState>, Path((channel_id, msg_id)): Path<(String, String)>, Json(req): Json<ReactReq>) -> Json<Value> {
    let ok = state.store.react(&channel_id, &msg_id, &req.emoji, &state.local_peer_id).await;
    Json(json!({"ok": ok}))
}

async fn api_members(State(state): State<HiveChatState>, Path(server_id): Path<String>) -> Json<Value> {
    let members = state.store.members.read().await;
    let list = members.get(&server_id).cloned().unwrap_or_default();
    Json(json!({"members": list, "online": list.iter().filter(|m| m.status == "online").count()}))
}

async fn api_dms(State(state): State<HiveChatState>) -> Json<Value> {
    let dms = state.store.dms.read().await;
    let my_dms: Vec<_> = dms.iter().filter(|d| d.from_id == state.local_peer_id || d.to_id == state.local_peer_id).cloned().collect();
    Json(json!({"dms": my_dms}))
}

async fn api_send_dm(State(state): State<HiveChatState>, Json(req): Json<SendDmReq>) -> Json<Value> {
    if req.content.trim().is_empty() { return Json(json!({"error": "Message cannot be empty"})); }
    let dm = state.store.send_dm(&state.local_peer_id, &state.local_display_name, &req.to_id, &req.content).await;
    Json(json!({"ok": true, "dm": dm}))
}

async fn api_link_discord(State(state): State<HiveChatState>, Json(req): Json<LinkDiscordReq>) -> Json<Value> {
    let mut members = state.store.members.write().await;
    for member_list in members.values_mut() {
        for member in member_list.iter_mut() {
            if member.peer_id == state.local_peer_id {
                member.discord_link = Some(req.discord_username.clone());
            }
        }
    }
    Json(json!({"ok": true, "linked": req.discord_username}))
}

async fn api_stream(State(state): State<HiveChatState>) -> Sse<impl Stream<Item = Result<sse::Event, Infallible>>> {
    let rx = state.store.subscribe();
    let stream = BroadcastStream::new(rx)
        .filter_map(|result| {
            result.ok().map(|val| {
                Ok(sse::Event::default()
                    .json_data(&val)
                    .unwrap_or_else(|_| sse::Event::default().data("error")))
            })
        });
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
    )
}

async fn api_chat_status(State(state): State<HiveChatState>) -> Json<Value> {
    let servers = state.store.servers.read().await;
    let channels = state.store.channels.read().await;
    let messages = state.store.messages.read().await;
    let total_msgs: usize = messages.values().map(|v| v.len()).sum();
    Json(json!({
        "servers": servers.len(), "channels": channels.len(),
        "total_messages": total_msgs, "peer_id": state.local_peer_id,
        "display_name": state.local_display_name,
    }))
}

// ─── SPA Frontend ───────────────────────────────────────────────────────

async fn serve_chat_spa() -> Html<String> {
    Html(CHAT_HTML.to_string())
}

use super::hive_chat_html::CHAT_HTML;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_html_not_empty() {
        assert!(CHAT_HTML.len() > 1000);
        assert!(CHAT_HTML.contains("HiveChat"));
        assert!(CHAT_HTML.contains("/api/servers"));
        assert!(CHAT_HTML.contains("/api/channel"));
    }

    #[tokio::test]
    async fn test_chat_store_create_server() {
        let store = ChatStore::new();
        let server = store.create_server("Test", "🎮", "owner1").await;
        assert_eq!(server.name, "Test");
        let servers = store.servers.read().await;
        assert!(servers.len() >= 2); // default + new
    }

    #[tokio::test]
    async fn test_chat_store_send_message() {
        let store = ChatStore::new();
        let msg = store.send_message("hive-general", "peer1", "Alice", "Hello!", None).await;
        assert!(msg.is_some());
        let messages = store.messages.read().await;
        assert!(messages["hive-general"].len() >= 2); // welcome + new
    }

    #[tokio::test]
    async fn test_chat_store_react() {
        let store = ChatStore::new();
        let msg = store.send_message("hive-general", "p1", "A", "Test", None).await.unwrap();
        assert!(store.react("hive-general", &msg.id, "👍", "p2").await);
        assert!(!store.react("nonexistent", "fake", "👍", "p2").await);
    }

    #[tokio::test]
    async fn test_chat_store_dm() {
        let store = ChatStore::new();
        let dm = store.send_dm("alice", "Alice", "bob", "Hey!").await;
        assert_eq!(dm.from_name, "Alice");
        let dms = store.dms.read().await;
        assert_eq!(dms.len(), 1);
    }
}
