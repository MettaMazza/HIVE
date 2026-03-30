/// Web Proxy — Local censorship-resistant proxy for the SafeNet mesh.
///
/// Routes web traffic through the mesh when clearnet is unavailable.
/// Runs on localhost:{HIVE_WEB_PROXY_PORT} (default 8480).
///
/// Modes:
/// - Normal: Direct HTTP/HTTPS pass-through (no mesh involvement)
/// - Mesh relay: When clearnet is blocked, route through peers who still have access
/// - Cache: Serve popular pages from mesh peer caches (opt-in)
///
/// SURVIVABILITY: If this peer has internet, it can relay for others.
/// If this peer has no internet, it can request relay from others.
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use axum::{
    routing::{get, post},
    Router, Json,
    extract::{State, Query},
    response::Html,
};
use tower_http::cors::CorsLayer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::network::transport::QuicTransport;

/// Content cache entry — a cached web page or file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub url: String,
    pub content_type: String,
    pub body: Vec<u8>,
    pub cached_at: String,
    pub size_bytes: usize,
    pub ttl_secs: u64,
}

/// Web proxy state.
#[derive(Clone)]
struct ProxyState {
    /// Shared reference to content cache
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// Whether mesh relay mode is enabled
    mesh_relay_enabled: bool,
    /// DoH (DNS-over-HTTPS) resolver URL
    doh_resolver: String,
    /// Whether direct clearnet access is available
    clearnet_available: Arc<RwLock<bool>>,
    /// Transport handle for mesh relay requests
    transport: Option<Arc<QuicTransport>>,
    /// Pool manager for relay peer selection
    pool: Option<Arc<crate::network::pool::PoolManager>>,
}

/// Configuration for the web proxy.
pub struct WebProxyConfig {
    pub port: u16,
    pub mesh_relay_enabled: bool,
    pub doh_resolver: String,
    pub cache_enabled: bool,
    pub max_cache_entries: usize,
}

impl WebProxyConfig {
    /// Load from environment variables.
    pub fn from_env() -> Self {
        Self {
            port: std::env::var("HIVE_WEB_PROXY_PORT")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(8480),
            mesh_relay_enabled: std::env::var("HIVE_WEB_PROXY_MESH_RELAY")
                .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
                .unwrap_or(true), // ON by default — equality
            doh_resolver: std::env::var("HIVE_DOH_RESOLVER")
                .unwrap_or_else(|_| "https://cloudflare-dns.com/dns-query".to_string()),
            cache_enabled: std::env::var("HIVE_WEB_PROXY_CACHE")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(true),
            max_cache_entries: std::env::var("HIVE_WEB_PROXY_MAX_CACHE")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(200),
        }
    }
}

/// Health check response for the proxy.
#[derive(Serialize)]
struct ProxyStatus {
    clearnet_available: bool,
    mesh_relay_enabled: bool,
    cache_entries: usize,
    doh_resolver: String,
    relay_peers: usize,
}

/// Proxy fetch request.
#[derive(Deserialize)]
struct FetchRequest {
    url: String,
    #[serde(default)]
    force_mesh: bool,
}

/// Proxy fetch response.
#[derive(Serialize)]
struct FetchResponse {
    url: String,
    status: u16,
    content_type: String,
    body: String,
    source: String, // "clearnet", "mesh_relay", or "cache"
    latency_ms: u64,
}

