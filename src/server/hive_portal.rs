/// HivePortal — Mesh Homepage & App Launcher.
///
/// The "Google Tab" for the mesh. A beautiful landing page with a search
/// bar, visual grid of all mesh services, and user-published mesh sites.
///
/// Served on localhost:3035 (configurable via REMOVED_MESH_GOVERNED).
use axum::{
    routing::get,
    Router,
    Json,
    extract::{State, Query},
    response::Html,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshSite {
    pub id: String,
    pub name: String,
    pub description: String,
    pub url: String,
    pub icon: String,
    pub author: String,
    pub category: String,
    pub created_at: String,
}

pub struct SiteRegistry {
    sites: RwLock<Vec<MeshSite>>,
    persist_path: String,
}

impl SiteRegistry {
    pub fn new() -> Self {
        let persist_path = "memory/portal_sites.json".to_string();
        let mut initial_sites = Vec::new();

        if let Ok(data) = std::fs::read_to_string(&persist_path) {
            if let Ok(sites) = serde_json::from_str::<Vec<MeshSite>>(&data) {
                tracing::info!("[PORTAL] 📂 Loaded {} mesh sites from disk", sites.len());
                initial_sites = sites;
            }
        }

        Self { 
            sites: RwLock::new(initial_sites),
            persist_path,
        }
    }

    /// Test-only constructor with isolated temp path.
    #[cfg(test)]
    fn new_test() -> Self {
        Self {
            sites: RwLock::new(Vec::new()),
            persist_path: format!("/tmp/hive_test_portal_{}.json", uuid::Uuid::new_v4()),
        }
    }

    pub async fn register(&self, site: MeshSite) {
        {
            self.sites.write().await.push(site);
        }
        self.persist().await;
    }

    pub async fn list(&self) -> Vec<MeshSite> {
        self.sites.read().await.clone()
    }

    pub async fn search(&self, query: &str) -> Vec<MeshSite> {
        let q = query.to_lowercase();
        self.sites.read().await.iter()
            .filter(|s| s.name.to_lowercase().contains(&q) || s.description.to_lowercase().contains(&q))
            .cloned().collect()
    }

    async fn persist(&self) {
        let sites = self.sites.read().await;
        if let Ok(json) = serde_json::to_string_pretty(&*sites) {
            if let Some(parent) = std::path::Path::new(&self.persist_path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&self.persist_path, json) {
                tracing::error!("[PORTAL] ❌ Failed to persist sites: {}", e);
            }
        }
    }

    /// Delete a site by ID.
    pub async fn delete(&self, site_id: &str) -> bool {
        let removed = {
            let mut sites = self.sites.write().await;
            let before = sites.len();
            sites.retain(|s| s.id != site_id);
            sites.len() < before
        };
        if removed { self.persist().await; }
        removed
    }

    /// Get site categories.
    pub async fn categories(&self) -> Vec<(String, usize)> {
        let sites = self.sites.read().await;
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for s in sites.iter() {
            *counts.entry(s.category.clone()).or_default() += 1;
        }
        let mut list: Vec<_> = counts.into_iter().collect();
        list.sort_by(|a, b| b.1.cmp(&a.1));
        list
    }
}

#[derive(Clone)]
struct PortalState {
    registry: Arc<SiteRegistry>,
    identity: Arc<crate::network::identity::MeshIdentity>,
}

#[derive(Deserialize)]
struct SearchQuery { q: Option<String> }

#[derive(Deserialize)]
struct SetIdentity {
    name: String,
    #[serde(default)]
    bio: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct RegisterSite {
    name: String,
    description: String,
    url: String,
    icon: Option<String>,
    category: Option<String>,
}

pub async fn spawn_hive_portal_server(registry: Arc<SiteRegistry>, identity: Arc<crate::network::identity::MeshIdentity>) {
    let port: u16 = 3035; // Mesh-governed: creator-key protected

    let state = PortalState { registry, identity };

    tokio::spawn(async move {
        tracing::info!("[PORTAL] 🏠 HivePortal starting on http://0.0.0.0:{}", port);

        let app = Router::new()
            .route("/api/services", get(api_services))
            .route("/api/health", get(api_health))
            .route("/api/sites", get(api_sites).post(api_register_site))
            .route("/api/sites/{site_id}", axum::routing::delete(api_delete_site))
            .route("/api/sites/categories", get(api_site_categories))
            .route("/api/search", get(api_search))
            .route("/api/status", get(api_portal_status))
            .route("/api/identity", get(api_get_identity).post(api_set_identity))
            .fallback(get(serve_portal))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let addr = format!("0.0.0.0:{}", port);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                tracing::info!("[PORTAL] 🏠 Bound on {}", addr);
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("[PORTAL] ❌ Server error: {}", e);
                }
            }
            Err(e) => tracing::error!("[PORTAL] ❌ Failed to bind {}: {}", addr, e),
        }
    });
}

