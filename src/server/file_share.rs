/// File Share — Mesh file sharing platform on port 3039.
///
/// Browse, upload, download, and share files across the mesh.
/// Uses the shared Upload Server (port 8421) for actual file storage.
/// This platform provides the browsable UI and metadata layer.
use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::{State, Path, Query},
    response::Html,
};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A shared file entry with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedFile {
    pub id: String,
    pub name: String,
    pub url: String,
    pub mime_type: String,
    pub size: usize,
    pub uploader_name: String,
    pub uploader_id: String,
    pub category: String,
    pub description: String,
    pub download_count: u64,
    pub shared_at: String,
}

/// File sharing store.
pub struct FileShareStore {
    files: RwLock<Vec<SharedFile>>,
    persist_path: String,
}

impl FileShareStore {
    pub fn new() -> Self {
        let persist_path = "memory/file_share.json".to_string();
        let files = if let Ok(data) = std::fs::read_to_string(&persist_path) {
            serde_json::from_str::<Vec<SharedFile>>(&data).unwrap_or_default()
        } else {
            Vec::new()
        };

        let count = files.len();
        if count > 0 {
            tracing::info!("[FILESHARE] 📁 Loaded {} shared files from disk", count);
        }

        Self {
            files: RwLock::new(files),
            persist_path,
        }
    }

    async fn persist(&self) {
        let files = self.files.read().await;
        if let Ok(json) = serde_json::to_string_pretty(&*files) {
            if let Some(parent) = std::path::Path::new(&self.persist_path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&self.persist_path, json);
        }
    }

    pub async fn add_file(&self, file: SharedFile) {
        self.files.write().await.push(file);
        self.persist().await;
    }

    pub async fn list(&self, category: Option<&str>, limit: usize) -> Vec<SharedFile> {
        let files = self.files.read().await;
        match category {
            Some(cat) => files.iter()
                .rev()
                .filter(|f| f.category == cat)
                .take(limit)
                .cloned()
                .collect(),
            None => files.iter().rev().take(limit).cloned().collect(),
        }
    }

    pub async fn search(&self, query: &str, limit: usize) -> Vec<SharedFile> {
        let files = self.files.read().await;
        let q = query.to_lowercase();
        files.iter()
            .rev()
            .filter(|f| f.name.to_lowercase().contains(&q)
                || f.description.to_lowercase().contains(&q)
                || f.category.to_lowercase().contains(&q))
            .take(limit)
            .cloned()
            .collect()
    }

    pub async fn increment_download(&self, file_id: &str) -> bool {
        let found = {
            let mut files = self.files.write().await;
            if let Some(f) = files.iter_mut().find(|f| f.id == file_id) {
                f.download_count += 1;
                true
            } else {
                false
            }
        };
        if found { self.persist().await; }
        found
    }

    pub async fn delete(&self, file_id: &str, uploader_id: &str) -> bool {
        let removed = {
            let mut files = self.files.write().await;
            let before = files.len();
            files.retain(|f| !(f.id == file_id && f.uploader_id == uploader_id));
            files.len() < before
        };
        if removed { self.persist().await; }
        removed
    }

    pub async fn categories(&self) -> Vec<(String, usize)> {
        let files = self.files.read().await;
        let mut counts: HashMap<String, usize> = HashMap::new();
        for f in files.iter() {
            *counts.entry(f.category.clone()).or_default() += 1;
        }
        let mut list: Vec<_> = counts.into_iter().collect();
        list.sort_by(|a, b| b.1.cmp(&a.1));
        list
    }

    pub async fn count(&self) -> usize {
        self.files.read().await.len()
    }
}

// ─── Server ─────────────────────────────────────────────────────────────

#[derive(Clone)]
struct FileShareState {
    store: Arc<FileShareStore>,
    identity: Arc<crate::network::identity::MeshIdentity>,
}

#[derive(Deserialize)]
struct ShareFileReq {
    name: String,
    url: String,
    mime_type: String,
    size: usize,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default)]
    limit: Option<usize>,
}

