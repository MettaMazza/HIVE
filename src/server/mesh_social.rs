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
    identity: Arc<crate::network::identity::MeshIdentity>,
}

/// Read the current display name from the shared identity store.
async fn get_display_name_from_identity(identity: &crate::network::identity::MeshIdentity) -> String {
    identity.display_name().await
}

/// Sync fallback for backward compat.
#[allow(dead_code)]
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

#[derive(Deserialize)]
struct EditPostReq {
    content: String,
}

pub async fn spawn_mesh_social_server(post_store: Arc<PostStore>, identity: Arc<crate::network::identity::MeshIdentity>) {
    let port: u16 = 3032; // Mesh-governed: creator-key protected

    let local_peer_id = identity.peer_id().await;

    let state = SurfaceState {
        post_store,
        local_peer_id,
        identity,
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
            .route("/api/post/{post_id}/edit", axum::routing::put(api_edit_post))
            .route("/api/post/{post_id}/delete", axum::routing::delete(api_delete_post))
            .route("/api/post/{post_id}/share", post(api_share_post))
            .route("/api/follow/{peer_id}", post(api_follow))
            .route("/api/unfollow/{peer_id}", post(api_unfollow))
            .route("/api/following", get(api_following))
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
    let clearnet = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build().unwrap_or_default()
        .get("https://1.1.1.1/cdn-cgi/trace")
        .send().await.is_ok();

    // Only report real connected peers — never fabricate
    Json(json!({
        "clearnet_available": clearnet,
        "connectivity": if clearnet { "online" } else { "mesh_only" },
        "web_relays": 0,
        "compute_nodes": 0,
        "total_compute_slots": 0,
        "web_share_enabled": true,
        "compute_share_enabled": true,
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
        &get_display_name_from_identity(&state.identity).await,
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
        &get_display_name_from_identity(&state.identity).await,
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
    let profile = state.identity.profile().await;
    let is_local = peer_id == state.local_peer_id;
    Json(json!({
        "peer_id": peer_id,
        "display_name": if is_local { profile.display_name } else { peer_id.clone() },
        "avatar_url": if is_local { profile.avatar_url } else { None::<String> },
        "bio": if is_local { profile.bio } else { None::<String> },
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

// ─── New P0 Endpoints ───────────────────────────────────────────────────

async fn api_edit_post(
    State(state): State<SurfaceState>,
    Path(post_id): Path<String>,
    Json(req): Json<EditPostReq>,
) -> Json<Value> {
    if req.content.trim().is_empty() {
        return Json(json!({"error": "Content cannot be empty"}));
    }
    match state.post_store.edit_post(&post_id, &state.local_peer_id, &req.content).await {
        Some(post) => Json(json!({"ok": true, "post": post})),
        None => Json(json!({"error": "Post not found or not yours"})),
    }
}

async fn api_delete_post(
    State(state): State<SurfaceState>,
    Path(post_id): Path<String>,
) -> Json<Value> {
    let deleted = state.post_store.delete_post(&post_id, &state.local_peer_id).await;
    Json(json!({"ok": deleted}))
}

async fn api_share_post(
    State(state): State<SurfaceState>,
    Path(post_id): Path<String>,
) -> Json<Value> {
    let name = get_display_name_from_identity(&state.identity).await;
    match state.post_store.share_post(&post_id, &state.local_peer_id, &name).await {
        Some(post) => Json(json!({"ok": true, "post": post})),
        None => Json(json!({"error": "Post not found"})),
    }
}

async fn api_follow(
    State(state): State<SurfaceState>,
    Path(peer_id): Path<String>,
) -> Json<Value> {
    if peer_id == state.local_peer_id {
        return Json(json!({"error": "Cannot follow yourself"}));
    }
    state.post_store.follow(&state.local_peer_id, &peer_id).await;
    Json(json!({"ok": true, "following": peer_id}))
}

async fn api_unfollow(
    State(state): State<SurfaceState>,
    Path(peer_id): Path<String>,
) -> Json<Value> {
    let removed = state.post_store.unfollow(&state.local_peer_id, &peer_id).await;
    Json(json!({"ok": removed}))
}

async fn api_following(State(state): State<SurfaceState>) -> Json<Value> {
    let following = state.post_store.following(&state.local_peer_id).await;
    Json(json!({"following": following, "count": following.len()}))
}

// ─── SPA Frontend ───────────────────────────────────────────────────────

async fn serve_spa() -> Html<String> {
    Html(SPA_HTML.to_string())
}

use super::mesh_social_html::SPA_HTML;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::post_store::{PostStore, PostType};

    #[test]
    fn test_spa_html_not_empty() {
        assert!(SPA_HTML.len() > 1000);
        assert!(SPA_HTML.contains("HiveSurface"));
        assert!(SPA_HTML.contains("/api/feed"));
        assert!(SPA_HTML.contains("/api/status"));
    }

    #[tokio::test]
    async fn test_post_edit() {
        let store = PostStore::new();
        let mut post = crate::network::post_store::MeshPost::new("peer1", "Alice", "Original", PostType::Text);
        post.media_urls = vec!["http://localhost:8421/file/test.png".to_string()];
        store.push(post.clone()).await;

        let edited = store.edit_post(&post.id, "peer1", "Edited content").await;
        assert!(edited.is_some());
        assert_eq!(edited.unwrap().content, "Edited content");

        // Non-author blocked
        assert!(store.edit_post(&post.id, "peer2", "Hack").await.is_none());
    }

    #[tokio::test]
    async fn test_post_delete() {
        let store = PostStore::new();
        let post = crate::network::post_store::MeshPost::new("peer1", "Alice", "Delete me", PostType::Text);
        let id = post.id.clone();
        store.push(post).await;
        let before = store.count().await;

        assert!(!store.delete_post(&id, "peer2").await); // non-author
        assert!(store.delete_post(&id, "peer1").await);
        assert_eq!(store.count().await, before - 1);
    }

    #[tokio::test]
    async fn test_post_share() {
        let store = PostStore::new();
        let post = crate::network::post_store::MeshPost::new("peer1", "Alice", "Share this", PostType::Text);
        let id = post.id.clone();
        store.push(post).await;

        let shared = store.share_post(&id, "peer2", "Bob").await;
        assert!(shared.is_some());
        let s = shared.unwrap();
        assert_eq!(s.shared_from.unwrap(), id);
        assert_eq!(s.author_id, "peer2");
    }

    #[tokio::test]
    async fn test_follow_system() {
        let store = PostStore::new();

        store.follow("alice", "bob").await;
        assert!(store.is_following("alice", "bob").await);
        assert!(!store.is_following("bob", "alice").await);

        let following = store.following("alice").await;
        assert_eq!(following.len(), 1);

        assert!(store.unfollow("alice", "bob").await);
        assert!(!store.is_following("alice", "bob").await);
        assert!(!store.unfollow("alice", "bob").await); // double unfollow
    }

    #[tokio::test]
    async fn test_follow_feed() {
        let store = PostStore::new();

        // Bob posts
        let post = crate::network::post_store::MeshPost::new("bob", "Bob", "Hello from Bob", PostType::Text);
        store.push(post).await;

        // Alice follows Bob
        store.follow("alice", "bob").await;

        let feed = store.by_follows("alice", 50).await;
        assert!(!feed.is_empty());
        assert!(feed.iter().all(|p| p.author_id == "bob"));

        // Charlie (not following) gets empty feed
        let empty = store.by_follows("charlie", 50).await;
        assert!(empty.is_empty());
    }
}