// ─── API Endpoints ──────────────────────────────────────────────────────

async fn api_services() -> Json<Value> {
    // Real connectivity check — no fake stats
    let clearnet = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build().unwrap_or_default()
        .get("https://1.1.1.1/cdn-cgi/trace")
        .send().await.is_ok();

    // Only count REAL connected remote peers — never count self
    // These will be populated by the mesh loop when peers actually connect
    let mesh_peers: u32 = 0; // TODO: Wire to real MeshHandlers.book peer count
    let remote_compute: u32 = 0; // TODO: Wire to real PoolManager compute_nodes
    let remote_relays: u32 = 0; // TODO: Wire to real PoolManager web_relays

    Json(json!({
        "services": [
            {"id":"surface","name":"HiveSurface","icon":"🌐","desc":"Social media — posts, reactions, communities","port":3032,"url":"http://localhost:3032","category":"social"},
            {"id":"code","name":"Apis Code","icon":"💻","desc":"AI-powered web IDE with terminal","port":3033,"url":"http://localhost:3033","category":"tools"},
            {"id":"chat","name":"HiveChat","icon":"💬","desc":"Discord-style messaging & servers","port":3034,"url":"http://localhost:3034","category":"social"},
            {"id":"book","name":"Apis Book","icon":"📖","desc":"Read-only AI mesh activity dashboard","port":3031,"url":"http://localhost:3031","category":"tools"},
            {"id":"panopticon","name":"Panopticon","icon":"👁️","desc":"Engine visualizer & telemetry","port":3030,"url":"http://localhost:3030","category":"tools"},
            {"id":"proxy","name":"Web Proxy","icon":"🛡️","desc":"Censorship-resistant mesh browsing","port":8480,"url":"http://localhost:8480","category":"network"},
            {"id":"bank","name":"HIVE Bank","icon":"🏦","desc":"Wallet, NFT gallery, credits & crypto dashboard","port":3037,"url":"http://localhost:3037","category":"economy"},
            {"id":"marketplace","name":"Marketplace","icon":"🛒","desc":"Goods & services — trade, browse, review","port":3038,"url":"http://localhost:3038","category":"economy"},
            {"id":"uploads","name":"File Upload","icon":"📤","desc":"Upload & share images, documents, and media","port":8421,"url":"http://localhost:8421","category":"tools"},
            {"id":"fileshare","name":"File Share","icon":"📁","desc":"Browse & discover shared files across the mesh","port":3039,"url":"http://localhost:3039","category":"tools"},
        ],
        "connectivity": if clearnet { "online" } else { "mesh_only" },
        "mesh_peers": mesh_peers,
        "web_relays": remote_relays,
        "compute_nodes": remote_compute,
    }))
}

async fn api_sites(State(state): State<PortalState>) -> Json<Value> {
    let sites = state.registry.list().await;
    Json(json!({"sites": sites, "count": sites.len()}))
}

