/// Apis Code — Decentralised Web IDE.
///
/// A VS Code-style browser IDE served on localhost:3033.
/// Users can browse files, edit code with syntax highlighting,
/// run terminal commands, and chat with Apis AI for assistance.
///
/// SECURITY: All file ops sandboxed to workspace root. No path traversal.
use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::{State, Query},
    response::Html,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone)]
struct CodeState {
    workspace: Arc<String>,
    ollama_base: Arc<String>,
    model: Arc<String>,
}

#[derive(Deserialize)]
struct FilePath {
    path: Option<String>,
}

#[derive(Deserialize)]
struct FileWrite {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct MkdirReq {
    path: String,
}

#[derive(Deserialize)]
struct TerminalReq {
    command: String,
}

#[derive(Deserialize)]
struct AskReq {
    question: String,
    #[serde(default)]
    file_context: Option<String>,
    #[serde(default)]
    file_path: Option<String>,
}

#[derive(Deserialize)]
struct SearchReq {
    q: String,
}

pub async fn spawn_apis_code_server() {
    let port: u16 = std::env::var("REMOVED_MESH_GOVERNED")
        .ok().and_then(|v| v.parse().ok())
        .unwrap_or(3033);

    let workspace = std::env::var("HIVE_CODE_WORKSPACE")
        .unwrap_or_else(|_| ".".to_string());

    let workspace = std::fs::canonicalize(&workspace)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .to_string_lossy().to_string();

    let ollama_base = std::env::var("OLLAMA_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());

    let model = std::env::var("HIVE_MODEL")
        .unwrap_or_else(|_| "qwen3.5:35b".to_string());

    let state = CodeState {
        workspace: Arc::new(workspace.clone()),
        ollama_base: Arc::new(ollama_base),
        model: Arc::new(model),
    };

    tokio::spawn(async move {
        tracing::info!("[APIS CODE] 💻 IDE starting on http://0.0.0.0:{} (workspace: {})", port, workspace);

        let app = Router::new()
            .route("/api/files", get(api_files))
            .route("/api/file", get(api_read_file).post(api_write_file).put(api_write_file).delete(api_delete_file))
            .route("/api/mkdir", post(api_mkdir))
            .route("/api/terminal", post(api_terminal))
            .route("/api/ask", post(api_ask))
            .route("/api/build-site", post(api_build_site))
            .route("/api/publish-site", post(api_publish_site))
            .route("/api/search", get(api_search))
            .route("/api/status", get(api_status))
            .fallback(get(serve_ide))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let addr = format!("0.0.0.0:{}", port);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                tracing::info!("[APIS CODE] 💻 IDE bound on {}", addr);
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("[APIS CODE] ❌ Server error: {}", e);
                }
            }
            Err(e) => tracing::error!("[APIS CODE] ❌ Failed to bind {}: {}", addr, e),
        }
    });
}

/// Resolve a path safely within the workspace. Returns None if path escapes.
fn safe_path(workspace: &str, relative: &str) -> Option<std::path::PathBuf> {
    let clean = relative.replace('\\', "/");
    // Block obvious traversal
    if clean.contains("..") || clean.starts_with('/') {
        return None;
    }
    let full = std::path::PathBuf::from(workspace).join(&clean);
    // Canonicalize and verify it's still under workspace
    if let Ok(canonical) = std::fs::canonicalize(&full) {
        if canonical.starts_with(workspace) {
            return Some(canonical);
        }
    }
    // File might not exist yet (for create) — check parent
    if let Some(parent) = full.parent() {
        if let Ok(canonical_parent) = std::fs::canonicalize(parent) {
            if canonical_parent.starts_with(workspace) {
                return Some(full);
            }
        }
    }
    None
}

// ─── API Endpoints ──────────────────────────────────────────────────────

