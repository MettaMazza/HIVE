/// HiveSurface — Decentralised Social Web Platform.
///
/// The localhost replacement for the surface web. Facebook + Reddit + Twitter
/// in one decentralised platform. Works without internet — mesh peers
/// share connections so everyone stays online.
///
/// Served on localhost:3032 (configurable via REMOVED_MESH_GOVERNED).
use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::{State, Query, Path},
    response::{Html, Sse, sse},
};
use std::sync::Arc;
use std::convert::Infallible;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use serde::Deserialize;
use serde_json::{Value, json};
use futures::stream::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::network::post_store::{PostStore, MeshPost, PostType};

#[derive(Clone)]
struct SurfaceState {
    post_store: Arc<PostStore>,
    local_peer_id: String,
    local_display_name: String,
}

#[derive(Deserialize)]
struct FeedQuery {
    limit: Option<usize>,
    community: Option<String>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct CreatePost {
    content: String,
    #[serde(default)]
    post_type: Option<String>,
    link_url: Option<String>,
    community: Option<String>,
}

#[derive(Deserialize)]
struct ReactRequest {
    emoji: String,
}

#[derive(Deserialize)]
struct ReplyRequest {
    content: String,
}

pub async fn spawn_mesh_social_server(post_store: Arc<PostStore>) {
    let port: u16 = std::env::var("REMOVED_MESH_GOVERNED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3032);

    let local_peer_id = std::env::var("HIVE_MESH_CHAT_NAME")
        .unwrap_or_else(|_| "Apis".to_string());
    let local_display_name = std::env::var("HIVE_USER_NAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "Anonymous".to_string());

    let state = SurfaceState {
        post_store,
        local_peer_id,
        local_display_name,
    };

    tokio::spawn(async move {
        tracing::info!("[SURFACE] 🌐 HiveSurface starting on http://127.0.0.1:{}", port);

        let app = Router::new()
            .route("/api/status", get(api_status))
            .route("/api/feed", get(api_feed))
            .route("/api/trending", get(api_trending))
            .route("/api/post", post(api_create_post))
            .route("/api/post/{post_id}/react", post(api_react))
            .route("/api/post/{post_id}/reply", post(api_reply))
            .route("/api/search", get(api_search))
            .route("/api/communities", get(api_communities))
            .route("/api/profile/{peer_id}", get(api_profile))
            .route("/api/alerts", get(api_alerts))
            .route("/api/stream", get(api_stream))
            .fallback(get(serve_spa))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let addr = format!("127.0.0.1:{}", port);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                tracing::info!("[SURFACE] 🌐 HiveSurface bound on {}", addr);
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("[SURFACE] ❌ Server error: {}", e);
                }
            }
            Err(e) => {
                tracing::error!("[SURFACE] ❌ Failed to bind {}: {}", addr, e);
            }
        }
    });
}

// ─── API Endpoints ──────────────────────────────────────────────────────

async fn api_status() -> Json<Value> {
    let pool = crate::network::pool::PoolManager::new(
        crate::network::messages::PeerId("status".into())
    );
    let pool_stats = pool.stats().await;

    let clearnet = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build().unwrap_or_default()
        .get("https://1.1.1.1/cdn-cgi/trace")
        .send().await.is_ok();

    Json(json!({
        "clearnet_available": clearnet,
        "connectivity": if clearnet { "online" } else { "mesh_only" },
        "web_relays": pool_stats["web_relays_available"],
        "compute_nodes": pool_stats["compute_nodes_available"],
        "total_compute_slots": pool_stats["total_compute_slots"],
        "web_share_enabled": pool_stats["web_share_enabled"],
        "compute_share_enabled": pool_stats["compute_share_enabled"],
    }))
}

async fn api_feed(
    State(state): State<SurfaceState>,
    Query(params): Query<FeedQuery>,
) -> Json<Value> {
    let limit = params.limit.unwrap_or(50).min(200);

    let posts = if let Some(community) = &params.community {
        state.post_store.by_community(community, limit).await
    } else {
        state.post_store.recent(limit).await
    };

    Json(json!({
        "posts": posts,
        "count": posts.len(),
    }))
}

async fn api_trending(State(state): State<SurfaceState>) -> Json<Value> {
    let posts = state.post_store.trending(20).await;
    Json(json!({
        "posts": posts,
        "count": posts.len(),
    }))
}

