/// Upload Server — Shared file upload endpoint for the HIVE mesh.
///
/// Serves on port 8421 (configurable via HIVE_UPLOAD_PORT).
/// Handles multipart uploads for images, documents, and media.
/// Files are stored in `data/uploads/` and served via GET.
///
/// Used by HiveSurface (post images), HiveChat (file attachments),
/// HivePortal (avatars), and the Mesh Site Builder (site assets).
use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::{State, Path, Multipart, DefaultBodyLimit},
    response::Html,
    body::Body,
};
use std::sync::Arc;
use std::path::PathBuf;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Metadata for an uploaded file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadedFile {
    pub id: String,
    pub original_name: String,
    pub stored_name: String,
    pub mime_type: String,
    pub size: usize,
    pub url: String,
    pub uploaded_at: String,
    pub uploader: String,
}

/// Upload store — tracks all uploaded files.
pub struct UploadStore {
    uploads: tokio::sync::RwLock<Vec<UploadedFile>>,
    upload_dir: PathBuf,
    persist_path: PathBuf,
    max_file_size: usize,
    port: u16,
}

impl UploadStore {
    pub fn new(port: u16) -> Self {
        let upload_dir = PathBuf::from(
            std::env::var("HIVE_UPLOAD_DIR")
                .unwrap_or_else(|_| "data/uploads".to_string())
        );
        let persist_path = PathBuf::from("memory/uploads.json");
        let max_file_size = std::env::var("HIVE_MAX_UPLOAD_MB")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(50) * 1024 * 1024; // Default 50MB

        let _ = std::fs::create_dir_all(&upload_dir);

        // Load persisted upload metadata
        let uploads = if persist_path.exists() {
            std::fs::read_to_string(&persist_path)
                .ok()
                .and_then(|data| serde_json::from_str::<Vec<UploadedFile>>(&data).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let count = uploads.len();
        if count > 0 {
            tracing::info!("[UPLOAD] 📂 Loaded {} upload records from disk", count);
        }

        Self {
            uploads: tokio::sync::RwLock::new(uploads),
            upload_dir,
            persist_path,
            max_file_size,
            port,
        }
    }

    /// Save upload metadata to disk.
    async fn persist(&self) {
        let uploads = self.uploads.read().await;
        if let Ok(json) = serde_json::to_string_pretty(&*uploads) {
            if let Some(parent) = self.persist_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&self.persist_path, json) {
                tracing::error!("[UPLOAD] ❌ Failed to persist upload metadata: {}", e);
            }
        }
    }

    /// Store a file and return its metadata.
    pub async fn store_file(
        &self,
        original_name: &str,
        data: &[u8],
        mime_type: &str,
        uploader: &str,
    ) -> Result<UploadedFile, String> {
        if data.len() > self.max_file_size {
            return Err(format!(
                "File too large: {} bytes (max {} MB)",
                data.len(),
                self.max_file_size / 1024 / 1024
            ));
        }

        // Validate MIME type
        if !Self::is_allowed_mime(mime_type) {
            return Err(format!("File type not allowed: {}", mime_type));
        }

        // Generate unique filename
        let id = uuid::Uuid::new_v4().to_string();
        let ext = Self::ext_from_name(original_name);
        let stored_name = format!("{}.{}", id, ext);
        let file_path = self.upload_dir.join(&stored_name);

        // Write file to disk
        tokio::fs::write(&file_path, data).await
            .map_err(|e| format!("Failed to write file: {}", e))?;

        let upload = UploadedFile {
            id: id.clone(),
            original_name: original_name.to_string(),
            stored_name: stored_name.clone(),
            mime_type: mime_type.to_string(),
            size: data.len(),
            url: format!("http://localhost:{}/file/{}", self.port, stored_name),
            uploaded_at: chrono::Utc::now().to_rfc3339(),
            uploader: uploader.to_string(),
        };

        self.uploads.write().await.push(upload.clone());
        self.persist().await;

        tracing::info!(
            "[UPLOAD] 📤 Stored: {} ({} bytes, {})",
            original_name, data.len(), mime_type
        );

        Ok(upload)
    }

    /// List all uploads.
    pub async fn list(&self) -> Vec<UploadedFile> {
        self.uploads.read().await.clone()
    }

    /// Get an upload by stored name.
    pub async fn get_by_name(&self, stored_name: &str) -> Option<UploadedFile> {
        self.uploads.read().await.iter()
            .find(|u| u.stored_name == stored_name)
            .cloned()
    }

    /// Delete an upload by ID.
    pub async fn delete(&self, id: &str) -> bool {
        let removed = {
            let mut uploads = self.uploads.write().await;
            if let Some(pos) = uploads.iter().position(|u| u.id == id) {
                let upload = uploads.remove(pos);
                let file_path = self.upload_dir.join(&upload.stored_name);
                let _ = tokio::fs::remove_file(file_path).await;
                true
            } else {
                false
            }
        };
        if removed {
            self.persist().await;
        }
        removed
    }

    /// Check if a MIME type is allowed.
    fn is_allowed_mime(mime_type: &str) -> bool {
        let allowed_prefixes = [
            "image/", "video/", "audio/",
            "text/plain", "text/markdown", "text/html", "text/css",
            "application/pdf", "application/json",
            "application/zip", "application/gzip",
            "application/octet-stream",
        ];
        allowed_prefixes.iter().any(|prefix| mime_type.starts_with(prefix))
    }

    /// Extract extension from filename.
    fn ext_from_name(name: &str) -> String {
        std::path::Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin")
            .to_lowercase()
    }

    /// Guess MIME type from extension.
    fn mime_from_ext(ext: &str) -> &'static str {
        match ext {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            "mp4" => "video/mp4",
            "webm" => "video/webm",
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            "ogg" => "audio/ogg",
            "pdf" => "application/pdf",
            "json" => "application/json",
            "txt" => "text/plain",
            "md" => "text/markdown",
            "html" | "htm" => "text/html",
            "css" => "text/css",
            "zip" => "application/zip",
            "gz" => "application/gzip",
            _ => "application/octet-stream",
        }
    }
}