pub async fn spawn_file_share_server(identity: Arc<crate::network::identity::MeshIdentity>) {
    let port: u16 = std::env::var("HIVE_FILESHARE_PORT")
        .ok().and_then(|v| v.parse().ok())
        .unwrap_or(3039);

    let store = Arc::new(FileShareStore::new());
    let state = FileShareState { store, identity };

    tokio::spawn(async move {
        tracing::info!("[FILESHARE] 📁 File sharing platform starting on http://0.0.0.0:{}", port);

        let app = Router::new()
            .route("/api/files", get(api_list_files).post(api_share_file))
            .route("/api/files/{file_id}", axum::routing::delete(api_delete_file))
            .route("/api/files/{file_id}/download", post(api_download))
            .route("/api/search", get(api_search_files))
            .route("/api/categories", get(api_categories))
            .route("/api/status", get(api_status))
            .fallback(get(serve_file_share_spa))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let addr = format!("0.0.0.0:{}", port);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                tracing::info!("[FILESHARE] 📁 Bound on {}", addr);
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("[FILESHARE] ❌ Server error: {}", e);
                }
            }
            Err(e) => tracing::error!("[FILESHARE] ❌ Failed to bind {}: {}", addr, e),
        }
    });
}

// ─── API Handlers ───────────────────────────────────────────────────────

async fn api_list_files(
    State(state): State<FileShareState>,
    Query(params): Query<ListQuery>,
) -> Json<Value> {
    let limit = params.limit.unwrap_or(50).min(200);
    let files = state.store.list(params.category.as_deref(), limit).await;
    Json(json!({"files": files, "count": files.len()}))
}

async fn api_share_file(
    State(state): State<FileShareState>,
    Json(req): Json<ShareFileReq>,
) -> Json<Value> {
    let profile = state.identity.profile().await;
    let file = SharedFile {
        id: uuid::Uuid::new_v4().to_string(),
        name: req.name,
        url: req.url,
        mime_type: req.mime_type,
        size: req.size,
        uploader_name: profile.display_name,
        uploader_id: profile.peer_id,
        category: req.category.unwrap_or_else(|| "general".to_string()),
        description: req.description.unwrap_or_default(),
        download_count: 0,
        shared_at: chrono::Utc::now().to_rfc3339(),
    };
    state.store.add_file(file.clone()).await;
    Json(json!({"ok": true, "file": file}))
}

async fn api_delete_file(
    State(state): State<FileShareState>,
    Path(file_id): Path<String>,
) -> Json<Value> {
    let peer_id = state.identity.peer_id().await;
    let deleted = state.store.delete(&file_id, &peer_id).await;
    Json(json!({"ok": deleted}))
}

async fn api_download(
    State(state): State<FileShareState>,
    Path(file_id): Path<String>,
) -> Json<Value> {
    let found = state.store.increment_download(&file_id).await;
    Json(json!({"ok": found}))
}

async fn api_search_files(
    State(state): State<FileShareState>,
    Query(params): Query<SearchQuery>,
) -> Json<Value> {
    let limit = params.limit.unwrap_or(50).min(200);
    let files = state.store.search(&params.q, limit).await;
    Json(json!({"files": files, "count": files.len()}))
}

async fn api_categories(State(state): State<FileShareState>) -> Json<Value> {
    let cats = state.store.categories().await;
    Json(json!({"categories": cats}))
}

async fn api_status(State(state): State<FileShareState>) -> Json<Value> {
    let count = state.store.count().await;
    Json(json!({"total_files": count, "platform": "HIVE File Share"}))
}

async fn serve_file_share_spa() -> Html<&'static str> {
    Html(FILE_SHARE_HTML)
}