async fn api_create_post(
    State(state): State<SurfaceState>,
    Json(req): Json<CreatePost>,
) -> Json<Value> {
    if req.content.trim().is_empty() {
        return Json(json!({"error": "Post content cannot be empty"}));
    }

    // Content filter
    let filter = crate::network::content_filter::ContentFilter::new();
    let peer_id = crate::network::messages::PeerId(state.local_peer_id.clone());
    let scan = filter.scan(&peer_id, &req.content).await;
    if scan != crate::network::content_filter::ScanResult::Clean {
        return Json(json!({"error": "Post rejected by content filter", "reason": format!("{:?}", scan)}));
    }

    let post_type = match req.post_type.as_deref() {
        Some("link") => PostType::Link,
        Some("alert") => PostType::EmergencyAlert,
        Some("resource") => PostType::ResourceOffer,
        _ => PostType::Text,
    };

    let mut post = MeshPost::new(
        &state.local_peer_id,
        &state.local_display_name,
        &req.content,
        post_type,
    );

    if let Some(url) = &req.link_url {
        post = post.with_link(url);
    }
    if let Some(community) = &req.community {
        post = post.with_community(community);
    }

    let post_id = post.id.clone();
    state.post_store.push(post).await;

    Json(json!({"ok": true, "post_id": post_id}))
}

async fn api_react(
    State(state): State<SurfaceState>,
    Path(post_id): Path<String>,
    Json(req): Json<ReactRequest>,
) -> Json<Value> {
    let ok = state.post_store.react(&post_id, &req.emoji, &state.local_peer_id).await;
    Json(json!({"ok": ok}))
}

async fn api_reply(
    State(state): State<SurfaceState>,
    Path(post_id): Path<String>,
    Json(req): Json<ReplyRequest>,
) -> Json<Value> {
    if req.content.trim().is_empty() {
        return Json(json!({"error": "Reply cannot be empty"}));
    }

    let reply = MeshPost::new(
        &state.local_peer_id,
        &state.local_display_name,
        &req.content,
        PostType::Text,
    );
    let ok = state.post_store.reply_to(&post_id, reply).await;
    Json(json!({"ok": ok}))
}

async fn api_search(
    State(state): State<SurfaceState>,
    Query(params): Query<SearchQuery>,
) -> Json<Value> {
    let limit = params.limit.unwrap_or(50).min(200);
    let posts = state.post_store.search(&params.q, limit).await;
    Json(json!({
        "posts": posts,
        "count": posts.len(),
        "query": params.q,
    }))
}

async fn api_communities(State(state): State<SurfaceState>) -> Json<Value> {
    let communities = state.post_store.communities().await;
    Json(json!({
        "communities": communities.iter().map(|(name, count)| json!({
            "name": name,
            "post_count": count,
        })).collect::<Vec<_>>(),
    }))
}

async fn api_profile(
    State(state): State<SurfaceState>,
    Path(peer_id): Path<String>,
) -> Json<Value> {
    let posts = state.post_store.by_author(&peer_id, 50).await;
    Json(json!({
        "peer_id": peer_id,
        "posts": posts,
        "post_count": posts.len(),
    }))
}

async fn api_alerts() -> Json<Value> {
    let gov = crate::network::governance::GovernanceEngine::new();
    let alerts = gov.recent_alerts(20).await;
    Json(json!({
        "alerts": alerts,
        "count": alerts.len(),
    }))
}

async fn api_stream(
    State(state): State<SurfaceState>,
) -> Sse<impl Stream<Item = Result<sse::Event, Infallible>>> {
    let rx = state.post_store.subscribe();
    let stream = BroadcastStream::new(rx)
        .filter_map(|result| {
            result.ok().map(|post| {
                Ok(sse::Event::default()
                    .json_data(&post)
                    .unwrap_or_else(|_| sse::Event::default().data("error")))
            })
        });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
    )
}

// ─── SPA Frontend ───────────────────────────────────────────────────────

async fn serve_spa() -> Html<String> {
    Html(SPA_HTML.to_string())
}