/// Spawn the web proxy server.
pub async fn spawn_web_proxy(
    config: WebProxyConfig,
    transport: Option<Arc<QuicTransport>>,
    pool: Option<Arc<crate::network::pool::PoolManager>>,
) {
    let port = config.port;

    let state = ProxyState {
        cache: Arc::new(RwLock::new(HashMap::new())),
        mesh_relay_enabled: config.mesh_relay_enabled,
        doh_resolver: config.doh_resolver,
        clearnet_available: Arc::new(RwLock::new(true)),
        transport,
        pool,
    };

    let health_state = state.clone();

    tokio::spawn(async move {
        tracing::info!("[WEB PROXY] 🌐 Starting on http://127.0.0.1:{}", port);

        let app = Router::new()
            .route("/", get(proxy_dashboard))
            .route("/api/status", get(proxy_status))
            .route("/api/fetch", post(proxy_fetch))
            .route("/api/cache", get(proxy_cache_list))
            .route("/api/dns", get(doh_resolve))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let addr = format!("127.0.0.1:{}", port);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                tracing::info!("[WEB PROXY] 🌐 Bound on {}", addr);
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("[WEB PROXY] ❌ Server error: {}", e);
                }
            }
            Err(e) => {
                tracing::error!("[WEB PROXY] ❌ Failed to bind on {}: {}", addr, e);
            }
        }
    });

    // Spawn clearnet health check (every 30 seconds)
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let available = client.get("https://1.1.1.1/cdn-cgi/trace")
                .send().await.is_ok();

            *health_state.clearnet_available.write().await = available;

            if !available {
                tracing::warn!("[WEB PROXY] ⚠️ Clearnet down — mesh relay active");
            }
        }
    });
}

// ─── Endpoints ──────────────────────────────────────────────────────────

async fn proxy_status(State(state): State<ProxyState>) -> Json<ProxyStatus> {
    let clearnet = *state.clearnet_available.read().await;
    let cache_count = state.cache.read().await.len();
    let relay_peers = if let Some(pool) = &state.pool {
        pool.web_pool.read().await.relay_count()
    } else { 0 };

    Json(ProxyStatus {
        clearnet_available: clearnet,
        mesh_relay_enabled: state.mesh_relay_enabled,
        cache_entries: cache_count,
        doh_resolver: state.doh_resolver.clone(),
        relay_peers,
    })
}

async fn proxy_fetch(
    State(state): State<ProxyState>,
    Json(req): Json<FetchRequest>,
) -> Json<FetchResponse> {
    let start = std::time::Instant::now();

    // 1. Check cache first
    {
        let cache = state.cache.read().await;
        if let Some(entry) = cache.get(&req.url) {
            let elapsed = start.elapsed().as_millis() as u64;
            return Json(FetchResponse {
                url: req.url.clone(),
                status: 200,
                content_type: entry.content_type.clone(),
                body: String::from_utf8_lossy(&entry.body).to_string(),
                source: "cache".to_string(),
                latency_ms: elapsed,
            });
        }
    }

    // 2. Try clearnet (unless force_mesh is set)
    if !req.force_mesh {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        match client.get(&req.url).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let content_type = resp.headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("text/html")
                    .to_string();
                let body_bytes = resp.bytes().await.unwrap_or_default();
                let elapsed = start.elapsed().as_millis() as u64;

                // Cache the response
                if status == 200 && body_bytes.len() < 5_000_000 {
                    let entry = CacheEntry {
                        url: req.url.clone(),
                        content_type: content_type.clone(),
                        body: body_bytes.to_vec(),
                        cached_at: chrono::Utc::now().to_rfc3339(),
                        size_bytes: body_bytes.len(),
                        ttl_secs: 300,
                    };
                    state.cache.write().await.insert(req.url.clone(), entry);
                }

                *state.clearnet_available.write().await = true;

                return Json(FetchResponse {
                    url: req.url.clone(),
                    status,
                    content_type,
                    body: String::from_utf8_lossy(&body_bytes).to_string(),
                    source: "clearnet".to_string(),
                    latency_ms: elapsed,
                });
            }
            Err(e) => {
                tracing::warn!("[WEB PROXY] Clearnet fetch failed for {}: {}", req.url, e);
                *state.clearnet_available.write().await = false;
            }
        }
    }

    // 3. Fall back to mesh relay via PoolManager
    if state.mesh_relay_enabled {
        if let Some(pool) = &state.pool {
            match pool.request_web_relay(&req.url).await {
                Ok(relay_peer) => {
                    tracing::info!("[WEB PROXY] 📡 Mesh relay for '{}' via peer {}",
                        &req.url[..req.url.len().min(60)], relay_peer);

                    // Send RelayRequest via QUIC transport to the selected peer
                    if let Some(transport) = &state.transport {
                        let relay_msg = crate::network::messages::MeshMessage::RelayRequest {
                            destination_url: req.url.clone(),
                            requester: crate::network::pool::PoolManager::ephemeral_id(),
                        };
                        let payload = rmp_serde::to_vec(&relay_msg).unwrap_or_default();
                        let envelope = crate::network::messages::SignedEnvelope {
                            sender: crate::network::pool::PoolManager::ephemeral_id(),
                            payload,
                            signature: vec![],
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        };
                        transport.broadcast(&envelope).await;

                        let elapsed = start.elapsed().as_millis() as u64;
                        return Json(FetchResponse {
                            url: req.url.clone(),
                            status: 202,
                            content_type: "text/plain".to_string(),
                            body: format!("MESH_RELAY: Request dispatched to peer {} via QUIC", relay_peer),
                            source: "mesh_relay".to_string(),
                            latency_ms: elapsed,
                        });
                    }

                    // Transport not yet connected — peer selected, relay queued
                    let elapsed = start.elapsed().as_millis() as u64;
                    return Json(FetchResponse {
                        url: req.url.clone(),
                        status: 202,
                        content_type: "text/plain".to_string(),
                        body: format!("MESH_RELAY: Peer {} selected — transport connecting", relay_peer),
                        source: "mesh_relay".to_string(),
                        latency_ms: elapsed,
                    });
                }
                Err(e) => {
                    tracing::warn!("[WEB PROXY] Relay selection failed: {}", e);
                }
            }
        }
    }

    // 4. Total failure — no clearnet, no relays
    let elapsed = start.elapsed().as_millis() as u64;
    Json(FetchResponse {
        url: req.url,
        status: 503,
        content_type: "text/plain".to_string(),
        body: "No clearnet access and no mesh relay peers available.".to_string(),
        source: "unavailable".to_string(),
        latency_ms: elapsed,
    })
}

