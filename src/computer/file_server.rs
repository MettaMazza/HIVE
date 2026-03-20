use std::path::PathBuf;
use tokio::fs;

/// Reads the current public tunnel URL from `memory/core/tunnel_url.txt`.
/// Returns the base URL (e.g. `https://xxxx.localhost.run`) or falls back to localhost.
pub fn get_public_base_url() -> String {
    let tunnel_path = std::path::Path::new("memory/core/tunnel_url.txt");
    if let Ok(url) = std::fs::read_to_string(tunnel_path) {
        let url = url.trim().to_string();
        if !url.is_empty() {
            return url;
        }
    }
    let port: u16 = std::env::var("HIVE_FILE_SERVER_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8420);
    format!("http://localhost:{}", port)
}

/// A simple static file server that serves generated/downloaded files over HTTP.
/// Used so the admin can access HIVE outputs remotely.
pub struct FileServer {
    pub port: u16,
    pub served_dirs: Vec<PathBuf>,
    pub auth_token: String,
}

impl FileServer {
    pub fn new(port: u16, auth_token: String) -> Self {
        Self {
            port,
            served_dirs: vec![
                PathBuf::from("memory/core/docs/rendered"),
                PathBuf::from("memory/core/downloads"),
            ],
            auth_token,
        }
    }

    /// Starts the file server. This function blocks (runs forever).
    /// Spawn it with `tokio::spawn`.
    #[cfg(not(tarpaulin_include))]
    pub async fn run(self) -> std::io::Result<()> {
        use std::net::SocketAddr;
        let addr = SocketAddr::from(([0, 0, 0, 0], self.port));
        let served_dirs = self.served_dirs.clone();
        let auth_token = self.auth_token.clone();

        tracing::info!("[FILE SERVER] 📂 Starting on http://0.0.0.0:{}", self.port);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("[FILE SERVER] ✅ Listening on port {}", self.port);

        loop {
            let (stream, _) = listener.accept().await?;
            let dirs = served_dirs.clone();
            let token = auth_token.clone();

            tokio::spawn(async move {
                let _ = handle_connection(stream, &dirs, &token).await;
            });
        }
    }
}

/// Handles a single HTTP request — minimal HTTP/1.1 parser.
#[cfg(not(tarpaulin_include))]
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    dirs: &[PathBuf],
    auth_token: &str,
) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]).to_string();

    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n";
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    let path = parts[1];

    // Auth check: require ?token=<TOKEN>
    if !auth_token.is_empty() {
        let has_token = path.contains(&format!("token={}", auth_token));
        if !has_token {
            let body = "401 Unauthorized — token required";
            let resp = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                body.len(), body
            );
            stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    }

    let clean_path = path.split('?').next().unwrap_or(path);
    let token_param = if !auth_token.is_empty() {
        format!("?token={}", auth_token)
    } else {
        String::new()
    };

    // GET / or /files/ — HTML file browser
    if clean_path == "/" || clean_path == "/files/" || clean_path == "/files" {
        let mut file_rows = String::new();
        let mut total_files = 0u32;
        let mut total_bytes = 0u64;
        for dir in dirs {
            let dir_label = if dir.to_string_lossy().contains("downloads") { "📥 Downloads" }
                else if dir.to_string_lossy().contains("rendered") { "📄 Documents" }
                else { "📁 Files" };
            if let Ok(mut reader) = fs::read_dir(dir).await {
                while let Ok(Some(entry)) = reader.next_entry().await {
                    if let Ok(meta) = entry.metadata().await {
                        if meta.is_file() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            let size = meta.len();
                            total_files += 1;
                            total_bytes += size;
                            let size_str = if size > 1_048_576 {
                                format!("{:.1} MB", size as f64 / 1_048_576.0)
                            } else if size > 1024 {
                                format!("{:.1} KB", size as f64 / 1024.0)
                            } else {
                                format!("{} B", size)
                            };
                            let icon = if name.ends_with(".pdf") { "📕" }
                                else if name.ends_with(".json") { "📋" }
                                else if name.ends_with(".html") { "🌐" }
                                else if name.ends_with(".md") || name.ends_with(".txt") { "📝" }
                                else if name.ends_with(".csv") { "📊" }
                                else if name.ends_with(".png") || name.ends_with(".jpg") { "🖼️" }
                                else { "📄" };
                            file_rows.push_str(&format!(
                                "<tr><td>{} {}</td><td>{}</td><td>{}</td><td><a href=\"/files/{}{}\" class=\"dl\">Download</a></td></tr>\n",
                                icon, html_esc(&name), size_str, dir_label, html_esc(&name), token_param
                            ));
                        }
                    }
                }
            }
        }
        if file_rows.is_empty() {
            file_rows = "<tr><td colspan=\"4\" style=\"text-align:center;opacity:0.5;padding:40px;\">No files yet — use Apis to generate or download files</td></tr>".to_string();
        }
        let body = format!(r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>HIVE File Server</title>
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:#0f0f1a;color:#e0e0e0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;padding:20px}}
.container{{max-width:900px;margin:0 auto}}
h1{{font-size:28px;margin-bottom:4px;color:#fbbf24}}
.subtitle{{color:#888;font-size:14px;margin-bottom:24px}}
.stats{{display:flex;gap:20px;margin-bottom:24px}}
.stat{{background:#1a1a2e;border:1px solid #2a2a4a;border-radius:8px;padding:12px 20px}}
.stat-val{{font-size:22px;font-weight:700;color:#fbbf24}}
.stat-label{{font-size:12px;color:#888}}
table{{width:100%;border-collapse:collapse;background:#1a1a2e;border-radius:8px;overflow:hidden}}
th{{background:#16213e;text-align:left;padding:12px 16px;font-size:12px;text-transform:uppercase;letter-spacing:1px;color:#888}}
td{{padding:10px 16px;border-top:1px solid #1e1e3a;font-size:14px}}
tr:hover{{background:#16213e}}
.dl{{background:#fbbf24;color:#0f0f1a;padding:4px 12px;border-radius:4px;text-decoration:none;font-weight:600;font-size:12px}}
.dl:hover{{background:#f59e0b}}
.footer{{text-align:center;margin-top:24px;font-size:12px;color:#555}}
</style></head><body>
<div class="container">
<h1>🐝 HIVE File Server</h1>
<p class="subtitle">Admin-only file browser — generated documents &amp; downloads</p>
<div class="stats">
<div class="stat"><div class="stat-val">{}</div><div class="stat-label">Files</div></div>
<div class="stat"><div class="stat-val">{}</div><div class="stat-label">Total Size</div></div>
</div>
<table><thead><tr><th>File</th><th>Size</th><th>Source</th><th></th></tr></thead>
<tbody>{}</tbody></table>
<div class="footer">HIVE File Server • Admin Only • Token Protected</div>
</div></body></html>"#,
            total_files,
            if total_bytes > 1_048_576 { format!("{:.1} MB", total_bytes as f64 / 1_048_576.0) }
            else if total_bytes > 1024 { format!("{:.1} KB", total_bytes as f64 / 1024.0) }
            else { format!("{} B", total_bytes) },
            file_rows
        );
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            body.len(), body
        );
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    // GET /files/:filename — serve a specific file
    if let Some(filename) = clean_path.strip_prefix("/files/") {
        let filename = filename.trim_start_matches('/');
        // Security: strip path traversal
        let safe_name = filename.replace("..", "").replace('/', "");
        if safe_name.is_empty() {
            let body = "400 Bad Request — invalid filename";
            let resp = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                body.len(), body
            );
            stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }

        // Search all served dirs for the file
        for dir in dirs {
            let file_path = dir.join(&safe_name);
            if file_path.exists() {
                if let Ok(data) = fs::read(&file_path).await {
                    let content_type = guess_content_type(&safe_name);
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"{}\"\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
                        content_type, data.len(), safe_name
                    );
                    stream.write_all(resp.as_bytes()).await?;
                    stream.write_all(&data).await?;
                    return Ok(());
                }
            }
        }

        let body = "404 Not Found";
        let resp = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
            body.len(), body
        );
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    // Redirect anything else to /files/
    let resp = format!(
        "HTTP/1.1 302 Found\r\nLocation: /files/{}\r\nContent-Length: 0\r\n\r\n",
        token_param
    );
    stream.write_all(resp.as_bytes()).await?;
    Ok(())
}

/// Minimal HTML escape for safe rendering in the file browser.
fn html_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn guess_content_type(filename: &str) -> &'static str {
    if filename.ends_with(".pdf") { "application/pdf" }
    else if filename.ends_with(".html") { "text/html" }
    else if filename.ends_with(".json") { "application/json" }
    else if filename.ends_with(".csv") { "text/csv" }
    else if filename.ends_with(".md") || filename.ends_with(".txt") { "text/plain" }
    else if filename.ends_with(".png") { "image/png" }
    else if filename.ends_with(".jpg") || filename.ends_with(".jpeg") { "image/jpeg" }
    else if filename.ends_with(".mp3") { "audio/mpeg" }
    else if filename.ends_with(".wav") { "audio/wav" }
    else if filename.ends_with(".zip") { "application/zip" }
    else { "application/octet-stream" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guess_content_type() {
        assert_eq!(guess_content_type("report.pdf"), "application/pdf");
        assert_eq!(guess_content_type("data.json"), "application/json");
        assert_eq!(guess_content_type("notes.txt"), "text/plain");
        assert_eq!(guess_content_type("page.html"), "text/html");
        assert_eq!(guess_content_type("unknown.xyz"), "application/octet-stream");
    }

    #[test]
    fn test_file_server_new() {
        let server = FileServer::new(8420, "test_token".to_string());
        assert_eq!(server.port, 8420);
        assert_eq!(server.served_dirs.len(), 2);
        assert_eq!(server.auth_token, "test_token");
    }
}
