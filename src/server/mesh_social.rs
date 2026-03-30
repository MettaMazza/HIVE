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
}

/// Read the current display name dynamically (reflects identity changes without restart).
fn get_display_name() -> String {
    std::env::var("HIVE_USER_NAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "Anonymous".to_string())
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

    let state = SurfaceState {
        post_store,
        local_peer_id,
    };

    tokio::spawn(async move {
        tracing::info!("[SURFACE] 🌐 HiveSurface starting on http://0.0.0.0:{}", port);

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

        let addr = format!("0.0.0.0:{}", port);
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
        &get_display_name(),
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
        &get_display_name(),
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

use super::mesh_social_html::SPA_HTML;

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