async fn api_register_site(State(state): State<PortalState>, Json(req): Json<RegisterSite>) -> Json<Value> {
    if req.name.trim().is_empty() || req.url.trim().is_empty() {
        return Json(json!({"error": "Name and URL required"}));
    }
    let site = MeshSite {
        id: uuid::Uuid::new_v4().to_string(),
        name: req.name, description: req.description,
        url: req.url, icon: req.icon.unwrap_or_else(|| "🌐".to_string()),
        author: std::env::var("HIVE_USER_NAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "Anonymous".to_string()),
        category: req.category.unwrap_or_else(|| "general".to_string()),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let id = site.id.clone();
    state.registry.register(site).await;
    Json(json!({"ok": true, "site_id": id}))
}

async fn api_search(State(state): State<PortalState>, Query(params): Query<SearchQuery>) -> Json<Value> {
    let q = params.q.unwrap_or_default();
    if q.is_empty() {
        return Json(json!({"results": [], "query": ""}));
    }
    let q_lower = q.to_lowercase();

    // Search user-published sites
    let sites = state.registry.search(&q).await;

    // Also search built-in services
    let services = vec![
        ("surface", "HiveSurface", "Social media posts reactions communities", 3032),
        ("code", "Apis Code", "AI web IDE terminal coding", 3033),
        ("chat", "HiveChat", "Discord messaging servers channels", 3034),
        ("bank", "HIVE Bank", "Wallet NFT crypto credits", 3037),
        ("marketplace", "Marketplace", "Goods services trade commerce", 3038),
        ("fileshare", "File Share", "Files documents media sharing", 3039),
        ("uploads", "File Upload", "Upload images documents", 8421),
    ];
    let matching_services: Vec<Value> = services.iter()
        .filter(|(_, name, desc, _)| name.to_lowercase().contains(&q_lower) || desc.to_lowercase().contains(&q_lower))
        .map(|(id, name, desc, port)| json!({
            "type": "service", "id": id, "name": name, "desc": desc,
            "url": format!("http://localhost:{}", port),
        }))
        .collect();

    Json(json!({
        "results": sites,
        "services": matching_services,
        "query": q,
        "count": sites.len() + matching_services.len(),
    }))
}

async fn api_portal_status(State(state): State<PortalState>) -> Json<Value> {
    let sites = state.registry.list().await;
    let profile = state.identity.profile().await;
    Json(json!({
        "portal": "HivePortal",
        "version": "2.0",
        "total_services": 10,
        "total_sites": sites.len(),
        "user": profile.display_name,
        "peer_id": profile.peer_id,
    }))
}

/// Health check — probe all mesh services.
async fn api_health() -> Json<Value> {
    let services = vec![
        ("panopticon", 3030), ("book", 3031), ("surface", 3032),
        ("code", 3033), ("chat", 3034), ("portal", 3035),
        ("bank", 3037), ("marketplace", 3038), ("fileshare", 3039),
        ("uploads", 8421), ("proxy", 8480),
    ];

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build().unwrap_or_default();

    let mut results = vec![];
    for (id, port) in &services {
        let url = format!("http://localhost:{}/api/status", port);
        let healthy = client.get(&url).send().await.is_ok();
        results.push(json!({
            "id": id, "port": port, "healthy": healthy,
            "status": if healthy { "up" } else { "down" },
        }));
    }

    let up = results.iter().filter(|r| r["healthy"] == true).count();
    let total = results.len();

    Json(json!({
        "services": results,
        "up": up, "down": total - up, "total": total,
        "health": if up == total { "all_healthy" } else if up > 0 { "degraded" } else { "down" },
    }))
}

async fn api_delete_site(
    State(state): State<PortalState>,
    axum::extract::Path(site_id): axum::extract::Path<String>,
) -> Json<Value> {
    let deleted = state.registry.delete(&site_id).await;
    Json(json!({"ok": deleted}))
}

async fn api_site_categories(State(state): State<PortalState>) -> Json<Value> {
    let cats = state.registry.categories().await;
    Json(json!({"categories": cats}))
}

async fn api_get_identity(State(state): State<PortalState>) -> Json<Value> {
    let profile = state.identity.profile().await;
    let is_anon = state.identity.is_anonymous().await;
    Json(json!({
        "name": profile.display_name,
        "peer_id": profile.peer_id,
        "avatar_url": profile.avatar_url,
        "bio": profile.bio,
        "status": format!("{}", profile.status),
        "custom_status": profile.custom_status,
        "is_anonymous": is_anon,
        "created_at": profile.created_at,
    }))
}

async fn api_set_identity(State(state): State<PortalState>, Json(req): Json<SetIdentity>) -> Json<Value> {
    let new_name = req.name.trim().to_string();
    if let Err(e) = state.identity.set_display_name(&new_name).await {
        return Json(json!({"error": e}));
    }

    // Update optional fields if provided
    if let Some(bio) = &req.bio {
        state.identity.set_bio(bio).await;
    }
    if let Some(avatar) = &req.avatar_url {
        state.identity.set_avatar(avatar).await;
    }
    if let Some(status_str) = &req.status {
        let status = match status_str.as_str() {
            "idle" => crate::network::identity::UserStatus::Idle,
            "dnd" => crate::network::identity::UserStatus::DoNotDisturb,
            "invisible" => crate::network::identity::UserStatus::Invisible,
            "offline" => crate::network::identity::UserStatus::Offline,
            _ => crate::network::identity::UserStatus::Online,
        };
        state.identity.set_status(status).await;
    }

    let profile = state.identity.profile().await;
    Json(json!({
        "ok": true,
        "name": profile.display_name,
        "peer_id": profile.peer_id,
        "avatar_url": profile.avatar_url,
        "bio": profile.bio,
        "status": format!("{}", profile.status),
    }))
}

// ─── SPA Frontend ───────────────────────────────────────────────────────

async fn serve_portal() -> Html<String> {
    Html(PORTAL_HTML.to_string())
}

const PORTAL_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HIVE — Mesh Homepage</title>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700;800;900&display=swap" rel="stylesheet">
    <style>
        *{margin:0;padding:0;box-sizing:border-box}
        :root{--bg:#08080d;--surface:#111118;--card:#16161f;--border:rgba(255,255,255,0.06);--text:#e0e0e8;--text-dim:#888;--text-muted:#555;--amber:#ffc107;--amber-glow:rgba(255,193,7,0.12);--green:#4caf50;--red:#ef5350;--blue:#42a5f5;--radius:20px}
        body{font-family:'Inter',sans-serif;background:var(--bg);color:var(--text);min-height:100vh}

        /* Status Bar */
        .topbar{padding:12px 32px;display:flex;justify-content:space-between;align-items:center;border-bottom:1px solid var(--border)}
        .topbar-left{display:flex;align-items:center;gap:8px}
        .topbar h1{font-size:18px;font-weight:800;background:linear-gradient(135deg,#ffc107,#ff9800);-webkit-background-clip:text;-webkit-text-fill-color:transparent}
        .topbar-right{display:flex;gap:10px;font-size:11px}
        .stat{padding:4px 10px;border-radius:16px;background:rgba(255,255,255,0.04);border:1px solid var(--border);display:flex;align-items:center;gap:4px}
        .dot{width:6px;height:6px;border-radius:50%}
        .dot-g{background:var(--green);box-shadow:0 0 8px var(--green)}
        .dot-r{background:var(--red);animation:pulse 2s infinite}
        @keyframes pulse{0%,100%{opacity:1}50%{opacity:.3}}

        /* Hero */
        .hero{text-align:center;padding:60px 20px 40px}
        .hero-logo{font-size:72px;margin-bottom:16px;filter:drop-shadow(0 0 40px rgba(255,193,7,0.3))}
        .hero h2{font-size:32px;font-weight:800;margin-bottom:8px;background:linear-gradient(135deg,#ffc107,#ff6f00);-webkit-background-clip:text;-webkit-text-fill-color:transparent}
        .hero p{color:var(--text-dim);font-size:14px;max-width:480px;margin:0 auto}

        /* Search */
        .search-wrap{max-width:600px;margin:30px auto;position:relative}
        .search-input{width:100%;padding:16px 56px 16px 24px;border-radius:30px;border:1px solid var(--border);background:var(--card);color:var(--text);font-family:inherit;font-size:15px;outline:none;transition:all .3s}
        .search-input:focus{border-color:var(--amber);box-shadow:0 0 30px rgba(255,193,7,0.1)}
        .search-btn{position:absolute;right:6px;top:6px;padding:10px 20px;border-radius:24px;border:none;background:linear-gradient(135deg,#ffc107,#ff9800);color:#000;font-weight:600;cursor:pointer;font-family:inherit;font-size:13px}
        .search-btn:hover{transform:scale(1.05)}

        /* Grid */
        .section{max-width:1100px;margin:0 auto;padding:0 24px}
        .section-title{font-size:14px;font-weight:700;color:var(--amber);text-transform:uppercase;letter-spacing:2px;margin:32px 0 16px;display:flex;align-items:center;gap:8px}
        .grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(200px,1fr));gap:16px}

        .card{background:var(--card);border:1px solid var(--border);border-radius:var(--radius);padding:24px;cursor:pointer;transition:all .3s;text-decoration:none;display:block}
        .card:hover{border-color:var(--amber);transform:translateY(-4px);box-shadow:0 12px 40px rgba(255,193,7,0.08)}
        .card-icon{font-size:40px;margin-bottom:12px;display:block}
        .card-name{font-size:15px;font-weight:700;color:var(--text);margin-bottom:4px}
        .card-desc{font-size:12px;color:var(--text-dim);line-height:1.5}
        .card-port{font-size:10px;color:var(--text-muted);margin-top:8px;font-family:monospace}

        /* User Sites */
        .user-card{background:linear-gradient(135deg,rgba(255,193,7,0.05),rgba(255,152,0,0.03));border:1px solid rgba(255,193,7,0.1)}
        .user-card:hover{border-color:var(--amber)}
        .user-card .card-author{font-size:10px;color:var(--text-muted);margin-top:6px}

        /* Build CTA */
        .cta{text-align:center;padding:40px 20px;margin-top:20px}
        .cta-btn{display:inline-flex;align-items:center;gap:8px;padding:14px 28px;border-radius:30px;border:2px solid var(--amber);background:transparent;color:var(--amber);font-weight:700;font-size:14px;cursor:pointer;font-family:inherit;transition:all .3s;text-decoration:none}
        .cta-btn:hover{background:var(--amber-glow);transform:scale(1.05)}

        /* Search Results */
        .results{max-width:600px;margin:0 auto;padding:0 24px}
        .result-item{padding:12px 16px;border-radius:12px;border:1px solid var(--border);background:var(--card);margin-bottom:8px;cursor:pointer;transition:all .2s}
        .result-item:hover{border-color:var(--amber)}
        .result-name{font-weight:600;font-size:14px}
        .result-url{font-size:11px;color:var(--text-muted)}

        /* Footer */
        .footer{text-align:center;padding:30px;color:var(--text-muted);font-size:11px;border-top:1px solid var(--border);margin-top:40px}
        
        /* Modal */
        .modal-overlay{position:fixed;top:0;left:0;right:0;bottom:0;background:rgba(0,0,0,0.8);backdrop-filter:blur(4px);z-index:9999;display:flex;align-items:center;justify-content:center}
        .modal-card{background:var(--card);border:1px solid var(--amber);padding:30px;border-radius:24px;width:100%;max-width:400px;box-shadow:0 20px 60px rgba(255,193,7,0.15)}
    </style>
</head>
<body>
    <div class="topbar">
        <div class="topbar-left">
            <span style="font-size:24px">🐝</span>
            <h1>HIVE</h1>
        </div>
        <div class="topbar-right">
            <div class="stat"><span id="user-display" onclick="document.getElementById('identity-modal').style.display='flex'" style="cursor:pointer" title="Change Identity">👤 ...</span></div>
            <div class="stat"><div class="dot" id="conn-dot"></div><span id="conn-text">checking...</span></div>
            <div class="stat">🖥️ <span id="compute-count">0</span> peers</div>
            <div class="stat">🌐 <span id="relay-count">0</span> relays</div>
        </div>
    </div>

    <div class="hero">
        <div class="hero-logo">🐝</div>
        <h2>Welcome to the Mesh</h2>
        <p>Your decentralised internet. No corporations, no censorship. Every peer is a server. Search, browse, create.</p>
    </div>

    <div class="search-wrap">
        <input class="search-input" id="search-input" placeholder="Search the mesh..." onkeydown="if(event.key==='Enter')doSearch()">
        <button class="search-btn" onclick="doSearch()">Search</button>
    </div>

    <div id="search-results" class="results" style="display:none"></div>

    <div class="section" id="services-section">
        <div class="section-title">⚡ Core Services</div>
        <div class="grid" id="services-grid"></div>
    </div>

    <div class="section" id="sites-section">
        <div class="section-title">🌐 Mesh Sites</div>
        <div class="grid" id="sites-grid">
            <div style="color:var(--text-muted);font-size:13px;grid-column:1/-1;text-align:center;padding:20px">
                No user sites published yet. Build one in <a href="http://localhost:3033" style="color:var(--amber)">Apis Code</a>!
            </div>
        </div>
    </div>

    <div class="cta">
        <a class="cta-btn" href="http://localhost:3033">🔧 Build Your Own Mesh Site</a>
    </div>

    <div class="footer">
        <p>🐝 HIVE v5.0 — The Human Internet Viable Ecosystem</p>
        <p>Everything here runs peer-to-peer. You are the internet.</p>
    </div>

    <!-- Identity Modal -->
    <div id="identity-modal" class="modal-overlay" style="display:none">
        <div class="modal-card">
            <h2 style="margin-bottom:10px;text-align:center;color:var(--amber)">Who are you?</h2>
            <p style="font-size:14px;color:var(--text-dim);margin-bottom:20px;text-align:center">Pick a display name for the mesh.</p>
            <input type="text" id="identity-input" class="search-input" placeholder="e.g. Neo" style="margin-bottom:15px;text-align:center" onkeydown="if(event.key==='Enter')setIdentity()">
            <div style="display:flex;gap:12px;justify-content:center">
                <button class="cta-btn" onclick="setIdentity()" style="padding:10px 20px">Set Name</button>
                <button class="cta-btn" style="padding:10px 20px;border-color:var(--border);color:var(--text-muted);" onclick="goAnonymous()">Stay Anon</button>
            </div>
            <div style="text-align:center;margin-top:15px">
                <a href="#" onclick="document.getElementById('identity-modal').style.display='none'" style="color:var(--text-muted);font-size:12px;text-decoration:none">Close</a>
            </div>
        </div>
    </div>

<script>
async function loadServices() {
    try {
        const res = await fetch('/api/services');
        const data = await res.json();

        // Status
        const dot = document.getElementById('conn-dot');
        const text = document.getElementById('conn-text');
        if (data.connectivity === 'online') { dot.className='dot dot-g'; text.textContent='online'; }
        else { dot.className='dot dot-r'; text.textContent='mesh only'; }
        document.getElementById('compute-count').textContent = data.mesh_peers || 0;
        document.getElementById('relay-count').textContent = data.web_relays || 0;

        // Services grid
        const grid = document.getElementById('services-grid');
        grid.innerHTML = (data.services || []).map(s => `
            <a class="card" href="${s.url}" target="_blank">
                <span class="card-icon">${s.icon}</span>
                <div class="card-name">${esc(s.name)}</div>
                <div class="card-desc">${esc(s.desc)}</div>
                <div class="card-port">:${s.port}</div>
            </a>
        `).join('');
    } catch(e) {}
}

async function loadSites() {
    try {
        const res = await fetch('/api/sites');
        const data = await res.json();
        const sites = data.sites || [];
        if (!sites.length) return;
        const grid = document.getElementById('sites-grid');
        grid.innerHTML = sites.map(s => `
            <a class="card user-card" href="${esc(s.url)}" target="_blank">
                <span class="card-icon">${s.icon}</span>
                <div class="card-name">${esc(s.name)}</div>
                <div class="card-desc">${esc(s.description)}</div>
                <div class="card-author">by ${esc(s.author)}</div>
            </a>
        `).join('');
    } catch(e) {}
}

async function doSearch() {
    const q = document.getElementById('search-input').value.trim();
    if (!q) { document.getElementById('search-results').style.display='none'; return; }

    const res = await fetch(`/api/search?q=${encodeURIComponent(q)}`);
    const data = await res.json();
    const results = data.results || [];
    const el = document.getElementById('search-results');

    if (!results.length) {
        el.style.display='block';
        el.innerHTML = `<div style="text-align:center;padding:20px;color:var(--text-muted)">No results for "${esc(q)}"</div>`;
        return;
    }
    el.style.display='block';
    el.innerHTML = results.map(r => `
        <a class="result-item" href="${esc(r.url)}" target="_blank" style="display:block;text-decoration:none;color:var(--text)">
            <div class="result-name">${r.icon} ${esc(r.name)}</div>
            <div class="result-url">${esc(r.url)}</div>
            <div style="font-size:12px;color:var(--text-dim);margin-top:4px">${esc(r.description)}</div>
        </a>
    `).join('');
}

function esc(t){if(!t)return'';const d=document.createElement('div');d.textContent=t;return d.innerHTML}

async function checkIdentity() {
    try {
        const res = await fetch('/api/identity');
        const data = await res.json();
        document.getElementById('user-display').innerHTML = `👤 ${esc(data.name)}`;
        const hasPrompted = localStorage.getItem('hive_identity_prompted');
        if (data.is_anonymous && !hasPrompted) {
            document.getElementById('identity-modal').style.display = 'flex';
        }
    } catch(e) {}
}

async function updateIdentity(name) {
    await fetch('/api/identity', {
        method: 'POST',
        headers: {'Content-Type': 'application/json'},
        body: JSON.stringify({name: name})
    });
    localStorage.setItem('hive_identity_prompted', 'true');
    document.getElementById('identity-modal').style.display = 'none';
    checkIdentity();
}

function setIdentity() {
    const val = document.getElementById('identity-input').value.trim();
    if (val) updateIdentity(val);
}

function goAnonymous() {
    updateIdentity("Anonymous");
}

loadServices();
loadSites();
checkIdentity();
setInterval(loadServices, 30000);
</script>
</body>
</html>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_portal_html_not_empty() {
        assert!(PORTAL_HTML.len() > 1000);
        assert!(PORTAL_HTML.contains("HIVE"));
        assert!(PORTAL_HTML.contains("/api/services"));
        assert!(PORTAL_HTML.contains("/api/sites"));
    }

    #[tokio::test]
    async fn test_site_registry() {
        let registry = SiteRegistry::new_test();
        assert_eq!(registry.list().await.len(), 0);

        registry.register(MeshSite {
            id: "1".into(), name: "Test Site".into(), description: "A test".into(),
            url: "http://localhost:9999".into(), icon: "🧪".into(),
            author: "Alice".into(), category: "test".into(),
            created_at: "2024-01-01".into(),
        }).await;

        assert_eq!(registry.list().await.len(), 1);

        let results = registry.search("test").await;
        assert_eq!(results.len(), 1);

        let empty = registry.search("nonexistent").await;
        assert_eq!(empty.len(), 0);
    }

    #[tokio::test]
    async fn test_site_delete() {
        let registry = SiteRegistry::new_test();
        registry.register(MeshSite {
            id: "del1".into(), name: "Delete Me".into(), description: "temp".into(),
            url: "http://test".into(), icon: "🗑".into(),
            author: "Bob".into(), category: "temp".into(),
            created_at: "2024-01-01".into(),
        }).await;

        assert_eq!(registry.list().await.len(), 1);
        assert!(registry.delete("del1").await);
        assert_eq!(registry.list().await.len(), 0);
        assert!(!registry.delete("del1").await); // double delete
    }

    #[tokio::test]
    async fn test_site_categories() {
        let registry = SiteRegistry::new_test();
        for i in 0..3 {
            registry.register(MeshSite {
                id: format!("c{}", i), name: format!("Site {}", i), description: "".into(),
                url: "http://test".into(), icon: "🌐".into(),
                author: "A".into(), category: "blog".into(),
                created_at: "2024-01-01".into(),
            }).await;
        }
        registry.register(MeshSite {
            id: "c3".into(), name: "Shop".into(), description: "".into(),
            url: "http://test".into(), icon: "🛒".into(),
            author: "A".into(), category: "commerce".into(),
            created_at: "2024-01-01".into(),
        }).await;

        let cats = registry.categories().await;
        assert_eq!(cats.len(), 2);
        assert_eq!(cats[0].0, "blog");
        assert_eq!(cats[0].1, 3);
    }
}