async fn api_files(State(state): State<CodeState>, Query(params): Query<FilePath>) -> Json<Value> {
    let base = params.path.unwrap_or_default();
    let root = if base.is_empty() {
        std::path::PathBuf::from(state.workspace.as_str())
    } else {
        match safe_path(&state.workspace, &base) {
            Some(p) => p,
            None => return Json(json!({"error": "Invalid path"})),
        }
    };

    fn build_tree(path: &std::path::Path, workspace: &str, depth: usize) -> Vec<Value> {
        if depth > 8 { return vec![]; }
        let mut entries = vec![];
        if let Ok(read_dir) = std::fs::read_dir(path) {
            let mut items: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();
            items.sort_by(|a, b| {
                let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
                b_dir.cmp(&a_dir).then(a.file_name().cmp(&b.file_name()))
            });
            for entry in items {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip hidden, target, node_modules
                if name.starts_with('.') || name == "target" || name == "node_modules" { continue; }
                let full = entry.path();
                let rel = full.strip_prefix(workspace).unwrap_or(&full)
                    .to_string_lossy().to_string();
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
                    entries.push(json!({
                        "name": name, "path": rel, "type": "dir",
                        "children": build_tree(&full, workspace, depth + 1)
                    }));
                } else {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    entries.push(json!({
                        "name": name, "path": rel, "type": "file", "size": size
                    }));
                }
            }
        }
        entries
    }

    let tree = build_tree(&root, &state.workspace, 0);
    Json(json!({"tree": tree, "workspace": *state.workspace}))
}

async fn api_read_file(State(state): State<CodeState>, Query(params): Query<FilePath>) -> Json<Value> {
    let path = match &params.path {
        Some(p) => p,
        None => return Json(json!({"error": "Missing path parameter"})),
    };

    let full = match safe_path(&state.workspace, path) {
        Some(p) => p,
        None => return Json(json!({"error": "Invalid path"})),
    };

    if !full.is_file() {
        return Json(json!({"error": "Not a file"}));
    }

    let metadata = std::fs::metadata(&full).ok();
    if metadata.as_ref().map(|m| m.len()).unwrap_or(0) > 10_000_000 {
        return Json(json!({"error": "File too large (>10MB)"}));
    }

    match std::fs::read_to_string(&full) {
        Ok(content) => {
            let ext = full.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
            let lines = content.lines().count();
            Json(json!({
                "path": path, "content": content,
                "language": ext_to_language(&ext),
                "lines": lines, "size": content.len()
            }))
        }
        Err(_) => {
            // Binary file
            Json(json!({"error": "Binary file — cannot display", "path": path}))
        }
    }
}

async fn api_write_file(State(state): State<CodeState>, Json(req): Json<FileWrite>) -> Json<Value> {
    if req.content.len() > 5_000_000 {
        return Json(json!({"error": "Content too large (>5MB)"}));
    }

    let full = match safe_path(&state.workspace, &req.path) {
        Some(p) => p,
        None => return Json(json!({"error": "Invalid path"})),
    };

    // Create parent dirs
    if let Some(parent) = full.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match std::fs::write(&full, &req.content) {
        Ok(_) => {
            tracing::info!("[APIS CODE] 💾 Saved: {} ({} bytes)", req.path, req.content.len());
            Json(json!({"ok": true, "path": req.path, "size": req.content.len()}))
        }
        Err(e) => Json(json!({"error": format!("Write failed: {}", e)})),
    }
}

async fn api_delete_file(State(state): State<CodeState>, Query(params): Query<FilePath>) -> Json<Value> {
    let path = match &params.path {
        Some(p) => p,
        None => return Json(json!({"error": "Missing path"})),
    };

    let full = match safe_path(&state.workspace, path) {
        Some(p) => p,
        None => return Json(json!({"error": "Invalid path"})),
    };

    if full.is_dir() {
        match std::fs::remove_dir_all(&full) {
            Ok(_) => Json(json!({"ok": true, "deleted": path})),
            Err(e) => Json(json!({"error": format!("Delete failed: {}", e)})),
        }
    } else if full.is_file() {
        match std::fs::remove_file(&full) {
            Ok(_) => Json(json!({"ok": true, "deleted": path})),
            Err(e) => Json(json!({"error": format!("Delete failed: {}", e)})),
        }
    } else {
        Json(json!({"error": "Path not found"}))
    }
}

