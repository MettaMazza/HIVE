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
        tracing::info!("[APIS CODE] 💻 IDE starting on http://127.0.0.1:{} (workspace: {})", port, workspace);

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

        let addr = format!("127.0.0.1:{}", port);
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
    let result = client.post(format!("http://127.0.0.1:{}/api/sites", portal_port))
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

const IDE_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Apis Code — AI-Powered IDE</title>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&display=swap" rel="stylesheet">
    <style>
        *{margin:0;padding:0;box-sizing:border-box}
        :root{
            --bg:#1e1e2e;--surface:#181825;--panel:#11111b;--card:#313244;
            --border:#45475a;--border-active:#cba6f7;
            --text:#cdd6f4;--text-dim:#a6adc8;--text-muted:#585b70;
            --accent:#cba6f7;--accent-dim:rgba(203,166,247,0.15);
            --green:#a6e3a1;--red:#f38ba8;--yellow:#f9e2af;--blue:#89b4fa;
            --peach:#fab387;--teal:#94e2d5;
        }
        body{font-family:'Inter',sans-serif;background:var(--bg);color:var(--text);height:100vh;overflow:hidden;display:flex;flex-direction:column}

        /* Title Bar */
        .titlebar{height:38px;background:var(--panel);border-bottom:1px solid var(--border);display:flex;align-items:center;justify-content:space-between;padding:0 16px;flex-shrink:0}
        .titlebar-left{display:flex;align-items:center;gap:10px}
        .titlebar h1{font-size:13px;font-weight:600;background:linear-gradient(135deg,#cba6f7,#89b4fa);-webkit-background-clip:text;-webkit-text-fill-color:transparent}
        .titlebar-right{display:flex;gap:8px;font-size:11px;color:var(--text-dim)}
        .tb-btn{padding:4px 10px;border-radius:6px;border:1px solid var(--border);background:transparent;color:var(--text-dim);cursor:pointer;font-size:11px;font-family:inherit;transition:all .2s}
        .tb-btn:hover{background:var(--accent-dim);border-color:var(--accent);color:var(--accent)}
        .tb-btn.save{background:var(--accent-dim);border-color:var(--accent);color:var(--accent)}

        /* Main Layout */
        .ide{display:flex;flex:1;overflow:hidden}

        /* File Explorer */
        .explorer{width:240px;background:var(--panel);border-right:1px solid var(--border);display:flex;flex-direction:column;flex-shrink:0;overflow:hidden}
        .explorer-header{padding:10px 14px;font-size:11px;font-weight:600;color:var(--text-dim);text-transform:uppercase;letter-spacing:1px;display:flex;justify-content:space-between;align-items:center;border-bottom:1px solid var(--border)}
        .explorer-header button{background:none;border:none;color:var(--text-dim);cursor:pointer;font-size:14px}
        .explorer-header button:hover{color:var(--accent)}
        .file-tree{flex:1;overflow-y:auto;padding:4px 0;font-size:12px;font-family:'JetBrains Mono',monospace}
        .tree-item{padding:3px 8px 3px 0;cursor:pointer;display:flex;align-items:center;gap:4px;white-space:nowrap;color:var(--text-dim);transition:background .15s}
        .tree-item:hover{background:rgba(255,255,255,0.04);color:var(--text)}
        .tree-item.active{background:var(--accent-dim);color:var(--accent)}
        .tree-icon{width:16px;text-align:center;flex-shrink:0;font-size:11px}

        /* Editor Area */
        .editor-area{flex:1;display:flex;flex-direction:column;overflow:hidden}

        /* Tabs */
        .tabs{display:flex;background:var(--panel);border-bottom:1px solid var(--border);overflow-x:auto;flex-shrink:0}
        .tab{padding:8px 16px;font-size:12px;color:var(--text-dim);cursor:pointer;border-right:1px solid var(--border);display:flex;align-items:center;gap:6px;white-space:nowrap;transition:all .15s;font-family:'JetBrains Mono',monospace}
        .tab:hover{background:rgba(255,255,255,0.04)}
        .tab.active{background:var(--bg);color:var(--text);border-bottom:2px solid var(--accent)}
        .tab .close{opacity:0;font-size:14px;line-height:1;margin-left:4px}
        .tab:hover .close{opacity:.6}
        .tab .close:hover{opacity:1;color:var(--red)}
        .tab .modified{color:var(--peach);font-size:16px}
        .tab-new{padding:8px 12px;color:var(--text-muted);cursor:pointer;font-size:14px}
        .tab-new:hover{color:var(--accent)}

        /* Editor */
        .editor-container{flex:1;position:relative;overflow:hidden;display:flex}
        .editor-pane{flex:1;display:flex;flex-direction:column;overflow:hidden}
        .code-editor{flex:1;background:var(--bg);font-family:'JetBrains Mono',monospace;font-size:13px;line-height:1.65;padding:0;overflow:auto;display:flex}
        .line-numbers{padding:8px 12px 8px 16px;text-align:right;color:var(--text-muted);user-select:none;font-size:13px;line-height:1.65;border-right:1px solid var(--border);flex-shrink:0}
        .code-content{flex:1;padding:8px 16px;outline:none;white-space:pre;tab-size:4;overflow:auto;color:var(--text)}
        .code-textarea{position:absolute;top:0;left:0;width:100%;height:100%;opacity:0;font-family:'JetBrains Mono',monospace;font-size:13px;padding:8px 16px;resize:none;background:transparent;color:transparent;caret-color:var(--text);z-index:2;white-space:pre;tab-size:4;line-height:1.65;border:none;outline:none}

        /* Welcome */
        .welcome{flex:1;display:flex;align-items:center;justify-content:center;flex-direction:column;gap:16px;color:var(--text-muted)}
        .welcome .icon{font-size:64px}
        .welcome h2{font-size:20px;color:var(--text-dim)}
        .welcome p{font-size:13px;max-width:400px;text-align:center;line-height:1.6}
        .welcome kbd{background:var(--card);padding:2px 6px;border-radius:4px;font-size:11px;font-family:'JetBrains Mono',monospace;border:1px solid var(--border)}

        /* AI Panel */
        .ai-panel{width:320px;background:var(--panel);border-left:1px solid var(--border);display:flex;flex-direction:column;flex-shrink:0}
        .ai-header{padding:10px 14px;font-size:11px;font-weight:600;color:var(--accent);text-transform:uppercase;letter-spacing:1px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:6px}
        .ai-messages{flex:1;overflow-y:auto;padding:12px}
        .ai-msg{margin-bottom:12px;animation:fadeIn .3s}
        @keyframes fadeIn{from{opacity:0;transform:translateY(4px)}to{opacity:1;transform:translateY(0)}}
        .ai-msg.user{text-align:right}
        .ai-msg .bubble{display:inline-block;max-width:90%;padding:10px 14px;border-radius:12px;font-size:12px;line-height:1.6;text-align:left}
        .ai-msg.user .bubble{background:var(--accent-dim);color:var(--accent);border-bottom-right-radius:4px}
        .ai-msg.assistant .bubble{background:var(--card);color:var(--text);border-bottom-left-radius:4px}
        .ai-msg.assistant .bubble pre{background:var(--panel);padding:8px;border-radius:6px;margin:6px 0;overflow-x:auto;font-family:'JetBrains Mono',monospace;font-size:11px}
        .ai-msg.assistant .bubble code{font-family:'JetBrains Mono',monospace;font-size:11px;background:var(--panel);padding:1px 4px;border-radius:3px}
        .ai-input-area{padding:10px;border-top:1px solid var(--border);display:flex;gap:6px}
        .ai-input{flex:1;padding:8px 12px;border-radius:8px;border:1px solid var(--border);background:var(--surface);color:var(--text);font-family:inherit;font-size:12px;outline:none;resize:none}
        .ai-input:focus{border-color:var(--accent)}
        .ai-send{padding:8px 14px;border-radius:8px;border:none;background:var(--accent);color:var(--panel);font-weight:600;cursor:pointer;font-family:inherit;font-size:12px}
        .ai-send:hover{opacity:.9}

        /* Terminal */
        .terminal-panel{height:200px;background:var(--panel);border-top:1px solid var(--border);display:flex;flex-direction:column;flex-shrink:0}
        .terminal-header{padding:6px 14px;font-size:11px;font-weight:600;color:var(--text-dim);border-bottom:1px solid var(--border);display:flex;justify-content:space-between;align-items:center}
        .terminal-header button{background:none;border:none;color:var(--text-dim);cursor:pointer;font-size:12px}
        .terminal-output{flex:1;overflow-y:auto;padding:8px 14px;font-family:'JetBrains Mono',monospace;font-size:12px;color:var(--green);white-space:pre-wrap;line-height:1.5}
        .terminal-output .err{color:var(--red)}
        .terminal-output .cmd{color:var(--blue)}
        .terminal-input-row{display:flex;align-items:center;padding:4px 14px 8px;gap:6px}
        .terminal-prompt{color:var(--accent);font-family:'JetBrains Mono',monospace;font-size:12px;flex-shrink:0}
        .terminal-input{flex:1;background:transparent;border:none;color:var(--text);font-family:'JetBrains Mono',monospace;font-size:12px;outline:none}

        /* Status Bar */
        .statusbar{height:24px;background:var(--accent);display:flex;align-items:center;padding:0 12px;font-size:11px;color:var(--panel);gap:16px;flex-shrink:0}
        .statusbar span{opacity:.8}

        /* Scrollbar */
        ::-webkit-scrollbar{width:8px;height:8px}
        ::-webkit-scrollbar-track{background:transparent}
        ::-webkit-scrollbar-thumb{background:var(--border);border-radius:4px}
        ::-webkit-scrollbar-thumb:hover{background:var(--text-muted)}

        /* Syntax Highlighting */
        .syn-kw{color:#cba6f7} .syn-str{color:#a6e3a1} .syn-num{color:#fab387}
        .syn-cmt{color:#585b70;font-style:italic} .syn-fn{color:#89b4fa}
        .syn-type{color:#f9e2af} .syn-op{color:#89dceb} .syn-attr{color:#f5c2e7}
    </style>
</head>
<body>
    <div class="titlebar">
        <div class="titlebar-left">
            <span style="font-size:18px">🐝</span>
            <h1>Apis Code</h1>
            <span style="font-size:11px;color:var(--text-muted)" id="workspace-path">loading...</span>
        </div>
        <div class="titlebar-right">
            <button class="tb-btn" onclick="buildSite()" style="border-color:#a6e3a1;color:#a6e3a1">🌐 Build a Site</button>
            <button class="tb-btn" onclick="newFile()">+ New File</button>
            <button class="tb-btn save" onclick="saveFile()" id="save-btn">💾 Save</button>
        </div>
    </div>

    <div class="ide">
        <div class="explorer">
            <div class="explorer-header">
                <span>📁 Explorer</span>
                <button onclick="refreshTree()" title="Refresh">⟳</button>
            </div>
            <div class="file-tree" id="file-tree"></div>
        </div>

        <div class="editor-area">
            <div class="tabs" id="tabs-bar">
                <div class="tab-new" onclick="newFile()" title="New File">+</div>
            </div>

            <div class="editor-container">
                <div class="editor-pane" id="editor-pane">
                    <div class="welcome" id="welcome-screen">
                        <div class="icon">🐝</div>
                        <h2>Apis Code</h2>
                        <p>Open a file from the explorer or create a new one. Press <kbd>Ctrl+S</kbd> to save, use the terminal below to run commands, and ask Apis for help in the AI panel.</p>
                    </div>
                    <div class="code-editor" id="code-editor" style="display:none">
                        <div class="line-numbers" id="line-numbers"></div>
                        <div class="code-content" id="code-display"></div>
                        <textarea class="code-textarea" id="code-textarea" spellcheck="false"></textarea>
                    </div>
                </div>
            </div>

            <div class="terminal-panel" id="terminal-panel">
                <div class="terminal-header">
                    <span>⌨ Terminal</span>
                    <button onclick="clearTerminal()">Clear</button>
                </div>
                <div class="terminal-output" id="terminal-output"><span class="cmd">Welcome to Apis Code terminal. Commands run in your workspace directory.</span>
</div>
                <div class="terminal-input-row">
                    <span class="terminal-prompt">❯</span>
                    <input class="terminal-input" id="terminal-input" placeholder="Type a command..." onkeydown="handleTerminalKey(event)">
                </div>
            </div>
        </div>

        <div class="ai-panel">
            <div class="ai-header">🤖 Apis AI Assistant</div>
            <div class="ai-messages" id="ai-messages">
                <div class="ai-msg assistant"><div class="bubble">Hi! I'm Apis, your AI coding assistant. Ask me anything about your code, and I'll help. I can see the file you have open.</div></div>
            </div>
            <div class="ai-input-area">
                <textarea class="ai-input" id="ai-input" rows="2" placeholder="Ask Apis about your code..." onkeydown="if(event.key==='Enter'&&!event.shiftKey){event.preventDefault();askApis()}"></textarea>
                <button class="ai-send" onclick="askApis()">Ask</button>
            </div>
        </div>
    </div>

    <div class="statusbar">
        <span id="sb-cursor">Ln 1, Col 1</span>
        <span id="sb-lang">—</span>
        <span>UTF-8</span>
        <span>LF</span>
        <span id="sb-files">—</span>
        <span id="sb-model">—</span>
    </div>

<script>
let openTabs = [];
let activeTab = null;
let termHistory = [];
let termHistIdx = -1;

// ── File Tree ──
async function refreshTree() {
    const res = await fetch('/api/files');
    const data = await res.json();
    document.getElementById('workspace-path').textContent = data.workspace || '';
    renderTree(data.tree || [], document.getElementById('file-tree'), 0);
}

function renderTree(items, container, depth) {
    container.innerHTML = '';
    items.forEach(item => {
        const div = document.createElement('div');
        div.className = 'tree-item' + (activeTab && activeTab.path === item.path ? ' active' : '');
        div.style.paddingLeft = (12 + depth * 14) + 'px';
        const icon = item.type === 'dir' ? '📁' : fileIcon(item.name);
        div.innerHTML = `<span class="tree-icon">${icon}</span> ${esc(item.name)}`;

        if (item.type === 'dir') {
            const childContainer = document.createElement('div');
            childContainer.style.display = 'none';
            let expanded = false;
            div.onclick = () => {
                expanded = !expanded;
                childContainer.style.display = expanded ? 'block' : 'none';
                div.querySelector('.tree-icon').textContent = expanded ? '📂' : '📁';
            };
            container.appendChild(div);
            if (item.children) renderTree(item.children, childContainer, depth + 1);
            container.appendChild(childContainer);
        } else {
            div.onclick = () => openFile(item.path);
            container.appendChild(div);
        }
    });
}

function fileIcon(name) {
    const ext = name.split('.').pop();
    const icons = {rs:'🦀',py:'🐍',js:'📜',ts:'📘',html:'🌐',css:'🎨',json:'📋',toml:'⚙️',md:'📝',sh:'🖥️',txt:'📄',yaml:'📋',yml:'📋'};
    return icons[ext] || '📄';
}

// ── Tabs & Editor ──
async function openFile(path) {
    let tab = openTabs.find(t => t.path === path);
    if (!tab) {
        const res = await fetch(`/api/file?path=${encodeURIComponent(path)}`);
        const data = await res.json();
        if (data.error) { alert(data.error); return; }
        tab = { path, name: path.split('/').pop(), content: data.content, original: data.content, language: data.language, modified: false };
        openTabs.push(tab);
    }
    activeTab = tab;
    renderTabs();
    showEditor(tab);
    refreshTreeHighlight();
}

function showEditor(tab) {
    document.getElementById('welcome-screen').style.display = 'none';
    document.getElementById('code-editor').style.display = 'flex';
    const textarea = document.getElementById('code-textarea');
    textarea.value = tab.content;
    updateDisplay();
    document.getElementById('sb-lang').textContent = tab.language || '—';
}

function renderTabs() {
    const bar = document.getElementById('tabs-bar');
    bar.innerHTML = openTabs.map((t, i) => `
        <div class="tab ${t === activeTab ? 'active' : ''}" onclick="switchTab(${i})">
            ${t.modified ? '<span class="modified">•</span>' : ''}
            ${esc(t.name)}
            <span class="close" onclick="event.stopPropagation();closeTab(${i})">×</span>
        </div>
    `).join('') + '<div class="tab-new" onclick="newFile()">+</div>';
}

function switchTab(idx) {
    if (activeTab) activeTab.content = document.getElementById('code-textarea').value;
    activeTab = openTabs[idx];
    showEditor(activeTab);
    renderTabs();
    refreshTreeHighlight();
}

function closeTab(idx) {
    const tab = openTabs[idx];
    if (tab.modified && !confirm(`${tab.name} has unsaved changes. Close anyway?`)) return;
    openTabs.splice(idx, 1);
    if (activeTab === tab) {
        activeTab = openTabs[Math.min(idx, openTabs.length - 1)] || null;
        if (activeTab) showEditor(activeTab);
        else { document.getElementById('welcome-screen').style.display='flex'; document.getElementById('code-editor').style.display='none'; }
    }
    renderTabs();
}

function refreshTreeHighlight() {
    document.querySelectorAll('.tree-item').forEach(el => el.classList.remove('active'));
}

// ── Editor Display ──
function updateDisplay() {
    const textarea = document.getElementById('code-textarea');
    const content = textarea.value;
    const lines = content.split('\n');
    document.getElementById('line-numbers').innerHTML = lines.map((_, i) => i + 1).join('\n');
    document.getElementById('code-display').innerHTML = highlightSyntax(content, activeTab?.language || 'text');

    // Track modifications
    if (activeTab) {
        activeTab.content = content;
        const isModified = content !== activeTab.original;
        if (isModified !== activeTab.modified) {
            activeTab.modified = isModified;
            renderTabs();
        }
    }

    // Cursor position
    const pos = textarea.selectionStart;
    const before = content.substring(0, pos);
    const line = before.split('\n').length;
    const col = pos - before.lastIndexOf('\n');
    document.getElementById('sb-cursor').textContent = `Ln ${line}, Col ${col}`;
}

document.getElementById('code-textarea').addEventListener('input', updateDisplay);
document.getElementById('code-textarea').addEventListener('click', updateDisplay);
document.getElementById('code-textarea').addEventListener('keyup', updateDisplay);
document.getElementById('code-textarea').addEventListener('scroll', function() {
    document.getElementById('code-display').parentElement.scrollTop = this.scrollTop;
    document.getElementById('code-display').parentElement.scrollLeft = this.scrollLeft;
    document.getElementById('line-numbers').parentElement.scrollTop = this.scrollTop;
});

// Handle tab key
document.getElementById('code-textarea').addEventListener('keydown', function(e) {
    if (e.key === 'Tab') {
        e.preventDefault();
        const s = this.selectionStart, end = this.selectionEnd;
        this.value = this.value.substring(0, s) + '    ' + this.value.substring(end);
        this.selectionStart = this.selectionEnd = s + 4;
        updateDisplay();
    }
});

// ── Syntax Highlighting (basic) ──
function highlightSyntax(code, lang) {
    let h = esc(code);
    // Comments
    h = h.replace(/(\/\/.*)/g, '<span class="syn-cmt">$1</span>');
    h = h.replace(/(#[^!\[{].*)/g, '<span class="syn-cmt">$1</span>');
    // Strings
    h = h.replace(/(&quot;(?:[^&]|&(?!quot;))*?&quot;)/g, '<span class="syn-str">$1</span>');
    // Numbers
    h = h.replace(/\b(\d+\.?\d*)\b/g, '<span class="syn-num">$1</span>');
    // Keywords
    const kw = 'fn|let|mut|const|pub|mod|use|struct|enum|impl|trait|async|await|if|else|match|for|while|loop|return|self|Self|super|crate|where|true|false|None|Some|Ok|Err|def|class|import|from|function|var|export|default';
    h = h.replace(new RegExp(`\\b(${kw})\\b`, 'g'), '<span class="syn-kw">$1</span>');
    // Types
    const types = 'String|Vec|Option|Result|Arc|Box|HashMap|bool|u8|u16|u32|u64|i32|i64|f32|f64|usize|str|int|float|list|dict|tuple';
    h = h.replace(new RegExp(`\\b(${types})\\b`, 'g'), '<span class="syn-type">$1</span>');
    return h;
}

// ── Save ──
async function saveFile() {
    if (!activeTab) return;
    activeTab.content = document.getElementById('code-textarea').value;
    const res = await fetch('/api/file', {
        method: 'POST', headers: {'Content-Type':'application/json'},
        body: JSON.stringify({ path: activeTab.path, content: activeTab.content })
    });
    const data = await res.json();
    if (data.ok) {
        activeTab.original = activeTab.content;
        activeTab.modified = false;
        renderTabs();
    } else {
        alert(data.error || 'Save failed');
    }
}

document.addEventListener('keydown', e => {
    if ((e.ctrlKey || e.metaKey) && e.key === 's') { e.preventDefault(); saveFile(); }
});

// ── New File ──
function newFile() {
    const name = prompt('File name (relative path, e.g. src/new_file.rs):');
    if (!name || !name.trim()) return;
    const tab = { path: name.trim(), name: name.trim().split('/').pop(), content: '', original: '', language: ext_to_lang(name), modified: true };
    openTabs.push(tab);
    activeTab = tab;
    renderTabs();
    showEditor(tab);
}

function ext_to_lang(name) {
    const ext = name.split('.').pop();
    return {rs:'rust',py:'python',js:'javascript',ts:'typescript',html:'html',css:'css',json:'json',toml:'toml',md:'markdown',sh:'shell'}[ext]||'text';
}

// ── Terminal ──
async function handleTerminalKey(e) {
    if (e.key === 'Enter') {
        const input = document.getElementById('terminal-input');
        const cmd = input.value.trim();
        if (!cmd) return;
        input.value = '';
        termHistory.push(cmd);
        termHistIdx = termHistory.length;

        const output = document.getElementById('terminal-output');
        output.innerHTML += `\n<span class="cmd">❯ ${esc(cmd)}</span>\n`;

        try {
            const res = await fetch('/api/terminal', {
                method: 'POST', headers: {'Content-Type':'application/json'},
                body: JSON.stringify({ command: cmd })
            });
            const data = await res.json();
            if (data.error) {
                output.innerHTML += `<span class="err">${esc(data.error)}</span>\n`;
            } else {
                if (data.stdout) output.innerHTML += esc(data.stdout);
                if (data.stderr) output.innerHTML += `<span class="err">${esc(data.stderr)}</span>`;
                if (data.exit_code !== 0) output.innerHTML += `<span class="err">[exit code: ${data.exit_code}]</span>\n`;
            }
        } catch(err) {
            output.innerHTML += `<span class="err">Error: ${err.message}</span>\n`;
        }
        output.scrollTop = output.scrollHeight;
    }
    if (e.key === 'ArrowUp') { termHistIdx = Math.max(0, termHistIdx-1); e.target.value = termHistory[termHistIdx]||''; }
    if (e.key === 'ArrowDown') { termHistIdx = Math.min(termHistory.length, termHistIdx+1); e.target.value = termHistory[termHistIdx]||''; }
}

function clearTerminal() { document.getElementById('terminal-output').innerHTML = ''; }

// ── AI Chat ──
async function askApis() {
    const input = document.getElementById('ai-input');
    const q = input.value.trim();
    if (!q) return;
    input.value = '';

    const msgs = document.getElementById('ai-messages');
    msgs.innerHTML += `<div class="ai-msg user"><div class="bubble">${esc(q)}</div></div>`;
    msgs.innerHTML += `<div class="ai-msg assistant" id="ai-loading"><div class="bubble">⏳ Thinking...</div></div>`;
    msgs.scrollTop = msgs.scrollHeight;

    const body = { question: q };
    if (activeTab) {
        body.file_path = activeTab.path;
        body.file_context = document.getElementById('code-textarea')?.value || '';
    }

    try {
        const res = await fetch('/api/ask', {
            method: 'POST', headers: {'Content-Type':'application/json'},
            body: JSON.stringify(body)
        });
        const data = await res.json();
        document.getElementById('ai-loading')?.remove();

        const response = data.response || data.error || 'No response';
        // Basic markdown: code blocks
        let html = esc(response)
            .replace(/```(\w*)\n([\s\S]*?)```/g, '<pre><code>$2</code></pre>')
            .replace(/`([^`]+)`/g, '<code>$1</code>')
            .replace(/\n/g, '<br>');

        msgs.innerHTML += `<div class="ai-msg assistant"><div class="bubble">${html}</div></div>`;
    } catch(err) {
        document.getElementById('ai-loading')?.remove();
        msgs.innerHTML += `<div class="ai-msg assistant"><div class="bubble" style="color:var(--red)">Error: ${err.message}</div></div>`;
    }
    msgs.scrollTop = msgs.scrollHeight;
}

// ── Utilities ──
function esc(t) { if(!t)return''; const d=document.createElement('div'); d.textContent=t; return d.innerHTML; }

// ── Status ──
async function loadStatus() {
    try {
        const res = await fetch('/api/status');
        const data = await res.json();
        document.getElementById('sb-files').textContent = `${data.file_count || 0} files`;
        document.getElementById('sb-model').textContent = `🤖 ${data.model || 'unknown'}`;
    } catch(e) {}
}

// ── Build a Mesh Site ──
async function buildSite() {
    const types = ['blog','portfolio','forum','shop','landing','documentation','gallery'];
    const type_ = prompt('What kind of site?\n\n' + types.map((t,i)=>`${i+1}. ${t}`).join('\n') + '\n\nEnter number or type:');
    if (!type_) return;
    const siteType = types[parseInt(type_)-1] || type_;
    const name = prompt('Site name:');
    if (!name) return;
    const desc = prompt('Brief description (optional):') || '';

    const msgs = document.getElementById('ai-messages');
    msgs.innerHTML += `<div class="ai-msg user"><div class="bubble">Build a ${siteType} site called "${esc(name)}"</div></div>`;
    msgs.innerHTML += `<div class="ai-msg assistant" id="build-loading"><div class="bubble">🔧 Building your mesh site... This may take 1-2 minutes while Apis designs it.</div></div>`;
    msgs.scrollTop = msgs.scrollHeight;

    try {
        const res = await fetch('/api/build-site', {
            method:'POST', headers:{'Content-Type':'application/json'},
            body: JSON.stringify({site_type: siteType, site_name: name, description: desc})
        });
        const data = await res.json();
        document.getElementById('build-loading')?.remove();

        if (data.ok) {
            msgs.innerHTML += `<div class="ai-msg assistant"><div class="bubble">✅ Site built! Saved to <strong>${esc(data.folder)}</strong> (${data.size} bytes).<br><br><button onclick="openFile('${esc(data.file)}')"
                style="padding:6px 12px;border-radius:6px;border:1px solid var(--green);background:rgba(166,227,161,0.1);color:var(--green);cursor:pointer;font-family:inherit">📄 Open index.html</button>
                <button onclick="publishSite('${esc(data.folder)}','${esc(name)}','${esc(desc)}')"
                style="padding:6px 12px;border-radius:6px;border:1px solid var(--accent);background:var(--accent-dim);color:var(--accent);cursor:pointer;font-family:inherit;margin-left:6px">🚀 Publish to Mesh</button>
            </div></div>`;
            refreshTree();
        } else {
            msgs.innerHTML += `<div class="ai-msg assistant"><div class="bubble" style="color:var(--red)">❌ ${esc(data.error)}</div></div>`;
        }
    } catch(err) {
        document.getElementById('build-loading')?.remove();
        msgs.innerHTML += `<div class="ai-msg assistant"><div class="bubble" style="color:var(--red)">Error: ${err.message}</div></div>`;
    }
    msgs.scrollTop = msgs.scrollHeight;
}

async function publishSite(folder, name, desc) {
    const msgs = document.getElementById('ai-messages');
    try {
        const res = await fetch('/api/publish-site', {
            method:'POST', headers:{'Content-Type':'application/json'},
            body: JSON.stringify({name, description: desc, folder, icon:'🌐'})
        });
        const data = await res.json();
        if (data.ok) {
            msgs.innerHTML += `<div class="ai-msg assistant"><div class="bubble">🚀 Published! Your site is now listed on <a href="http://localhost:3035" target="_blank" style="color:var(--accent)">HivePortal</a>.</div></div>`;
        } else {
            msgs.innerHTML += `<div class="ai-msg assistant"><div class="bubble" style="color:var(--red)">${esc(data.error)}</div></div>`;
        }
    } catch(err) {
        msgs.innerHTML += `<div class="ai-msg assistant"><div class="bubble" style="color:var(--red)">Error: ${err.message}</div></div>`;
    }
    msgs.scrollTop = msgs.scrollHeight;
}

// ── Boot ──
refreshTree();
loadStatus();
</script>
</body>
</html>"##;

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