// ─── Server Setup ───────────────────────────────────────────────────────

#[derive(Clone)]
struct UploadState {
    store: Arc<UploadStore>,
    identity: Arc<crate::network::identity::MeshIdentity>,
}

pub async fn spawn_upload_server(identity: Arc<crate::network::identity::MeshIdentity>) {
    let port: u16 = std::env::var("HIVE_UPLOAD_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8421);

    let store = Arc::new(UploadStore::new(port));
    let state = UploadState { store, identity };

    tokio::spawn(async move {
        tracing::info!("[UPLOAD] 📤 Upload server starting on http://0.0.0.0:{}", port);

        let app = Router::new()
            .route("/upload", post(api_upload))
            .route("/file/{filename}", get(api_serve_file))
            .route("/api/uploads", get(api_list_uploads))
            .route("/api/upload/{id}", axum::routing::delete(api_delete_upload))
            .route("/api/status", get(api_upload_status))
            .fallback(get(serve_upload_page))
            .layer(DefaultBodyLimit::max(50 * 1024 * 1024)) // 50MB
            .layer(CorsLayer::permissive())
            .with_state(state);

        let addr = format!("0.0.0.0:{}", port);

        // NOTE: Removed fuser -k port cleanup — it kills HIVE's own process
        // since other servers in the same binary may already hold this port.
        // SO_REUSEADDR below handles socket reuse instead.

        // Bind with SO_REUSEADDR to handle TIME_WAIT sockets
        let socket = match socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("[UPLOAD] ❌ Failed to create socket: {}", e);
                return;
            }
        };
        socket.set_reuse_address(true).ok();
        socket.set_nonblocking(true).ok();
        let addr_parsed: std::net::SocketAddr = addr.parse().unwrap();
        if let Err(e) = socket.bind(&addr_parsed.into()) {
            tracing::error!("[UPLOAD] ❌ Failed to bind {}: {}", addr, e);
            return;
        }
        if let Err(e) = socket.listen(128) {
            tracing::error!("[UPLOAD] ❌ Failed to listen on {}: {}", addr, e);
            return;
        }
        let std_listener: std::net::TcpListener = socket.into();
        match TcpListener::from_std(std_listener) {
            Ok(listener) => {
                tracing::info!("[UPLOAD] 📤 Upload server bound on {}", addr);
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("[UPLOAD] ❌ Server error: {}", e);
                }
            }
            Err(e) => tracing::error!("[UPLOAD] ❌ Failed to convert listener: {}", e),
        }
    });
}