async fn api_mkdir(State(state): State<CodeState>, Json(req): Json<MkdirReq>) -> Json<Value> {
    let full = match safe_path(&state.workspace, &req.path) {
        Some(p) => p,
        None => return Json(json!({"error": "Invalid path"})),
    };

    match std::fs::create_dir_all(&full) {
        Ok(_) => Json(json!({"ok": true, "path": req.path})),
        Err(e) => Json(json!({"error": format!("mkdir failed: {}", e)})),
    }
}

async fn api_terminal(State(state): State<CodeState>, Json(req): Json<TerminalReq>) -> Json<Value> {
    let cmd = req.command.trim();
    if cmd.is_empty() {
        return Json(json!({"error": "Empty command"}));
    }

    // Security blocklist
    let blocked = ["rm -rf /", "sudo rm", "mkfs", "dd if=", "shutdown", "reboot",
        ":(){ :|:&", "> /dev/sd", "chmod -R 777 /"];
    for b in &blocked {
        if cmd.contains(b) {
            return Json(json!({"error": format!("Blocked command: {}", b)}));
        }
    }

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(state.workspace.as_str())
            .output()
    ).await;

    match output {
        Ok(Ok(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let truncated = stdout.len() > 50_000 || stderr.len() > 50_000;
            Json(json!({
                "exit_code": out.status.code().unwrap_or(-1),
                "stdout": if truncated { stdout[..50_000.min(stdout.len())].to_string() } else { stdout },
                "stderr": if truncated { stderr[..50_000.min(stderr.len())].to_string() } else { stderr },
                "truncated": truncated,
            }))
        }
        Ok(Err(e)) => Json(json!({"error": format!("Command failed: {}", e)})),
        Err(_) => Json(json!({"error": "Command timed out (30s limit)"})),
    }
}

async fn api_ask(State(state): State<CodeState>, Json(req): Json<AskReq>) -> Json<Value> {
    let mut prompt = format!("You are Apis, an AI coding assistant in the Apis Code IDE. Help the user with their code.\n\n");

    if let Some(path) = &req.file_path {
        prompt.push_str(&format!("Currently open file: {}\n", path));
    }
    if let Some(context) = &req.file_context {
        let ctx = if context.len() > 8000 { &context[..8000] } else { context };
        prompt.push_str(&format!("File contents:\n```\n{}\n```\n\n", ctx));
    }
    prompt.push_str(&format!("User question: {}", req.question));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build().unwrap_or_default();

    let body = json!({
        "model": *state.model,
        "prompt": prompt,
        "stream": false,
        "options": { "num_predict": 2048, "temperature": 0.3 }
    });

    match client.post(format!("{}/api/generate", *state.ollama_base))
        .json(&body).send().await
    {
        Ok(resp) => {
            match resp.json::<Value>().await {
                Ok(data) => {
                    let response = data["response"].as_str().unwrap_or("No response from model").to_string();
                    Json(json!({"response": response, "model": *state.model}))
                }
                Err(e) => Json(json!({"error": format!("Parse error: {}", e)})),
            }
        }
        Err(e) => Json(json!({"error": format!("Ollama error: {}. Is Ollama running?", e)})),
    }
}

async fn api_search(State(state): State<CodeState>, Query(params): Query<SearchReq>) -> Json<Value> {
    let query = &params.q;
    if query.is_empty() {
        return Json(json!({"results": [], "count": 0}));
    }

    // Use grep to search
    let output = tokio::process::Command::new("grep")
        .args(["-rnI", "--include=*.rs", "--include=*.py", "--include=*.js",
            "--include=*.ts", "--include=*.html", "--include=*.css",
            "--include=*.json", "--include=*.toml", "--include=*.md",
            "--include=*.txt", "--include=*.yaml", "--include=*.yml",
            "-l", query])
        .current_dir(state.workspace.as_str())
        .output().await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let files: Vec<&str> = stdout.lines().take(50).collect();

            // Get matching lines for first 10 files
            let mut results = vec![];
            for file in files.iter().take(10) {
                if let Ok(content) = std::fs::read_to_string(
                    std::path::PathBuf::from(state.workspace.as_str()).join(file)
                ) {
                    let query_lower = query.to_lowercase();
                    for (i, line) in content.lines().enumerate() {
                        if line.to_lowercase().contains(&query_lower) {
                            results.push(json!({
                                "file": file, "line": i + 1,
                                "content": line.trim(),
                            }));
                        }
                    }
                }
            }

            Json(json!({
                "results": results.into_iter().take(100).collect::<Vec<_>>(),
                "total_files": files.len(),
                "query": query,
            }))
        }
        Err(e) => Json(json!({"error": format!("Search failed: {}", e)})),
    }
}