async fn proxy_cache_list(State(state): State<ProxyState>) -> Json<serde_json::Value> {
    let cache = state.cache.read().await;
    let entries: Vec<serde_json::Value> = cache.values().map(|e| {
        serde_json::json!({
            "url": e.url,
            "content_type": e.content_type,
            "size_bytes": e.size_bytes,
            "cached_at": e.cached_at,
        })
    }).collect();

    Json(serde_json::json!({
        "entries": entries,
        "count": entries.len(),
    }))
}

#[derive(Deserialize)]
struct DnsQuery {
    name: String,
}

async fn doh_resolve(
    State(state): State<ProxyState>,
    Query(query): Query<DnsQuery>,
) -> Json<serde_json::Value> {
    // DNS-over-HTTPS query to bypass DNS poisoning
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let doh_url = format!("{}?name={}&type=A", state.doh_resolver, query.name);

    match client.get(&doh_url)
        .header("Accept", "application/dns-json")
        .send()
        .await
    {
        Ok(resp) => {
            match resp.json::<serde_json::Value>().await {
                Ok(dns_response) => Json(dns_response),
                Err(_) => Json(serde_json::json!({"error": "Failed to parse DNS response"})),
            }
        }
        Err(e) => Json(serde_json::json!({
            "error": format!("DoH query failed: {}", e),
            "fallback": "Use mesh DNS if available"
        })),
    }
}

async fn proxy_dashboard() -> Html<String> {
    Html(PROXY_DASHBOARD.to_string())
}

use super::web_proxy_html::PROXY_DASHBOARD;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = WebProxyConfig::from_env();
        assert_eq!(config.port, 8480);
        assert!(!config.mesh_relay_enabled);
        assert!(config.cache_enabled);
        assert_eq!(config.max_cache_entries, 200);
    }

    #[test]
    fn test_cache_entry() {
        let entry = CacheEntry {
            url: "https://example.com".to_string(),
            content_type: "text/html".to_string(),
            body: b"<html>test</html>".to_vec(),
            cached_at: chrono::Utc::now().to_rfc3339(),
            size_bytes: 17,
            ttl_secs: 300,
        };
        assert_eq!(entry.size_bytes, 17);
    }
}