// ─── API Endpoints ──────────────────────────────────────────────────────

async fn api_upload(
    State(state): State<UploadState>,
    mut multipart: Multipart,
) -> Json<Value> {
    let mut uploaded = Vec::new();
    let uploader = state.identity.display_name().await;

    while let Ok(Some(field)) = multipart.next_field().await {
        let original_name = field.file_name()
            .unwrap_or("unnamed")
            .to_string();

        let content_type = field.content_type()
            .unwrap_or("application/octet-stream")
            .to_string();

        match field.bytes().await {
            Ok(data) => {
                match state.store.store_file(
                    &original_name,
                    &data,
                    &content_type,
                    &uploader,
                ).await {
                    Ok(upload) => uploaded.push(json!({
                        "id": upload.id,
                        "url": upload.url,
                        "name": upload.original_name,
                        "size": upload.size,
                        "mime": upload.mime_type,
                    })),
                    Err(e) => uploaded.push(json!({
                        "error": e,
                        "name": original_name,
                    })),
                }
            }
            Err(e) => {
                uploaded.push(json!({
                    "error": format!("Failed to read field: {}", e),
                    "name": original_name,
                }));
            }
        }
    }

    Json(json!({
        "ok": true,
        "files": uploaded,
        "count": uploaded.len(),
    }))
}

async fn api_serve_file(
    State(state): State<UploadState>,
    Path(filename): Path<String>,
) -> axum::response::Response {
    let file_path = state.store.upload_dir.join(&filename);

    if !file_path.exists() || !file_path.starts_with(&state.store.upload_dir) {
        return axum::response::Response::builder()
            .status(404)
            .body(Body::from("File not found"))
            .unwrap();
    }

    let ext = std::path::Path::new(&filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");

    let mime = UploadStore::mime_from_ext(ext);

    match tokio::fs::read(&file_path).await {
        Ok(data) => {
            axum::response::Response::builder()
                .header("Content-Type", mime)
                .header("Content-Disposition", format!("inline; filename=\"{}\"", filename))
                .header("Cache-Control", "public, max-age=86400")
                .body(Body::from(data))
                .unwrap()
        }
        Err(_) => {
            axum::response::Response::builder()
                .status(500)
                .body(Body::from("Failed to read file"))
                .unwrap()
        }
    }
}

async fn api_list_uploads(State(state): State<UploadState>) -> Json<Value> {
    let uploads = state.store.list().await;
    Json(json!({
        "uploads": uploads,
        "count": uploads.len(),
    }))
}

async fn api_delete_upload(
    State(state): State<UploadState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let ok = state.store.delete(&id).await;
    Json(json!({"ok": ok}))
}

async fn api_upload_status(State(state): State<UploadState>) -> Json<Value> {
    let uploads = state.store.list().await;
    let total_size: usize = uploads.iter().map(|u| u.size).sum();
    Json(json!({
        "total_files": uploads.len(),
        "total_size_bytes": total_size,
        "total_size_mb": total_size / 1024 / 1024,
        "max_file_size_mb": state.store.max_file_size / 1024 / 1024,
        "upload_dir": state.store.upload_dir.to_string_lossy(),
    }))
}

async fn serve_upload_page() -> Html<&'static str> {
    Html(UPLOAD_HTML)
}

