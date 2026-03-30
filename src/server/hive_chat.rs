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

const CHAT_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HiveChat — Decentralised Messaging</title>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet">
    <style>
        *{margin:0;padding:0;box-sizing:border-box}
        :root{--bg:#1a1a2e;--surface:#16213e;--panel:#0f3460;--card:#1a1a40;--border:rgba(255,255,255,0.08);--text:#e8e8f0;--text-dim:#8888aa;--text-muted:#555577;--accent:#e94560;--accent-dim:rgba(233,69,96,0.15);--green:#53d769;--yellow:#f5c542;--blue:#5b7fff;--radius:12px}
        body{font-family:'Inter',sans-serif;background:var(--bg);color:var(--text);height:100vh;overflow:hidden;display:flex}

        /* Server List */
        .server-list{width:72px;background:#0a0a1a;display:flex;flex-direction:column;align-items:center;padding:12px 0;gap:8px;border-right:1px solid var(--border);overflow-y:auto;flex-shrink:0}
        .server-icon{width:48px;height:48px;border-radius:16px;background:var(--card);display:flex;align-items:center;justify-content:center;font-size:20px;cursor:pointer;transition:all .2s;border:2px solid transparent}
        .server-icon:hover{border-radius:12px;border-color:var(--accent)}
        .server-icon.active{border-color:var(--accent);border-radius:12px;background:var(--accent-dim)}
        .server-add{width:48px;height:48px;border-radius:50%;background:transparent;border:2px dashed var(--border);display:flex;align-items:center;justify-content:center;font-size:20px;color:var(--text-muted);cursor:pointer}
        .server-add:hover{border-color:var(--green);color:var(--green)}
        .server-divider{width:32px;height:2px;background:var(--border);border-radius:1px}

        /* Channel Sidebar */
        .channel-sidebar{width:240px;background:var(--surface);display:flex;flex-direction:column;border-right:1px solid var(--border);flex-shrink:0}
        .server-header{padding:14px 16px;font-weight:700;font-size:14px;border-bottom:1px solid var(--border);display:flex;justify-content:space-between;align-items:center}
        .server-header button{background:none;border:none;color:var(--text-dim);cursor:pointer;font-size:16px}
        .channel-list{flex:1;overflow-y:auto;padding:8px}
        .channel-category{font-size:10px;font-weight:700;color:var(--text-muted);text-transform:uppercase;letter-spacing:1px;padding:16px 8px 4px;display:flex;align-items:center;justify-content:space-between}
        .channel-item{padding:6px 8px;border-radius:6px;cursor:pointer;display:flex;align-items:center;gap:6px;font-size:13px;color:var(--text-dim);transition:background .15s}
        .channel-item:hover{background:rgba(255,255,255,0.05);color:var(--text)}
        .channel-item.active{background:var(--accent-dim);color:var(--text)}
        .channel-hash{color:var(--text-muted);font-weight:500}

        /* User Panel */
        .user-panel{padding:10px;border-top:1px solid var(--border);background:rgba(0,0,0,0.2);display:flex;align-items:center;gap:8px}
        .user-avatar{width:32px;height:32px;border-radius:50%;background:linear-gradient(135deg,var(--accent),var(--blue));display:flex;align-items:center;justify-content:center;font-size:14px;font-weight:700}
        .user-info{flex:1}
        .user-info .name{font-size:12px;font-weight:600}
        .user-info .status{font-size:10px;color:var(--green);display:flex;align-items:center;gap:4px}
        .status-dot{width:6px;height:6px;border-radius:50%;background:var(--green)}

        /* Chat Area */
        .chat-area{flex:1;display:flex;flex-direction:column;overflow:hidden}
        .chat-header{padding:12px 16px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:8px;background:rgba(0,0,0,0.1)}
        .chat-header .channel-name{font-weight:600;font-size:15px}
        .chat-header .topic{font-size:12px;color:var(--text-dim);margin-left:8px;border-left:1px solid var(--border);padding-left:8px}

        .messages{flex:1;overflow-y:auto;padding:16px}
        .message{display:flex;gap:12px;padding:4px 0;margin-bottom:4px;border-radius:8px;transition:background .15s}
        .message:hover{background:rgba(255,255,255,0.02)}
        .msg-avatar{width:40px;height:40px;border-radius:50%;background:linear-gradient(135deg,var(--accent),#ff6b6b);display:flex;align-items:center;justify-content:center;font-weight:700;font-size:14px;flex-shrink:0}
        .msg-body{flex:1;min-width:0}
        .msg-header{display:flex;align-items:baseline;gap:8px}
        .msg-author{font-weight:600;font-size:14px;color:var(--accent)}
        .msg-time{font-size:11px;color:var(--text-muted)}
        .msg-content{font-size:14px;line-height:1.5;color:var(--text);margin-top:2px;word-break:break-word;white-space:pre-wrap}
        .msg-reactions{display:flex;gap:4px;margin-top:4px;flex-wrap:wrap}
        .msg-react-btn{padding:2px 8px;border-radius:6px;border:1px solid var(--border);background:transparent;color:var(--text-dim);cursor:pointer;font-size:12px;transition:all .15s}
        .msg-react-btn:hover{background:var(--accent-dim);border-color:var(--accent)}
        .msg-reply{font-size:11px;color:var(--text-muted);padding:4px 8px;border-left:2px solid var(--accent);margin-bottom:4px}

        /* Message Input */
        .msg-input-area{padding:12px 16px;border-top:1px solid var(--border)}
        .msg-input-wrap{display:flex;align-items:center;background:var(--card);border-radius:8px;border:1px solid var(--border);padding:4px}
        .msg-input{flex:1;background:transparent;border:none;color:var(--text);font-family:inherit;font-size:14px;padding:8px 12px;outline:none}
        .msg-send{padding:8px 16px;border-radius:6px;border:none;background:var(--accent);color:#fff;font-weight:600;cursor:pointer;font-family:inherit;font-size:13px}
        .msg-send:hover{opacity:.9}

        /* Members */
        .member-list{width:240px;background:var(--surface);border-left:1px solid var(--border);padding:12px;overflow-y:auto;flex-shrink:0}
        .member-category{font-size:10px;font-weight:700;color:var(--text-muted);text-transform:uppercase;letter-spacing:1px;padding:8px 0 4px}
        .member-item{display:flex;align-items:center;gap:8px;padding:4px 0;cursor:pointer;border-radius:4px}
        .member-item:hover{background:rgba(255,255,255,0.03)}
        .member-avatar{width:28px;height:28px;border-radius:50%;background:var(--card);display:flex;align-items:center;justify-content:center;font-size:11px;font-weight:600;position:relative}
        .member-avatar .dot{position:absolute;bottom:-1px;right:-1px;width:10px;height:10px;border-radius:50%;border:2px solid var(--surface)}
        .member-name{font-size:13px;color:var(--text-dim)}
        .discord-badge{font-size:9px;color:var(--blue);background:rgba(91,127,255,0.15);padding:1px 4px;border-radius:3px}

        /* Day Separator */
        .day-sep{text-align:center;padding:16px 0;font-size:11px;color:var(--text-muted);position:relative}
        .day-sep span{background:var(--bg);padding:0 12px;position:relative;z-index:1}
        .day-sep::before{content:'';position:absolute;left:0;right:0;top:50%;height:1px;background:var(--border)}

        /* Scrollbar */
        ::-webkit-scrollbar{width:6px}::-webkit-scrollbar-track{background:transparent}::-webkit-scrollbar-thumb{background:var(--border);border-radius:3px}

        @media(max-width:900px){.member-list{display:none}}
        @media(max-width:700px){.channel-sidebar{display:none}}
    </style>
</head>
<body>
    <!-- Server List -->
    <div class="server-list" id="server-list"></div>

    <!-- Channel Sidebar -->
    <div class="channel-sidebar">
        <div class="server-header">
            <span id="server-name">🐝 HIVE</span>
            <button onclick="createChannel()" title="New Channel">+</button>
        </div>
        <div class="channel-list" id="channel-list"></div>
        <div class="user-panel">
            <div class="user-avatar" id="user-initial">?</div>
            <div class="user-info">
                <div class="name" id="user-name">Loading...</div>
                <div class="status"><div class="status-dot"></div> Online</div>
            </div>
            <button style="background:none;border:none;color:var(--text-muted);cursor:pointer;font-size:14px" onclick="linkDiscord()" title="Link Discord">🔗</button>
        </div>
    </div>

    <!-- Chat Area -->
    <div class="chat-area">
        <div class="chat-header">
            <span class="channel-hash">#</span>
            <span class="channel-name" id="current-channel-name">general</span>
            <span class="topic" id="current-topic">General discussion — say hi!</span>
        </div>
        <div class="messages" id="messages"></div>
        <div class="msg-input-area">
            <div class="msg-input-wrap">
                <input class="msg-input" id="msg-input" placeholder="Message #general" onkeydown="if(event.key==='Enter')sendMsg()">
                <button class="msg-send" onclick="sendMsg()">Send</button>
            </div>
        </div>
    </div>

    <!-- Members -->
    <div class="member-list" id="member-list"></div>

<script>
let currentServer = 'hive-main';
let currentChannel = 'hive-general';
let servers = [];
let channels = [];

async function loadServers() {
    const res = await fetch('/api/servers');
    const data = await res.json();
    servers = data.servers || [];
    const list = document.getElementById('server-list');
    list.innerHTML = servers.map(s => `
        <div class="server-icon ${s.id===currentServer?'active':''}" onclick="switchServer('${s.id}')" title="${esc(s.name)}">
            ${s.icon || s.name[0]}
        </div>
    `).join('') + '<div class="server-divider"></div><div class="server-add" onclick="createServer()">+</div>';
}

async function switchServer(id) {
    currentServer = id;
    const s = servers.find(x=>x.id===id);
    document.getElementById('server-name').textContent = s ? s.name : id;
    loadServers();
    await loadChannels();
    if (channels.length) switchChannel(channels[0].id);
}

async function loadChannels() {
    const res = await fetch(`/api/server/${currentServer}/channels`);
    const data = await res.json();
    channels = data.channels || [];
    const list = document.getElementById('channel-list');
    list.innerHTML = '<div class="channel-category">Text Channels</div>' +
        channels.map(c => `
            <div class="channel-item ${c.id===currentChannel?'active':''}" onclick="switchChannel('${c.id}')">
                <span class="channel-hash">#</span> ${esc(c.name)}
            </div>
        `).join('');
}

function switchChannel(id) {
    currentChannel = id;
    const ch = channels.find(c=>c.id===id);
    document.getElementById('current-channel-name').textContent = ch ? ch.name : id;
    document.getElementById('current-topic').textContent = ch ? ch.topic : '';
    document.getElementById('msg-input').placeholder = `Message #${ch ? ch.name : 'general'}`;
    loadChannels();
    loadMessages();
}

async function loadMessages() {
    const res = await fetch(`/api/channel/${currentChannel}/messages?limit=100`);
    const data = await res.json();
    const container = document.getElementById('messages');
    const msgs = data.messages || [];
    if (!msgs.length) {
        container.innerHTML = '<div style="text-align:center;padding:40px;color:var(--text-muted)"><p style="font-size:40px">👋</p><p>No messages yet. Say something!</p></div>';
        return;
    }

    container.innerHTML = '<div class="day-sep"><span>Today</span></div>' +
        msgs.map(m => {
            const init = (m.author_name||'?')[0].toUpperCase();
            const reactions = Object.entries(m.reactions||{}).map(([e,v]) =>
                `<button class="msg-react-btn" onclick="reactMsg('${m.id}','${e}')">${e} ${v.length}</button>`
            ).join('');
            const reply = m.reply_to ? `<div class="msg-reply">↩ reply</div>` : '';
            return `${reply}<div class="message">
                <div class="msg-avatar">${init}</div>
                <div class="msg-body">
                    <div class="msg-header">
                        <span class="msg-author">${esc(m.author_name)}</span>
                        <span class="msg-time">${timeAgo(m.timestamp)}</span>
                    </div>
                    <div class="msg-content">${esc(m.content)}</div>
                    <div class="msg-reactions">${reactions}
                        <button class="msg-react-btn" onclick="reactMsg('${m.id}','👍')">+</button>
                    </div>
                </div>
            </div>`;
        }).join('');
    container.scrollTop = container.scrollHeight;
}

async function sendMsg() {
    const input = document.getElementById('msg-input');
    const content = input.value.trim();
    if (!content) return;
    input.value = '';
    await fetch(`/api/channel/${currentChannel}/message`, {
        method: 'POST', headers: {'Content-Type':'application/json'},
        body: JSON.stringify({ content })
    });
}

async function reactMsg(msgId, emoji) {
    await fetch(`/api/message/${currentChannel}/${msgId}/react`, {
        method: 'POST', headers: {'Content-Type':'application/json'},
        body: JSON.stringify({ emoji })
    });
    loadMessages();
}

async function loadMembers() {
    const res = await fetch(`/api/server/${currentServer}/members`);
    const data = await res.json();
    const list = document.getElementById('member-list');
    const members = data.members || [];
    const online = members.filter(m=>m.status==='online');
    const offline = members.filter(m=>m.status!=='online');
    list.innerHTML = `<div class="member-category">Online — ${online.length}</div>` +
        online.map(m => memberHtml(m, true)).join('') +
        (offline.length ? `<div class="member-category">Offline — ${offline.length}</div>` +
        offline.map(m => memberHtml(m, false)).join('') : '');
}

function memberHtml(m, online) {
    const badge = m.discord_link ? `<span class="discord-badge">🔗 ${esc(m.discord_link)}</span>` : '';
    return `<div class="member-item">
        <div class="member-avatar">${(m.display_name||'?')[0].toUpperCase()}
            <div class="dot" style="background:${online?'var(--green)':'var(--text-muted)'}"></div>
        </div>
        <div><div class="member-name">${esc(m.display_name)}</div>${badge}</div>
    </div>`;
}

function createServer() {
    const name = prompt('Server name:');
    if (!name) return;
    const icon = prompt('Server icon (emoji):', '🌐') || '🌐';
    fetch('/api/servers', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({name, icon}) })
        .then(() => loadServers());
}

function createChannel() {
    const name = prompt('Channel name:');
    if (!name) return;
    fetch(`/api/server/${currentServer}/channels`, { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({name}) })
        .then(() => loadChannels());
}

function linkDiscord() {
    const username = prompt('Your Discord username (e.g. user#1234):');
    if (!username) return;
    fetch('/api/link-discord', { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({discord_username: username}) })
        .then(() => loadMembers());
}

// SSE
const evtSource = new EventSource('/api/stream');
evtSource.onmessage = (e) => {
    try {
        const data = JSON.parse(e.data);
        if (data.type === 'message' && data.channel_id === currentChannel) loadMessages();
    } catch(err) {}
};

function timeAgo(ts) {
    const s = Math.floor((Date.now()-new Date(ts))/1000);
    if(s<60)return 'just now';if(s<3600)return Math.floor(s/60)+'m ago';
    if(s<86400)return Math.floor(s/3600)+'h ago';return Math.floor(s/86400)+'d ago';
}
function esc(t){if(!t)return'';const d=document.createElement('div');d.textContent=t;return d.innerHTML}

async function loadStatus() {
    const res = await fetch('/api/status');
    const data = await res.json();
    document.getElementById('user-name').textContent = data.display_name || 'Unknown';
    document.getElementById('user-initial').textContent = (data.display_name||'?')[0].toUpperCase();
}

// Boot
loadServers(); loadChannels().then(()=>loadMessages()); loadMembers(); loadStatus();
</script>
</body>
</html>"##;

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