const SPA_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HiveSurface — Decentralised Web</title>
    <meta name="description" content="The decentralised surface web. Social, browsing, and communication — all peer-to-peer.">
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700;800&display=swap" rel="stylesheet">
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        :root {
            --bg: #08080d; --surface: #111118; --card: #16161f;
            --border: rgba(255,255,255,0.06); --border-hover: rgba(255,193,7,0.3);
            --text: #e0e0e8; --text-dim: #888; --text-muted: #555;
            --amber: #ffc107; --amber-glow: rgba(255,193,7,0.15);
            --green: #4caf50; --red: #ef5350; --blue: #42a5f5;
            --radius: 16px; --radius-sm: 10px;
        }
        body { font-family: 'Inter', sans-serif; background: var(--bg); color: var(--text); min-height: 100vh; }

        /* ── Status Bar ── */
        .status-bar {
            position: fixed; top: 0; left: 0; right: 0; z-index: 100;
            background: rgba(8,8,13,0.85); backdrop-filter: blur(20px);
            border-bottom: 1px solid var(--border);
            display: flex; align-items: center; justify-content: space-between;
            padding: 0 24px; height: 56px;
        }
        .logo { display: flex; align-items: center; gap: 10px; }
        .logo h1 { font-size: 20px; font-weight: 800;
            background: linear-gradient(135deg, #ffc107, #ff9800);
            -webkit-background-clip: text; -webkit-text-fill-color: transparent; }
        .mesh-stats { display: flex; gap: 12px; font-size: 12px; }
        .stat-pill {
            padding: 4px 12px; border-radius: 20px;
            background: rgba(255,255,255,0.04); border: 1px solid var(--border);
            display: flex; align-items: center; gap: 6px;
        }
        .stat-pill .dot { width: 6px; height: 6px; border-radius: 50%; }
        .dot-green { background: var(--green); box-shadow: 0 0 8px var(--green); }
        .dot-amber { background: var(--amber); box-shadow: 0 0 8px var(--amber); }
        .dot-red { background: var(--red); animation: pulse 2s infinite; }
        @keyframes pulse { 0%,100%{opacity:1} 50%{opacity:0.3} }

        /* ── Nav ── */
        .nav-bar {
            position: fixed; top: 56px; left: 0; right: 0; z-index: 99;
            background: rgba(8,8,13,0.9); backdrop-filter: blur(12px);
            border-bottom: 1px solid var(--border);
            display: flex; gap: 4px; padding: 8px 24px; overflow-x: auto;
        }
        .nav-btn {
            padding: 8px 18px; border-radius: 24px; border: 1px solid var(--border);
            background: transparent; color: var(--text-dim); cursor: pointer;
            font-size: 13px; font-family: inherit; white-space: nowrap; transition: all 0.2s;
        }
        .nav-btn:hover, .nav-btn.active {
            background: var(--amber-glow); border-color: var(--amber); color: var(--amber);
        }

        /* ── Layout ── */
        .app { display: flex; margin-top: 112px; min-height: calc(100vh - 112px); }
        .sidebar { width: 260px; padding: 20px; border-right: 1px solid var(--border);
            position: sticky; top: 112px; height: calc(100vh - 112px); overflow-y: auto; }
        .main { flex: 1; max-width: 680px; padding: 20px; margin: 0 auto; }
        .right-bar { width: 300px; padding: 20px; border-left: 1px solid var(--border);
            position: sticky; top: 112px; height: calc(100vh - 112px); overflow-y: auto; }

        @media (max-width: 1100px) { .right-bar { display: none; } }
        @media (max-width: 800px) { .sidebar { display: none; } .main { padding: 12px; } }

        /* ── Composer ── */
        .composer {
            background: var(--card); border: 1px solid var(--border);
            border-radius: var(--radius); padding: 16px; margin-bottom: 20px;
        }
        .composer textarea {
            width: 100%; background: transparent; border: none; color: var(--text);
            font-family: inherit; font-size: 14px; resize: none; outline: none;
            min-height: 60px; line-height: 1.6;
        }
        .composer-actions {
            display: flex; justify-content: space-between; align-items: center;
            margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--border);
        }
        .composer-tools { display: flex; gap: 8px; }
        .tool-btn {
            padding: 6px 12px; border-radius: 8px; border: 1px solid var(--border);
            background: transparent; color: var(--text-dim); cursor: pointer;
            font-size: 12px; font-family: inherit; transition: all 0.2s;
        }
        .tool-btn:hover { background: var(--amber-glow); border-color: var(--amber); color: var(--amber); }
        .tool-btn.active { background: var(--amber-glow); border-color: var(--amber); color: var(--amber); }
        .post-btn {
            padding: 8px 20px; border-radius: 24px; border: none;
            background: linear-gradient(135deg, #ffc107, #ff9800);
            color: #000; font-weight: 600; cursor: pointer; font-family: inherit;
            font-size: 13px; transition: all 0.2s;
        }
        .post-btn:hover { transform: scale(1.05); box-shadow: 0 4px 20px rgba(255,193,7,0.3); }

        /* ── Posts ── */
        .post {
            background: var(--card); border: 1px solid var(--border);
            border-radius: var(--radius); padding: 18px 20px; margin-bottom: 12px;
            transition: all 0.3s; animation: fadeIn 0.4s ease;
        }
        .post:hover { border-color: var(--border-hover); }
        @keyframes fadeIn { from{opacity:0;transform:translateY(-6px)} to{opacity:1;transform:translateY(0)} }
        .post-header { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; }
        .avatar {
            width: 38px; height: 38px; border-radius: 50%;
            background: linear-gradient(135deg, #ffc107, #ff6f00);
            display: flex; align-items: center; justify-content: center;
            font-size: 16px; font-weight: 700; color: #000;
        }
        .post-meta { flex: 1; }
        .post-author { font-weight: 600; font-size: 14px; color: var(--text); }
        .post-time { font-size: 11px; color: var(--text-muted); }
        .post-type-badge {
            padding: 2px 8px; border-radius: 10px; font-size: 10px;
            font-weight: 600; text-transform: uppercase;
        }
        .badge-text { background: rgba(66,165,245,0.15); color: var(--blue); }
        .badge-link { background: rgba(76,175,80,0.15); color: var(--green); }
        .badge-alert { background: rgba(239,83,80,0.15); color: var(--red); }
        .badge-resource { background: var(--amber-glow); color: var(--amber); }
        .badge-ai { background: rgba(171,71,188,0.15); color: #ab47bc; }
        .post-content { font-size: 14px; line-height: 1.7; color: var(--text); white-space: pre-wrap; word-break: break-word; }
        .post-link {
            display: block; margin-top: 10px; padding: 10px 14px;
            background: rgba(255,255,255,0.03); border: 1px solid var(--border);
            border-radius: var(--radius-sm); color: var(--blue); text-decoration: none;
            font-size: 13px; transition: all 0.2s;
        }
        .post-link:hover { border-color: var(--blue); background: rgba(66,165,245,0.08); }
        .post-actions {
            display: flex; gap: 4px; margin-top: 12px; padding-top: 10px;
            border-top: 1px solid var(--border);
        }
        .action-btn {
            padding: 6px 12px; border-radius: 8px; border: none;
            background: transparent; color: var(--text-dim); cursor: pointer;
            font-size: 12px; font-family: inherit; transition: all 0.2s;
            display: flex; align-items: center; gap: 4px;
        }
        .action-btn:hover { background: rgba(255,255,255,0.06); color: var(--text); }
        .action-count { font-weight: 600; }

        /* ── Replies ── */
        .replies { margin-top: 12px; padding-left: 20px; border-left: 2px solid var(--border); }
        .reply { padding: 10px 0; }
        .reply .post-author { font-size: 12px; }
        .reply .post-content { font-size: 13px; }

        /* ── Sidebar Cards ── */
        .sidebar-card {
            background: var(--card); border: 1px solid var(--border);
            border-radius: var(--radius-sm); padding: 14px; margin-bottom: 12px;
        }
        .sidebar-card h3 { font-size: 13px; font-weight: 600; color: var(--amber); margin-bottom: 10px; }
        .sidebar-item {
            padding: 6px 0; font-size: 12px; color: var(--text-dim);
            display: flex; justify-content: space-between; cursor: pointer;
        }
        .sidebar-item:hover { color: var(--amber); }

        /* ── Right Bar ── */
        .pool-card {
            background: linear-gradient(135deg, rgba(255,193,7,0.08), rgba(255,152,0,0.04));
            border: 1px solid rgba(255,193,7,0.15);
            border-radius: var(--radius-sm); padding: 14px; margin-bottom: 12px;
        }
        .pool-card h3 { font-size: 13px; font-weight: 600; color: var(--amber); margin-bottom: 8px; }
        .pool-stat { display: flex; justify-content: space-between; padding: 4px 0; font-size: 12px; }
        .pool-stat span:first-child { color: var(--text-dim); }
        .pool-stat span:last-child { color: var(--text); font-weight: 500; }

        /* ── Mesh Banner ── */
        .mesh-banner {
            background: linear-gradient(135deg, rgba(239,83,80,0.15), rgba(255,152,0,0.1));
            border: 1px solid rgba(239,83,80,0.3); border-radius: var(--radius-sm);
            padding: 12px 16px; margin-bottom: 16px; text-align: center;
            font-size: 13px; color: var(--red); display: none;
        }
        .mesh-banner.visible { display: block; }

        /* ── Empty State ── */
        .empty { text-align: center; padding: 60px 20px; color: var(--text-muted); }
        .empty .icon { font-size: 48px; margin-bottom: 16px; }

        /* ── Search ── */
        .search-bar {
            display: flex; gap: 8px; padding: 16px 0;
        }
        .search-bar input {
            flex: 1; padding: 10px 16px; border-radius: 24px;
            border: 1px solid var(--border); background: var(--card);
            color: var(--text); font-family: inherit; font-size: 13px; outline: none;
        }
        .search-bar input:focus { border-color: var(--amber); }
    </style>
</head>
<body>
    <!-- Status Bar -->
    <div class="status-bar">
        <div class="logo">
            <span style="font-size: 24px;">🐝</span>
            <h1>HiveSurface</h1>
        </div>
        <div class="mesh-stats">
            <div class="stat-pill"><div class="dot" id="connectivity-dot"></div><span id="connectivity-text">checking...</span></div>
            <div class="stat-pill">👥 <span id="peer-count">0</span> peers</div>
            <div class="stat-pill">🖥️ <span id="compute-slots">0</span> compute</div>
            <div class="stat-pill">🌐 <span id="relay-count">0</span> relays</div>
        </div>
    </div>

    <!-- Nav -->
    <div class="nav-bar">
        <button class="nav-btn active" data-view="feed">📰 Feed</button>
        <button class="nav-btn" data-view="trending">🔥 Trending</button>
        <button class="nav-btn" data-view="communities">🏘️ Communities</button>
        <button class="nav-btn" data-view="search">🔍 Search</button>
        <button class="nav-btn" data-view="alerts">🚨 Alerts</button>
    </div>

    <div class="app">
        <!-- Sidebar -->
        <div class="sidebar">
            <div class="sidebar-card">
                <h3>🏘️ Communities</h3>
                <div id="sidebar-communities"><div style="font-size:12px;color:var(--text-muted)">Loading...</div></div>
            </div>
            <div class="sidebar-card">
                <h3>🌐 About</h3>
                <p style="font-size:12px;color:var(--text-dim);line-height:1.6">
                    HiveSurface is the decentralised web. Every peer is a server. No corporations, no censorship. If the internet goes down, the mesh keeps you connected.
                </p>
            </div>
        </div>

        <!-- Main Feed -->
        <div class="main">
            <div class="mesh-banner" id="mesh-banner">
                📡 <strong>MESH ONLY MODE</strong> — Internet unavailable. Connected via peer relay.
            </div>

            <!-- Composer -->
            <div class="composer" id="composer-section">
                <textarea id="post-input" placeholder="What's on your mind? Share with the mesh..."></textarea>
                <div class="composer-actions">
                    <div class="composer-tools">
                        <button class="tool-btn" onclick="setPostType('text')" id="btn-text">📝 Text</button>
                        <button class="tool-btn" onclick="setPostType('link')" id="btn-link">🔗 Link</button>
                        <button class="tool-btn" onclick="setPostType('resource')" id="btn-resource">📡 Resource</button>
                    </div>
                    <button class="post-btn" onclick="createPost()">Post to Mesh ▶</button>
                </div>
                <input type="text" id="link-input" placeholder="Paste URL..." style="display:none;width:100%;margin-top:10px;padding:8px 14px;border-radius:8px;border:1px solid var(--border);background:var(--card);color:var(--text);font-family:inherit;font-size:13px;outline:none;">
                <input type="text" id="community-input" placeholder="Community (optional, e.g. tech, news, survival)" style="width:100%;margin-top:8px;padding:8px 14px;border-radius:8px;border:1px solid var(--border);background:var(--card);color:var(--text);font-family:inherit;font-size:13px;outline:none;">
            </div>

            <!-- Search (hidden by default) -->
            <div class="search-bar" id="search-section" style="display:none;">
                <input type="text" id="search-input" placeholder="Search the mesh..." onkeydown="if(event.key==='Enter')doSearch()">
                <button class="post-btn" onclick="doSearch()">Search</button>
            </div>

            <div id="feed-container"></div>
        </div>

        <!-- Right Bar -->
        <div class="right-bar">
            <div class="pool-card">
                <h3>⚡ Mesh Resources</h3>
                <div class="pool-stat"><span>Connectivity</span><span id="rb-connectivity">—</span></div>
                <div class="pool-stat"><span>Web Relays</span><span id="rb-relays">0</span></div>
                <div class="pool-stat"><span>Compute Nodes</span><span id="rb-compute">0</span></div>
                <div class="pool-stat"><span>Compute Slots</span><span id="rb-slots">0</span></div>
                <div class="pool-stat"><span>Web Sharing</span><span id="rb-web-share">—</span></div>
                <div class="pool-stat"><span>Compute Sharing</span><span id="rb-compute-share">—</span></div>
            </div>
            <div class="sidebar-card">
                <h3>🔥 Trending Now</h3>
                <div id="rb-trending"><div style="font-size:12px;color:var(--text-muted)">Loading...</div></div>
            </div>
        </div>
    </div>

    <script>
    let currentView = 'feed';
    let currentPostType = 'text';
    let liveCount = 0;

    // ── Status Polling ──
    async function updateStatus() {
        try {
            const res = await fetch('/api/status');
            const data = await res.json();
            const dot = document.getElementById('connectivity-dot');
            const text = document.getElementById('connectivity-text');
            const banner = document.getElementById('mesh-banner');

            if (data.clearnet_available) {
                dot.className = 'dot dot-green';
                text.textContent = 'online';
                banner.classList.remove('visible');
            } else {
                dot.className = 'dot dot-red';
                text.textContent = 'mesh only';
                banner.classList.add('visible');
            }

            const peers = (data.web_relays || 0) + (data.compute_nodes || 0);
            document.getElementById('peer-count').textContent = peers;
            document.getElementById('compute-slots').textContent = data.total_compute_slots || 0;
            document.getElementById('relay-count').textContent = data.web_relays || 0;

            document.getElementById('rb-connectivity').textContent = data.clearnet_available ? '🟢 Online' : '📡 Mesh';
            document.getElementById('rb-relays').textContent = data.web_relays || 0;
            document.getElementById('rb-compute').textContent = data.compute_nodes || 0;
            document.getElementById('rb-slots').textContent = data.total_compute_slots || 0;
            document.getElementById('rb-web-share').textContent = data.web_share_enabled ? '✅ Active' : '❌ Off';
            document.getElementById('rb-compute-share').textContent = data.compute_share_enabled ? '✅ Active' : '❌ Off';
        } catch(e) {}
    }

    // ── Feed ──
    async function loadFeed() {
        const res = await fetch('/api/feed?limit=50');
        const data = await res.json();
        renderPosts(data.posts);
    }

    async function loadTrending() {
        const res = await fetch('/api/trending');
        const data = await res.json();
        renderPosts(data.posts);
    }

    async function loadCommunities() {
        const res = await fetch('/api/communities');
        const data = await res.json();
        const container = document.getElementById('feed-container');
        if (!data.communities || data.communities.length === 0) {
            container.innerHTML = '<div class="empty"><div class="icon">🏘️</div><p>No communities yet. Create a post with a community tag!</p></div>';
            return;
        }
        container.innerHTML = data.communities.map(c => `
            <div class="post" onclick="loadCommunityFeed('${esc(c.name)}')" style="cursor:pointer">
                <div class="post-header">
                    <div class="avatar">🏘️</div>
                    <div class="post-meta">
                        <div class="post-author">${esc(c.name)}</div>
                        <div class="post-time">${c.post_count} posts</div>
                    </div>
                </div>
            </div>
        `).join('');
    }

    async function loadCommunityFeed(name) {
        const res = await fetch(`/api/feed?community=${encodeURIComponent(name)}&limit=50`);
        const data = await res.json();
        renderPosts(data.posts);
    }

    async function doSearch() {
        const q = document.getElementById('search-input').value.trim();
        if (!q) return;
        const res = await fetch(`/api/search?q=${encodeURIComponent(q)}&limit=50`);
        const data = await res.json();
        renderPosts(data.posts);
    }

    async function loadAlerts() {
        const res = await fetch('/api/alerts');
        const data = await res.json();
        const container = document.getElementById('feed-container');
        if (!data.alerts || data.alerts.length === 0) {
            container.innerHTML = '<div class="empty"><div class="icon">✅</div><p>No active alerts. The mesh is healthy.</p></div>';
            return;
        }
        container.innerHTML = data.alerts.map(a => `<div class="post" style="border-color:rgba(239,83,80,0.3)">
            <div class="post-content">🚨 ${esc(JSON.stringify(a))}</div>
        </div>`).join('');
    }

    // ── Render ──
    function renderPosts(posts) {
        const container = document.getElementById('feed-container');
        if (!posts || posts.length === 0) {
            container.innerHTML = '<div class="empty"><div class="icon">📭</div><p>No posts yet. Be the first to share!</p></div>';
            return;
        }
        container.innerHTML = posts.map(renderPost).join('');
    }

    function renderPost(p) {
        const icon = typeIcon(p.post_type);
        const badge = typeBadge(p.post_type);
        const time = timeAgo(p.created_at);
        const initial = (p.author_name || '?')[0].toUpperCase();
        const reactions = Object.entries(p.reactions || {}).map(([emoji, voters]) =>
            `<button class="action-btn" onclick="react('${p.id}','${emoji}')">
                ${emoji} <span class="action-count">${voters.length}</span>
            </button>`
        ).join('');
        const link = p.link_url ? `<a class="post-link" href="${esc(p.link_url)}" target="_blank">🔗 ${esc(p.link_url)}</a>` : '';
        const replies = (p.replies || []).map(r => `
            <div class="reply">
                <span class="post-author">${esc(r.author_name)}</span>
                <span class="post-time" style="margin-left:8px">${timeAgo(r.created_at)}</span>
                <div class="post-content">${esc(r.content)}</div>
            </div>
        `).join('');
        const replySection = replies ? `<div class="replies">${replies}</div>` : '';

        return `<div class="post">
            <div class="post-header">
                <div class="avatar">${initial}</div>
                <div class="post-meta">
                    <div class="post-author">${esc(p.author_name)} ${badge}</div>
                    <div class="post-time">${time}${p.community ? ' · 🏘️ ' + esc(p.community) : ''}</div>
                </div>
            </div>
            <div class="post-content">${esc(p.content)}</div>
            ${link}
            <div class="post-actions">
                ${reactions}
                <button class="action-btn" onclick="react('${p.id}','👍')">👍</button>
                <button class="action-btn" onclick="react('${p.id}','❤️')">❤️</button>
                <button class="action-btn" onclick="react('${p.id}','🔥')">🔥</button>
                <button class="action-btn" onclick="promptReply('${p.id}')">💬 ${p.reply_count || 0}</button>
            </div>
            ${replySection}
        </div>`;
    }

    function typeIcon(t) { return {text:'📝',link:'🔗',alert:'🚨',resource:'📡',ai:'🤖'}[t]||'📝'; }
    function typeBadge(t) {
        const cls = {text:'badge-text',link:'badge-link',alert:'badge-alert',resource:'badge-resource',ai:'badge-ai'}[t]||'badge-text';
        return `<span class="post-type-badge ${cls}">${t}</span>`;
    }
    function timeAgo(ts) {
        const s = Math.floor((Date.now() - new Date(ts)) / 1000);
        if (s < 60) return 'just now';
        if (s < 3600) return Math.floor(s/60) + 'm ago';
        if (s < 86400) return Math.floor(s/3600) + 'h ago';
        return Math.floor(s/86400) + 'd ago';
    }
    function esc(t) { if (!t) return ''; const d=document.createElement('div'); d.textContent=t; return d.innerHTML; }

    // ── Actions ──
    async function createPost() {
        const content = document.getElementById('post-input').value.trim();
        if (!content) return;
        const body = { content, post_type: currentPostType };
        const link = document.getElementById('link-input').value.trim();
        if (link) body.link_url = link;
        const community = document.getElementById('community-input').value.trim();
        if (community) body.community = community;

        const res = await fetch('/api/post', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify(body) });
        const data = await res.json();
        if (data.ok) {
            document.getElementById('post-input').value = '';
            document.getElementById('link-input').value = '';
            document.getElementById('community-input').value = '';
            loadFeed();
        } else {
            alert(data.error || 'Failed to post');
        }
    }

    async function react(postId, emoji) {
        await fetch(`/api/post/${postId}/react`, { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({emoji}) });
        loadView(currentView);
    }

    function promptReply(postId) {
        const content = prompt('Your reply:');
        if (content && content.trim()) {
            fetch(`/api/post/${postId}/reply`, { method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({content}) })
                .then(() => loadView(currentView));
        }
    }

    function setPostType(type) {
        currentPostType = type;
        document.querySelectorAll('.tool-btn').forEach(b => b.classList.remove('active'));
        document.getElementById('btn-' + type)?.classList.add('active');
        document.getElementById('link-input').style.display = type === 'link' ? 'block' : 'none';
    }

    // ── Navigation ──
    function loadView(view) {
        currentView = view;
        document.querySelectorAll('.nav-btn').forEach(b => b.classList.remove('active'));
        document.querySelector(`[data-view="${view}"]`)?.classList.add('active');

        document.getElementById('composer-section').style.display = (view==='feed'||view==='communities') ? 'block' : 'none';
        document.getElementById('search-section').style.display = view==='search' ? 'flex' : 'none';

        if (view === 'feed') loadFeed();
        else if (view === 'trending') loadTrending();
        else if (view === 'communities') loadCommunities();
        else if (view === 'search') { document.getElementById('feed-container').innerHTML = '<div class="empty"><div class="icon">🔍</div><p>Search the mesh...</p></div>'; }
        else if (view === 'alerts') loadAlerts();
    }

    document.querySelectorAll('.nav-btn').forEach(btn => {
        btn.addEventListener('click', () => loadView(btn.dataset.view));
    });

    // ── SSE Live Stream ──
    const evtSource = new EventSource('/api/stream');
    evtSource.onmessage = (event) => {
        if (currentView !== 'feed') return;
        try {
            const post = JSON.parse(event.data);
            const container = document.getElementById('feed-container');
            const empty = container.querySelector('.empty');
            if (empty) empty.remove();
            container.insertAdjacentHTML('afterbegin', renderPost(post));
        } catch(e) {}
    };

    // ── Sidebar communities ──
    async function loadSidebarCommunities() {
        try {
            const res = await fetch('/api/communities');
            const data = await res.json();
            const el = document.getElementById('sidebar-communities');
            if (!data.communities || data.communities.length === 0) {
                el.innerHTML = '<div class="sidebar-item" style="color:var(--text-muted)">None yet</div>';
                return;
            }
            el.innerHTML = data.communities.slice(0, 8).map(c =>
                `<div class="sidebar-item" onclick="loadCommunityFeed('${esc(c.name)}')">
                    <span>🏘️ ${esc(c.name)}</span><span>${c.post_count}</span>
                </div>`
            ).join('');
        } catch(e) {}
    }

    async function loadRightBarTrending() {
        try {
            const res = await fetch('/api/trending');
            const data = await res.json();
            const el = document.getElementById('rb-trending');
            if (!data.posts || data.posts.length === 0) {
                el.innerHTML = '<div style="font-size:12px;color:var(--text-muted)">No trending posts</div>';
                return;
            }
            el.innerHTML = data.posts.slice(0, 5).map(p =>
                `<div class="sidebar-item">
                    <span>${esc(p.content.substring(0,40))}...</span>
                    <span>${p.reply_count || 0} 💬</span>
                </div>`
            ).join('');
        } catch(e) {}
    }

    // ── Boot ──
    updateStatus();
    loadFeed();
    loadSidebarCommunities();
    loadRightBarTrending();
    setInterval(updateStatus, 15000);
    setInterval(loadSidebarCommunities, 60000);
    setPostType('text');
    </script>
</body>
</html>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spa_html_not_empty() {
        assert!(SPA_HTML.len() > 1000);
        assert!(SPA_HTML.contains("HiveSurface"));
        assert!(SPA_HTML.contains("/api/feed"));
        assert!(SPA_HTML.contains("/api/status"));
    }
}