const UPLOAD_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HIVE Upload</title>
    <style>
        *{margin:0;padding:0;box-sizing:border-box}
        body{font-family:'Inter',system-ui,sans-serif;background:#08080d;color:#e0e0e8;min-height:100vh;display:flex;align-items:center;justify-content:center;flex-direction:column}
        .drop-zone{width:400px;height:250px;border:2px dashed rgba(255,193,7,0.3);border-radius:20px;display:flex;align-items:center;justify-content:center;flex-direction:column;gap:12px;cursor:pointer;transition:all .3s;background:rgba(255,255,255,0.02)}
        .drop-zone:hover,.drop-zone.active{border-color:#ffc107;background:rgba(255,193,7,0.05);transform:scale(1.02)}
        .drop-zone h2{font-size:24px;background:linear-gradient(135deg,#ffc107,#ff9800);-webkit-background-clip:text;-webkit-text-fill-color:transparent}
        .drop-zone p{color:#888;font-size:13px}
        input[type=file]{display:none}
        .results{margin-top:20px;width:400px}
        .file-item{padding:10px 16px;background:rgba(255,255,255,0.04);border-radius:12px;margin-bottom:8px;font-size:13px;display:flex;justify-content:space-between;align-items:center}
        .file-url{color:#ffc107;font-size:11px;word-break:break-all}
        .status{margin-top:20px;color:#555;font-size:11px;text-align:center}
    </style>
</head>
<body>
    <div class="drop-zone" id="drop-zone" onclick="document.getElementById('file-input').click()">
        <h2>📤 Upload Files</h2>
        <p>Drop files here or click to browse</p>
        <p style="font-size:11px;color:#555">Max 50MB per file • Images, documents, media</p>
    </div>
    <input type="file" id="file-input" multiple onchange="uploadFiles(this.files)">
    <div class="results" id="results"></div>
    <div class="status" id="status"></div>
<script>
const dz = document.getElementById('drop-zone');
dz.addEventListener('dragover', e => { e.preventDefault(); dz.classList.add('active'); });
dz.addEventListener('dragleave', () => dz.classList.remove('active'));
dz.addEventListener('drop', e => { e.preventDefault(); dz.classList.remove('active'); uploadFiles(e.dataTransfer.files); });

async function uploadFiles(files) {
    const fd = new FormData();
    for (const f of files) fd.append('file', f);
    const res = await fetch('/upload', { method: 'POST', body: fd });
    const data = await res.json();
    const el = document.getElementById('results');
    el.innerHTML = (data.files || []).map(f => f.error
        ? `<div class="file-item" style="border:1px solid #ef5350">${f.name}: ${f.error}</div>`
        : `<div class="file-item"><span>${f.name} (${(f.size/1024).toFixed(1)}KB)</span><div class="file-url">${f.url}</div></div>`
    ).join('');
}

fetch('/api/status').then(r=>r.json()).then(d=>{
    document.getElementById('status').textContent = `${d.total_files} files • ${d.total_size_mb}MB stored`;
});
</script>
</body>
</html>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_mime_types() {
        assert!(UploadStore::is_allowed_mime("image/jpeg"));
        assert!(UploadStore::is_allowed_mime("image/png"));
        assert!(UploadStore::is_allowed_mime("video/mp4"));
        assert!(UploadStore::is_allowed_mime("application/pdf"));
        assert!(UploadStore::is_allowed_mime("text/plain"));
        assert!(!UploadStore::is_allowed_mime("application/x-executable"));
        assert!(!UploadStore::is_allowed_mime("application/x-sharedlib"));
    }

    #[test]
    fn test_ext_from_name() {
        assert_eq!(UploadStore::ext_from_name("photo.jpg"), "jpg");
        assert_eq!(UploadStore::ext_from_name("document.PDF"), "pdf");
        assert_eq!(UploadStore::ext_from_name("noext"), "bin");
        assert_eq!(UploadStore::ext_from_name("archive.tar.gz"), "gz");
    }

    #[test]
    fn test_mime_from_ext() {
        assert_eq!(UploadStore::mime_from_ext("jpg"), "image/jpeg");
        assert_eq!(UploadStore::mime_from_ext("png"), "image/png");
        assert_eq!(UploadStore::mime_from_ext("mp4"), "video/mp4");
        assert_eq!(UploadStore::mime_from_ext("pdf"), "application/pdf");
        assert_eq!(UploadStore::mime_from_ext("unknown"), "application/octet-stream");
    }

    #[test]
    fn test_upload_html_not_empty() {
        assert!(UPLOAD_HTML.len() > 500);
        assert!(UPLOAD_HTML.contains("Upload"));
        assert!(UPLOAD_HTML.contains("/upload"));
    }

    #[tokio::test]
    async fn test_upload_store_creation() {
        let store = UploadStore {
            uploads: tokio::sync::RwLock::new(Vec::new()),
            upload_dir: PathBuf::from("/tmp/hive_test_uploads"),
            persist_path: PathBuf::from("/tmp/hive_test_uploads_meta.json"),
            max_file_size: 50 * 1024 * 1024,
            port: 8421,
        };
        assert_eq!(store.list().await.len(), 0);
    }

    #[tokio::test]
    async fn test_upload_store_file() {
        let upload_dir = PathBuf::from(format!("/tmp/hive_test_uploads_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&upload_dir);

        let store = UploadStore {
            uploads: tokio::sync::RwLock::new(Vec::new()),
            upload_dir: upload_dir.clone(),
            persist_path: PathBuf::from(format!("/tmp/hive_test_up_meta_{}.json", std::process::id())),
            max_file_size: 50 * 1024 * 1024,
            port: 8421,
        };

        let result = store.store_file(
            "test.txt",
            b"Hello, HIVE!",
            "text/plain",
            "test_user",
        ).await;

        assert!(result.is_ok());
        let upload = result.unwrap();
        assert_eq!(upload.original_name, "test.txt");
        assert_eq!(upload.size, 12);
        assert_eq!(upload.mime_type, "text/plain");
        assert!(upload.url.contains("localhost:8421"));

        assert_eq!(store.list().await.len(), 1);

        // Cleanup
        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    #[tokio::test]
    async fn test_upload_file_too_large() {
        let store = UploadStore {
            uploads: tokio::sync::RwLock::new(Vec::new()),
            upload_dir: PathBuf::from("/tmp/hive_test_uploads_size"),
            persist_path: PathBuf::from("/tmp/hive_test_up_size.json"),
            max_file_size: 10, // 10 bytes max
            port: 8421,
        };

        let result = store.store_file(
            "big.txt",
            b"This is more than 10 bytes of data!",
            "text/plain",
            "test_user",
        ).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too large"));
    }

    #[tokio::test]
    async fn test_upload_blocked_mime() {
        let store = UploadStore {
            uploads: tokio::sync::RwLock::new(Vec::new()),
            upload_dir: PathBuf::from("/tmp/hive_test_uploads_mime"),
            persist_path: PathBuf::from("/tmp/hive_test_up_mime.json"),
            max_file_size: 50 * 1024 * 1024,
            port: 8421,
        };

        let result = store.store_file(
            "evil.exe",
            b"MZ",
            "application/x-executable",
            "attacker",
        ).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not allowed"));
    }

    #[tokio::test]
    async fn test_upload_delete() {
        let upload_dir = PathBuf::from(format!("/tmp/hive_test_uploads_del_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&upload_dir);

        let store = UploadStore {
            uploads: tokio::sync::RwLock::new(Vec::new()),
            upload_dir: upload_dir.clone(),
            persist_path: PathBuf::from(format!("/tmp/hive_test_up_del_{}.json", std::process::id())),
            max_file_size: 50 * 1024 * 1024,
            port: 8421,
        };

        let upload = store.store_file("del.txt", b"data", "text/plain", "user").await.unwrap();
        assert_eq!(store.list().await.len(), 1);

        assert!(store.delete(&upload.id).await);
        assert_eq!(store.list().await.len(), 0);

        // Double-delete returns false
        assert!(!store.delete(&upload.id).await);

        let _ = std::fs::remove_dir_all(&upload_dir);
    }
}
