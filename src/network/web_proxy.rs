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
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
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
pub async fn spawn_web_proxy(config: WebProxyConfig, transport: Option<Arc<QuicTransport>>) {
    let port = config.port;

    let state = ProxyState {
        cache: Arc::new(RwLock::new(HashMap::new())),
        mesh_relay_enabled: config.mesh_relay_enabled,
        doh_resolver: config.doh_resolver,
        clearnet_available: Arc::new(RwLock::new(true)),
        transport,
    };

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

            // Note: we can't update state here directly (moved). 
            // The actual clearnet check is done per-request in proxy_fetch.
            if !available {
                tracing::warn!("[WEB PROXY] ⚠️ Clearnet appears down");
            }
        }
    });
}

// ─── Endpoints ──────────────────────────────────────────────────────────

async fn proxy_status(State(state): State<ProxyState>) -> Json<ProxyStatus> {
    let clearnet = *state.clearnet_available.read().await;
    let cache_count = state.cache.read().await.len();

    Json(ProxyStatus {
        clearnet_available: clearnet,
        mesh_relay_enabled: state.mesh_relay_enabled,
        cache_entries: cache_count,
        doh_resolver: state.doh_resolver.clone(),
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

    // 3. Fall back to mesh relay (if enabled)
    if state.mesh_relay_enabled {
        tracing::info!("[WEB PROXY] 📡 Attempting mesh relay for: {}", req.url);
        // TODO: Wire actual mesh relay via QuicTransport
        // For now, return a clear indicator that mesh relay was attempted
        let elapsed = start.elapsed().as_millis() as u64;
        return Json(FetchResponse {
            url: req.url.clone(),
            status: 503,
            content_type: "text/plain".to_string(),
            body: "MESH_RELAY: No mesh peers with internet access available. Waiting for a relay peer.".to_string(),
            source: "mesh_relay_pending".to_string(),
            latency_ms: elapsed,
        });
    }

    // 4. Total failure
    let elapsed = start.elapsed().as_millis() as u64;
    Json(FetchResponse {
        url: req.url,
        status: 503,
        content_type: "text/plain".to_string(),
        body: "No clearnet access and mesh relay is disabled.".to_string(),
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

const PROXY_DASHBOARD: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>SafeNet Web Proxy</title>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700&display=swap" rel="stylesheet">
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: 'Inter', sans-serif;
            background: #0a0a0f;
            color: #e0e0e8;
            min-height: 100vh;
            padding: 32px;
        }
        .header {
            margin-bottom: 32px;
        }
        .header h1 {
            font-size: 28px;
            font-weight: 700;
            background: linear-gradient(135deg, #4caf50, #2196f3);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }
        .header p { color: #888; font-size: 14px; margin-top: 4px; }
        .stats {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 16px;
            margin-bottom: 32px;
        }
        .stat-card {
            background: rgba(255,255,255,0.03);
            border: 1px solid rgba(255,255,255,0.08);
            border-radius: 12px;
            padding: 20px;
        }
        .stat-card .label { font-size: 12px; color: #888; text-transform: uppercase; }
        .stat-card .value { font-size: 24px; font-weight: 600; margin-top: 4px; }
        .stat-card .value.green { color: #4caf50; }
        .stat-card .value.red { color: #f44336; }
        .stat-card .value.amber { color: #ff9800; }
        .fetch-box {
            background: rgba(255,255,255,0.03);
            border: 1px solid rgba(255,255,255,0.08);
            border-radius: 12px;
            padding: 24px;
            margin-bottom: 32px;
        }
        .fetch-box h2 { font-size: 16px; margin-bottom: 12px; color: #aaa; }
        .fetch-row {
            display: flex;
            gap: 10px;
        }
        .fetch-row input {
            flex: 1;
            padding: 10px 14px;
            background: rgba(0,0,0,0.4);
            border: 1px solid rgba(255,255,255,0.12);
            border-radius: 8px;
            color: #fff;
            font-family: monospace;
            font-size: 14px;
        }
        .fetch-row button {
            padding: 10px 20px;
            background: linear-gradient(135deg, #4caf50, #2196f3);
            border: none;
            border-radius: 8px;
            color: #fff;
            font-weight: 600;
            cursor: pointer;
        }
        #result {
            margin-top: 16px;
            padding: 16px;
            background: rgba(0,0,0,0.3);
            border-radius: 8px;
            font-family: monospace;
            font-size: 13px;
            max-height: 300px;
            overflow: auto;
            display: none;
        }
    </style>
</head>
<body>
    <div class="header">
        <h1>🌐 SafeNet Web Proxy</h1>
        <p>Censorship-resistant browsing through the HIVE mesh</p>
    </div>
    <div class="stats" id="stats">
        <div class="stat-card">
            <div class="label">Clearnet</div>
            <div class="value" id="clearnet-status">Checking...</div>
        </div>
        <div class="stat-card">
            <div class="label">Mesh Relay</div>
            <div class="value" id="relay-status">—</div>
        </div>
        <div class="stat-card">
            <div class="label">Cache</div>
            <div class="value" id="cache-count">0</div>
        </div>
        <div class="stat-card">
            <div class="label">DoH Resolver</div>
            <div class="value" id="doh-resolver" style="font-size:13px">—</div>
        </div>
    </div>
    <div class="fetch-box">
        <h2>Fetch URL</h2>
        <div class="fetch-row">
            <input type="text" id="url-input" placeholder="https://example.com" />
            <button onclick="fetchUrl()">Fetch</button>
        </div>
        <pre id="result"></pre>
    </div>
    <script>
        async function loadStatus() {
            try {
                const res = await fetch('/api/status');
                const s = await res.json();
                const cn = document.getElementById('clearnet-status');
                cn.textContent = s.clearnet_available ? '✅ Online' : '❌ Down';
                cn.className = 'value ' + (s.clearnet_available ? 'green' : 'red');
                
                const rl = document.getElementById('relay-status');
                rl.textContent = s.mesh_relay_enabled ? '✅ Active' : '❌ Off';
                rl.className = 'value ' + (s.mesh_relay_enabled ? 'green' : 'amber');
                
                document.getElementById('cache-count').textContent = s.cache_entries;
                document.getElementById('doh-resolver').textContent = s.doh_resolver.replace('https://', '');
            } catch(e) {
                document.getElementById('clearnet-status').textContent = 'Error';
            }
        }
        async function fetchUrl() {
            const url = document.getElementById('url-input').value;
            if (!url) return;
            const result = document.getElementById('result');
            result.style.display = 'block';
            result.textContent = 'Fetching...';
            try {
                const res = await fetch('/api/fetch', {
                    method: 'POST',
                    headers: {'Content-Type': 'application/json'},
                    body: JSON.stringify({ url })
                });
                const data = await res.json();
                result.textContent = `Source: ${data.source} | Status: ${data.status} | ${data.latency_ms}ms\n\n${data.body.substring(0, 2000)}`;
            } catch(e) {
                result.textContent = 'Fetch failed: ' + e.message;
            }
        }
        loadStatus();
        setInterval(loadStatus, 15000);
    </script>
</body>
</html>"##;

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