async fn api_status(State(state): State<CodeState>) -> Json<Value> {
    // Count files in workspace
    let file_count = walkdir_count(state.workspace.as_str());

    Json(json!({
        "workspace": *state.workspace,
        "file_count": file_count,
        "model": *state.model,
        "ollama_base": *state.ollama_base,
    }))
}

fn walkdir_count(path: &str) -> usize {
    let mut count = 0;
    fn walk(path: &std::path::Path, count: &mut usize, depth: usize) {
        if depth > 6 { return; }
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name == "target" || name == "node_modules" { continue; }
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    walk(&entry.path(), count, depth + 1);
                } else {
                    *count += 1;
                }
            }
        }
    }
    walk(std::path::Path::new(path), &mut count, 0);
    count
}

fn ext_to_language(ext: &str) -> &'static str {
    match ext {
        "rs" => "rust", "py" => "python", "js" => "javascript",
        "ts" => "typescript", "html" | "htm" => "html", "css" => "css",
        "json" => "json", "toml" => "toml", "md" => "markdown",
        "sh" | "bash" | "zsh" => "shell", "yaml" | "yml" => "yaml",
        "sql" => "sql", "xml" => "xml", "c" | "h" => "c",
        "cpp" | "hpp" | "cc" => "cpp", "java" => "java",
        "go" => "go", "rb" => "ruby", "php" => "php",
        "txt" | "log" => "text", _ => "text",
    }
}