const FILE_SHARE_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HIVE File Share</title>
    <style>
        *{margin:0;padding:0;box-sizing:border-box}
        body{font-family:'Inter',system-ui,sans-serif;background:#08080d;color:#e0e0e8;min-height:100vh}
        .header{padding:20px 32px;border-bottom:1px solid rgba(255,255,255,0.06);display:flex;align-items:center;gap:16px}
        .header h1{font-size:22px;background:linear-gradient(135deg,#ffc107,#ff9800);-webkit-background-clip:text;-webkit-text-fill-color:transparent}
        .toolbar{padding:16px 32px;display:flex;gap:12px;align-items:center;flex-wrap:wrap}
        .toolbar input,.toolbar select{background:rgba(255,255,255,0.06);border:1px solid rgba(255,255,255,0.1);color:#e0e0e8;padding:8px 14px;border-radius:10px;font-size:13px;outline:none}
        .toolbar input:focus{border-color:#ffc107}
        .toolbar button{background:linear-gradient(135deg,#ffc107,#ff9800);color:#000;border:none;padding:8px 18px;border-radius:10px;cursor:pointer;font-weight:600;font-size:13px;transition:all .2s}
        .toolbar button:hover{transform:scale(1.03);box-shadow:0 4px 20px rgba(255,193,7,0.3)}
        .grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(280px,1fr));gap:16px;padding:16px 32px}
        .file-card{background:rgba(255,255,255,0.03);border:1px solid rgba(255,255,255,0.06);border-radius:16px;padding:20px;transition:all .3s}
        .file-card:hover{border-color:rgba(255,193,7,0.3);transform:translateY(-2px);box-shadow:0 8px 32px rgba(0,0,0,0.3)}
        .file-icon{font-size:32px;margin-bottom:12px}
        .file-name{font-size:15px;font-weight:600;margin-bottom:6px;word-break:break-word}
        .file-meta{font-size:11px;color:#888;display:flex;flex-direction:column;gap:4px}
        .file-actions{margin-top:12px;display:flex;gap:8px}
        .file-actions a,.file-actions button{padding:6px 14px;border-radius:8px;font-size:12px;cursor:pointer;text-decoration:none;border:none;transition:all .2s}
        .file-actions a{background:rgba(255,193,7,0.15);color:#ffc107}
        .file-actions a:hover{background:rgba(255,193,7,0.3)}
        .file-actions button{background:rgba(239,83,80,0.15);color:#ef5350}
        .file-actions button:hover{background:rgba(239,83,80,0.3)}
        .cat-badge{display:inline-block;padding:3px 10px;border-radius:20px;font-size:10px;font-weight:600;background:rgba(255,193,7,0.12);color:#ffc107;margin-bottom:8px}
        .empty{text-align:center;padding:60px;color:#555;font-size:14px}
        .stats{padding:8px 32px;font-size:11px;color:#555}
    </style>
</head>
<body>
    <div class="header">
        <span style="font-size:28px">📁</span>
        <h1>HIVE File Share</h1>
    </div>
    <div class="toolbar">
        <input type="text" id="search" placeholder="Search files..." oninput="searchFiles()">
        <select id="cat-filter" onchange="loadFiles()">
            <option value="">All Categories</option>
        </select>
        <button onclick="document.getElementById('upload-input').click()">📤 Upload & Share</button>
        <input type="file" id="upload-input" multiple style="display:none" onchange="uploadAndShare(this.files)">
    </div>
    <div class="stats" id="stats"></div>
    <div class="grid" id="file-grid"></div>
<script>
const UPLOAD_URL = 'http://localhost:8421/upload';

async function loadFiles() {
    const cat = document.getElementById('cat-filter').value;
    const url = cat ? `/api/files?category=${cat}` : '/api/files';
    const res = await fetch(url);
    const data = await res.json();
    renderFiles(data.files || []);
    document.getElementById('stats').textContent = `${data.count} files shared on the mesh`;
}

async function searchFiles() {
    const q = document.getElementById('search').value.trim();
    if (!q) return loadFiles();
    const res = await fetch(`/api/search?q=${encodeURIComponent(q)}`);
    const data = await res.json();
    renderFiles(data.files || []);
}

function getIcon(mime) {
    if (mime.startsWith('image/')) return '🖼️';
    if (mime.startsWith('video/')) return '🎬';
    if (mime.startsWith('audio/')) return '🎵';
    if (mime.includes('pdf')) return '📄';
    if (mime.includes('zip') || mime.includes('gzip')) return '📦';
    if (mime.includes('json') || mime.includes('text')) return '📝';
    return '📎';
}

function formatSize(bytes) {
    if (bytes < 1024) return bytes + ' B';
    if (bytes < 1024*1024) return (bytes/1024).toFixed(1) + ' KB';
    return (bytes/1024/1024).toFixed(1) + ' MB';
}

function renderFiles(files) {
    const grid = document.getElementById('file-grid');
    if (!files.length) {
        grid.innerHTML = '<div class="empty">No files shared yet. Upload something to get started!</div>';
        return;
    }
    grid.innerHTML = files.map(f => `
        <div class="file-card">
            <div class="file-icon">${getIcon(f.mime_type)}</div>
            <div class="cat-badge">${f.category}</div>
            <div class="file-name">${f.name}</div>
            <div class="file-meta">
                <span>${formatSize(f.size)} · ${f.mime_type}</span>
                <span>by ${f.uploader_name} · ${f.download_count} downloads</span>
                ${f.description ? `<span style="color:#aaa;margin-top:4px">${f.description}</span>` : ''}
            </div>
            <div class="file-actions">
                <a href="${f.url}" target="_blank" onclick="trackDownload('${f.id}')">⬇ Download</a>
                <button onclick="deleteFile('${f.id}')">🗑 Delete</button>
            </div>
        </div>
    `).join('');
}

async function uploadAndShare(fileList) {
    const fd = new FormData();
    for (const f of fileList) fd.append('file', f);
    const res = await fetch(UPLOAD_URL, { method: 'POST', body: fd });
    const data = await res.json();
    for (const f of (data.files || [])) {
        if (f.error) continue;
        const cat = prompt(`Category for "${f.name}"?`, 'general') || 'general';
        const desc = prompt(`Description for "${f.name}"?`, '') || '';
        await fetch('/api/files', {
            method: 'POST',
            headers: {'Content-Type':'application/json'},
            body: JSON.stringify({ name: f.name, url: f.url, mime_type: f.mime, size: f.size, category: cat, description: desc })
        });
    }
    loadFiles();
    loadCategories();
}

async function deleteFile(id) {
    if (!confirm('Delete this shared file?')) return;
    await fetch(`/api/files/${id}`, { method: 'DELETE' });
    loadFiles();
}

async function trackDownload(id) {
    fetch(`/api/files/${id}/download`, { method: 'POST' });
}

async function loadCategories() {
    const res = await fetch('/api/categories');
    const data = await res.json();
    const sel = document.getElementById('cat-filter');
    sel.innerHTML = '<option value="">All Categories</option>' +
        (data.categories || []).map(([name, count]) => `<option value="${name}">${name} (${count})</option>`).join('');
}

loadFiles();
loadCategories();
</script>
</body>
</html>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_file_share_store() {
        let store = FileShareStore {
            files: RwLock::new(Vec::new()),
            persist_path: "/tmp/hive_test_fileshare.json".to_string(),
        };

        let file = SharedFile {
            id: "test1".to_string(),
            name: "readme.md".to_string(),
            url: "http://localhost:8421/file/test.md".to_string(),
            mime_type: "text/markdown".to_string(),
            size: 1234,
            uploader_name: "Alice".to_string(),
            uploader_id: "alice".to_string(),
            category: "documents".to_string(),
            description: "Test file".to_string(),
            download_count: 0,
            shared_at: chrono::Utc::now().to_rfc3339(),
        };

        store.add_file(file).await;
        assert_eq!(store.count().await, 1);
        assert_eq!(store.list(None, 10).await.len(), 1);
        assert_eq!(store.list(Some("documents"), 10).await.len(), 1);
        assert_eq!(store.list(Some("images"), 10).await.len(), 0);
    }

    #[tokio::test]
    async fn test_file_share_search() {
        let store = FileShareStore {
            files: RwLock::new(Vec::new()),
            persist_path: "/tmp/hive_test_fileshare_search.json".to_string(),
        };

        store.add_file(SharedFile {
            id: "1".into(), name: "photo.jpg".into(), url: "u".into(),
            mime_type: "image/jpeg".into(), size: 100, uploader_name: "A".into(),
            uploader_id: "a".into(), category: "images".into(),
            description: "sunset photo".into(), download_count: 0,
            shared_at: chrono::Utc::now().to_rfc3339(),
        }).await;

        assert_eq!(store.search("photo", 10).await.len(), 1);
        assert_eq!(store.search("sunset", 10).await.len(), 1);
        assert_eq!(store.search("video", 10).await.len(), 0);
    }

    #[tokio::test]
    async fn test_file_share_download_count() {
        let store = FileShareStore {
            files: RwLock::new(Vec::new()),
            persist_path: "/tmp/hive_test_fileshare_dl.json".to_string(),
        };

        store.add_file(SharedFile {
            id: "dl1".into(), name: "doc.pdf".into(), url: "u".into(),
            mime_type: "application/pdf".into(), size: 500, uploader_name: "B".into(),
            uploader_id: "b".into(), category: "docs".into(),
            description: "".into(), download_count: 0,
            shared_at: chrono::Utc::now().to_rfc3339(),
        }).await;

        assert!(store.increment_download("dl1").await);
        assert!(store.increment_download("dl1").await);
        assert!(!store.increment_download("nonexistent").await);

        let files = store.list(None, 10).await;
        assert_eq!(files[0].download_count, 2);
    }

    #[tokio::test]
    async fn test_file_share_delete() {
        let store = FileShareStore {
            files: RwLock::new(Vec::new()),
            persist_path: "/tmp/hive_test_fileshare_del.json".to_string(),
        };

        store.add_file(SharedFile {
            id: "del1".into(), name: "rm.txt".into(), url: "u".into(),
            mime_type: "text/plain".into(), size: 10, uploader_name: "C".into(),
            uploader_id: "charlie".into(), category: "misc".into(),
            description: "".into(), download_count: 0,
            shared_at: chrono::Utc::now().to_rfc3339(),
        }).await;

        assert!(!store.delete("del1", "eve").await); // wrong uploader
        assert!(store.delete("del1", "charlie").await);
        assert_eq!(store.count().await, 0);
    }

    #[tokio::test]
    async fn test_file_share_categories() {
        let store = FileShareStore {
            files: RwLock::new(Vec::new()),
            persist_path: "/tmp/hive_test_fileshare_cats.json".to_string(),
        };

        for i in 0..3 {
            store.add_file(SharedFile {
                id: format!("c{}", i), name: format!("f{}.jpg", i), url: "u".into(),
                mime_type: "image/jpeg".into(), size: 100, uploader_name: "A".into(),
                uploader_id: "a".into(), category: "images".into(),
                description: "".into(), download_count: 0,
                shared_at: chrono::Utc::now().to_rfc3339(),
            }).await;
        }
        store.add_file(SharedFile {
            id: "c3".into(), name: "doc.pdf".into(), url: "u".into(),
            mime_type: "application/pdf".into(), size: 200, uploader_name: "A".into(),
            uploader_id: "a".into(), category: "documents".into(),
            description: "".into(), download_count: 0,
            shared_at: chrono::Utc::now().to_rfc3339(),
        }).await;

        let cats = store.categories().await;
        assert_eq!(cats.len(), 2);
        assert_eq!(cats[0].0, "images"); // most files
        assert_eq!(cats[0].1, 3);
    }

    #[test]
    fn test_file_share_html() {
        assert!(FILE_SHARE_HTML.len() > 500);
        assert!(FILE_SHARE_HTML.contains("File Share"));
        assert!(FILE_SHARE_HTML.contains("/api/files"));
    }
}