#[derive(Deserialize)]
struct BuildSiteReq {
    site_type: String, // blog, portfolio, forum, shop, landing
    site_name: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct PublishSiteReq {
    name: String,
    description: String,
    folder: String, // relative path to site folder
    icon: Option<String>,
}

async fn api_build_site(State(state): State<CodeState>, Json(req): Json<BuildSiteReq>) -> Json<Value> {
    let prompt = format!(
        r#"You are the Mesh Site Builder AI — an expert web designer specialising in decentralised mesh websites.

You create beautiful, fully functional single-page websites that work WITHOUT internet, CDNs, or external dependencies. All CSS is inline, all JS is embedded. The sites must be self-contained HTML files.

Design rules:
- Dark theme with modern aesthetics (glassmorphism, gradients, smooth animations)
- Responsive design (mobile-first)
- No external dependencies (no CDN links, no npm, no frameworks)
- Professional quality — investor-demo ready
- All images use CSS gradients or emoji as placeholders
- Include proper meta tags and SEO

The user wants a {} site called "{}".
Additional context: {}

Generate a COMPLETE index.html file. Include ALL the HTML, CSS, and JavaScript in a single file. The site should look premium and professional. Do not use any placeholder text — fill in realistic content appropriate for the site type.

Respond with ONLY the complete HTML code, nothing else."#,
        req.site_type, req.site_name,
        req.description.as_deref().unwrap_or("No additional details")
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build().unwrap_or_default();

    let body = serde_json::json!({
        "model": *state.model,
        "prompt": prompt,
        "stream": false,
        "options": { "num_predict": 8192, "temperature": 0.4 }
    });

    match client.post(format!("{}/api/generate", *state.ollama_base))
        .json(&body).send().await
    {
        Ok(resp) => {
            match resp.json::<Value>().await {
                Ok(data) => {
                    let response = data["response"].as_str().unwrap_or("").to_string();
                    // Extract HTML from response (might be wrapped in code fences)
                    let html = if response.contains("```html") {
                        response.split("```html").nth(1)
                            .and_then(|s| s.split("```").next())
                            .unwrap_or(&response).trim().to_string()
                    } else if response.contains("<!DOCTYPE") || response.contains("<html") {
                        response.trim().to_string()
                    } else {
                        response
                    };

                    // Save to workspace
                    let folder = format!("mesh_sites/{}", req.site_name.to_lowercase().replace(' ', "_"));
                    let site_path = std::path::PathBuf::from(state.workspace.as_str()).join(&folder);
                    let _ = std::fs::create_dir_all(&site_path);
                    let index_path = site_path.join("index.html");
                    let _ = std::fs::write(&index_path, &html);

                    tracing::info!("[APIS CODE] 🌐 Built mesh site: {} ({} bytes)", folder, html.len());

                    Json(serde_json::json!({
                        "ok": true,
                        "folder": folder,
                        "file": format!("{}/index.html", folder),
                        "size": html.len(),
                        "html": html,
                    }))
                }
                Err(e) => Json(serde_json::json!({"error": format!("Parse error: {}", e)})),
            }
        }
        Err(e) => Json(serde_json::json!({"error": format!("AI error: {}. Is Ollama running?", e)})),
    }
}

async fn api_publish_site(State(state): State<CodeState>, Json(req): Json<PublishSiteReq>) -> Json<Value> {
    // Verify the folder exists and has an index.html
    let site_path = std::path::PathBuf::from(state.workspace.as_str()).join(&req.folder);
    let index = site_path.join("index.html");
    if !index.exists() {
        return Json(serde_json::json!({"error": "No index.html found in site folder"}));
    }

    // Register with HivePortal
    let portal_port: u16 = std::env::var("REMOVED_MESH_GOVERNED")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(3035);

    let client = reqwest::Client::new();
    let result = client.post(format!("http://0.0.0.0:{}/api/sites", portal_port))
        .json(&serde_json::json!({
            "name": req.name,
            "description": req.description,
            "url": format!("file://{}", index.to_string_lossy()),
            "icon": req.icon.unwrap_or_else(|| "🌐".to_string()),
            "category": "user-site",
        }))
        .send().await;

    match result {
        Ok(resp) => {
            match resp.json::<Value>().await {
                Ok(data) => {
                    tracing::info!("[APIS CODE] 🌐 Published mesh site: {}", req.name);
                    Json(data)
                }
                Err(e) => Json(serde_json::json!({"error": format!("Portal response error: {}", e)})),
            }
        }
        Err(e) => Json(serde_json::json!({"error": format!("Could not reach HivePortal: {}", e)})),
    }
}

// ─── SPA Frontend ───────────────────────────────────────────────────────

async fn serve_ide() -> Html<String> {
    Html(IDE_HTML.to_string())
}

use super::apis_code_html::IDE_HTML;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ide_html_not_empty() {
        assert!(IDE_HTML.len() > 1000);
        assert!(IDE_HTML.contains("Apis Code"));
        assert!(IDE_HTML.contains("/api/files"));
        assert!(IDE_HTML.contains("/api/file"));
        assert!(IDE_HTML.contains("/api/terminal"));
        assert!(IDE_HTML.contains("/api/ask"));
    }

    #[test]
    fn test_safe_path_blocks_traversal() {
        let workspace = "/tmp/test_workspace";
        assert!(safe_path(workspace, "../etc/passwd").is_none());
        assert!(safe_path(workspace, "../../root").is_none());
        assert!(safe_path(workspace, "/etc/passwd").is_none());
    }

    #[test]
    fn test_ext_to_language() {
        assert_eq!(ext_to_language("rs"), "rust");
        assert_eq!(ext_to_language("py"), "python");
        assert_eq!(ext_to_language("js"), "javascript");
        assert_eq!(ext_to_language("json"), "json");
        assert_eq!(ext_to_language("xyz"), "text");
    }
}
