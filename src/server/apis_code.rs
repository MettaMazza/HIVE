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

#[derive(Deserialize)]
struct RenameReq {
    from: String,
    to: String,
}

#[derive(Deserialize)]
struct FindReplaceReq {
    path: String,
    find: String,
    replace: String,
    #[serde(default)]
    regex: bool,
    #[serde(default)]
    case_sensitive: Option<bool>,
}

#[derive(Deserialize)]
struct GrepFileReq {
    path: String,
    query: String,
    #[serde(default)]
    context_lines: Option<usize>,
}

pub async fn spawn_apis_code_server() {
    let port: u16 = 3033; // Mesh-governed: creator-key protected

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
            .route("/api/rename", post(api_rename))
            .route("/api/find-replace", post(api_find_replace))
            .route("/api/grep", get(api_grep_file))
            .route("/api/terminal", post(api_terminal))
            .route("/api/ask", post(api_ask))
            .route("/api/build-site", post(api_build_site))
            .route("/api/build-template", post(api_build_template))
            .route("/api/templates", get(api_templates))
            .route("/api/sites", get(api_list_sites))
            .route("/api/preview", get(api_preview_site))
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

#[derive(Deserialize)]
struct TemplateReq {
    template: String,
    site_name: String,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Deserialize)]
struct PreviewQuery {
    folder: String,
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
    let portal_port: u16 = 3035; // Mesh-governed

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

// ─── Template Library & Site Management ─────────────────────────────────

/// Get available site templates.
async fn api_templates() -> Json<Value> {
    Json(json!({
        "templates": [
            {"id": "blog", "name": "Blog", "desc": "Minimal dark blog with article layout", "icon": "📝"},
            {"id": "portfolio", "name": "Portfolio", "desc": "Developer portfolio with project showcase", "icon": "💼"},
            {"id": "landing", "name": "Landing Page", "desc": "Product landing page with hero & CTA", "icon": "🚀"},
            {"id": "docs", "name": "Documentation", "desc": "Technical documentation site with sidebar nav", "icon": "📖"},
            {"id": "forum", "name": "Forum", "desc": "Community forum with threads and replies", "icon": "💬"},
            {"id": "shop", "name": "Shop", "desc": "Simple storefront with product grid", "icon": "🛒"},
            {"id": "dashboard", "name": "Dashboard", "desc": "Analytics dashboard with charts and metrics", "icon": "📊"},
            {"id": "wiki", "name": "Wiki", "desc": "Knowledge base with search and categories", "icon": "📚"},
        ]
    }))
}

/// Build a site from a template (instant, no AI needed).
async fn api_build_template(State(state): State<CodeState>, Json(req): Json<TemplateReq>) -> Json<Value> {
    let color = req.color.as_deref().unwrap_or("#ffc107");
    let name = &req.site_name;

    let html = match req.template.as_str() {
        "blog" => generate_template_blog(name, color),
        "portfolio" => generate_template_portfolio(name, color),
        "landing" => generate_template_landing(name, color),
        "docs" => generate_template_docs(name, color),
        _ => generate_template_landing(name, color), // fallback
    };

    let folder = format!("mesh_sites/{}", name.to_lowercase().replace(' ', "_"));
    let site_path = std::path::PathBuf::from(state.workspace.as_str()).join(&folder);
    let _ = std::fs::create_dir_all(&site_path);
    let index_path = site_path.join("index.html");
    let _ = std::fs::write(&index_path, &html);

    tracing::info!("[APIS CODE] 🎨 Built template site: {} ({})", folder, req.template);

    Json(json!({
        "ok": true, "folder": folder,
        "file": format!("{}/index.html", folder),
        "size": html.len(), "template": req.template,
    }))
}

/// List all built mesh sites.
async fn api_list_sites(State(state): State<CodeState>) -> Json<Value> {
    let sites_dir = std::path::PathBuf::from(state.workspace.as_str()).join("mesh_sites");
    let mut sites = vec![];

    if let Ok(entries) = std::fs::read_dir(&sites_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let name = entry.file_name().to_string_lossy().to_string();
                let index = entry.path().join("index.html");
                let has_index = index.exists();
                let size = if has_index {
                    std::fs::metadata(&index).map(|m| m.len()).unwrap_or(0)
                } else { 0 };

                sites.push(json!({
                    "name": name,
                    "folder": format!("mesh_sites/{}", name),
                    "has_index": has_index,
                    "size": size,
                }));
            }
        }
    }

    Json(json!({"sites": sites, "count": sites.len()}))
}

/// Preview a built site (serve its index.html).
async fn api_preview_site(State(state): State<CodeState>, Query(params): Query<PreviewQuery>) -> Html<String> {
    let full = std::path::PathBuf::from(state.workspace.as_str())
        .join(&params.folder)
        .join("index.html");

    match std::fs::read_to_string(&full) {
        Ok(html) => Html(html),
        Err(_) => Html("<h1>Site not found</h1><p>No index.html in this folder.</p>".to_string()),
    }
}

// ─── Template Generators ────────────────────────────────────────────────

fn generate_template_blog(name: &str, color: &str) -> String {
    format!(r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>{name}</title><style>
*{{margin:0;padding:0;box-sizing:border-box}}body{{font-family:system-ui,sans-serif;background:#0a0a0f;color:#e0e0e8;line-height:1.7}}
header{{padding:40px 20px;text-align:center;border-bottom:1px solid rgba(255,255,255,.06)}}
h1{{font-size:32px;background:linear-gradient(135deg,{color},#ff9800);-webkit-background-clip:text;-webkit-text-fill-color:transparent}}
.subtitle{{color:#888;margin-top:8px}}.posts{{max-width:720px;margin:40px auto;padding:0 20px}}
.post{{background:rgba(255,255,255,.03);border:1px solid rgba(255,255,255,.06);border-radius:16px;padding:28px;margin-bottom:24px;transition:all .3s}}
.post:hover{{border-color:rgba(255,193,7,.3);transform:translateY(-2px)}}
.post h2{{font-size:20px;margin-bottom:8px}}.post .meta{{font-size:12px;color:#666;margin-bottom:16px}}.post p{{color:#aaa}}
footer{{text-align:center;padding:40px;color:#555;font-size:12px;border-top:1px solid rgba(255,255,255,.06)}}
</style></head><body>
<header><h1>{name}</h1><p class="subtitle">A decentralised blog on the mesh</p></header>
<div class="posts">
<article class="post"><h2>Welcome to {name}</h2><div class="meta">Today · 3 min read</div><p>This is your first post on the mesh. Edit this file to add your own content. Everything here runs peer-to-peer — no servers, no corporations, no censorship.</p></article>
<article class="post"><h2>Building on the Mesh</h2><div class="meta">Yesterday · 5 min read</div><p>The HIVE mesh network enables truly decentralised publishing. Your content belongs to you and is distributed across peers who choose to host it.</p></article>
</div>
<footer>Powered by HIVE Mesh · No servers, no surveillance</footer>
</body></html>"##, name=name, color=color)
}

fn generate_template_portfolio(name: &str, color: &str) -> String {
    format!(r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>{name}</title><style>
*{{margin:0;padding:0;box-sizing:border-box}}body{{font-family:system-ui,sans-serif;background:#0a0a0f;color:#e0e0e8}}
.hero{{text-align:center;padding:80px 20px}}h1{{font-size:42px;background:linear-gradient(135deg,{color},#ff9800);-webkit-background-clip:text;-webkit-text-fill-color:transparent}}
.tagline{{color:#888;margin:16px 0;font-size:18px}}.projects{{display:grid;grid-template-columns:repeat(auto-fill,minmax(300px,1fr));gap:20px;padding:20px 40px;max-width:1200px;margin:0 auto}}
.card{{background:rgba(255,255,255,.03);border:1px solid rgba(255,255,255,.06);border-radius:16px;padding:24px;transition:all .3s}}
.card:hover{{border-color:{color}40;transform:translateY(-4px);box-shadow:0 12px 40px rgba(0,0,0,.4)}}
.card h3{{margin-bottom:8px}}.card p{{color:#888;font-size:14px}}.tag{{display:inline-block;padding:3px 10px;border-radius:20px;font-size:11px;background:{color}15;color:{color};margin-top:12px;margin-right:4px}}
.contact{{text-align:center;padding:60px 20px}}.contact a{{color:{color};text-decoration:none}}
</style></head><body>
<div class="hero"><h1>{name}</h1><p class="tagline">Developer · Designer · Mesh Architect</p></div>
<div class="projects">
<div class="card"><h3>🌐 Project Alpha</h3><p>A decentralised social network built on mesh protocols.</p><span class="tag">Rust</span><span class="tag">P2P</span></div>
<div class="card"><h3>🔐 SecureVault</h3><p>End-to-end encrypted file storage for the mesh.</p><span class="tag">Crypto</span><span class="tag">Storage</span></div>
<div class="card"><h3>🤖 MeshBot</h3><p>AI assistant running entirely on local hardware.</p><span class="tag">AI</span><span class="tag">Local</span></div>
</div>
<div class="contact"><p>Built on the HIVE mesh · <a href="#">Contact</a></p></div>
</body></html>"##, name=name, color=color)
}

fn generate_template_landing(name: &str, color: &str) -> String {
    format!(r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>{name}</title><style>
*{{margin:0;padding:0;box-sizing:border-box}}body{{font-family:system-ui,sans-serif;background:#0a0a0f;color:#e0e0e8}}
.hero{{text-align:center;padding:100px 20px;background:linear-gradient(180deg,rgba(255,193,7,.05),transparent)}}
h1{{font-size:48px;background:linear-gradient(135deg,{color},#ff9800);-webkit-background-clip:text;-webkit-text-fill-color:transparent;margin-bottom:16px}}
.subtitle{{color:#888;font-size:20px;max-width:600px;margin:0 auto 32px}}.cta{{display:inline-block;padding:14px 32px;background:linear-gradient(135deg,{color},#ff9800);color:#000;border-radius:12px;text-decoration:none;font-weight:700;font-size:16px;transition:all .2s}}
.cta:hover{{transform:scale(1.05);box-shadow:0 8px 30px {color}40}}
.features{{display:grid;grid-template-columns:repeat(auto-fit,minmax(250px,1fr));gap:24px;padding:60px 40px;max-width:1000px;margin:0 auto}}
.feature{{text-align:center;padding:32px 20px}}.feature .icon{{font-size:40px;margin-bottom:16px}}.feature h3{{margin-bottom:8px}}.feature p{{color:#888;font-size:14px}}
footer{{text-align:center;padding:40px;color:#555;font-size:12px}}
</style></head><body>
<div class="hero"><h1>{name}</h1><p class="subtitle">The future is decentralised. Build, share, and connect on the mesh — no servers required.</p><a href="#" class="cta">Get Started →</a></div>
<div class="features">
<div class="feature"><div class="icon">🔒</div><h3>Private by Default</h3><p>End-to-end encrypted communication with zero data harvesting.</p></div>
<div class="feature"><div class="icon">🌐</div><h3>Mesh-Powered</h3><p>Works without internet through peer-to-peer mesh relay.</p></div>
<div class="feature"><div class="icon">⚡</div><h3>Lightning Fast</h3><p>Native performance with Rust-powered backend.</p></div>
</div>
<footer>Built on HIVE Mesh · Sovereign Technology</footer>
</body></html>"##, name=name, color=color)
}

fn generate_template_docs(name: &str, color: &str) -> String {
    format!(r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>{name} — Docs</title><style>
*{{margin:0;padding:0;box-sizing:border-box}}body{{font-family:system-ui,sans-serif;background:#0a0a0f;color:#e0e0e8;display:flex;min-height:100vh}}
.sidebar{{width:260px;background:rgba(255,255,255,.02);border-right:1px solid rgba(255,255,255,.06);padding:24px;flex-shrink:0}}
.sidebar h2{{font-size:16px;background:linear-gradient(135deg,{color},#ff9800);-webkit-background-clip:text;-webkit-text-fill-color:transparent;margin-bottom:20px}}
.sidebar a{{display:block;color:#888;text-decoration:none;padding:6px 12px;border-radius:8px;margin-bottom:4px;font-size:14px;transition:all .2s}}
.sidebar a:hover,.sidebar a.active{{background:rgba(255,255,255,.06);color:#e0e0e8}}
.content{{flex:1;padding:40px;max-width:800px}}h1{{font-size:28px;margin-bottom:16px}}h2{{font-size:20px;margin:24px 0 12px;color:{color}}}
p{{color:#aaa;margin-bottom:12px;line-height:1.8}}code{{background:rgba(255,255,255,.06);padding:2px 8px;border-radius:4px;font-size:13px}}
pre{{background:rgba(255,255,255,.04);border:1px solid rgba(255,255,255,.06);border-radius:12px;padding:16px;margin:12px 0;overflow-x:auto;font-size:13px}}
</style></head><body>
<div class="sidebar"><h2>{name}</h2><a class="active" href="#">Getting Started</a><a href="#">Installation</a><a href="#">Configuration</a><a href="#">API Reference</a><a href="#">Deployment</a></div>
<div class="content"><h1>Getting Started</h1><p>Welcome to the {name} documentation. This guide will help you get up and running.</p>
<h2>Prerequisites</h2><p>You'll need <code>Rust 1.75+</code> and a running HIVE mesh node.</p>
<h2>Quick Start</h2><pre>cargo install hive-cli
hive init my-project
hive run</pre>
<p>That's it! Your mesh node is now running and discoverable by peers.</p>
</div></body></html>"##, name=name, color=color)
}

// ─── SPA Frontend ───────────────────────────────────────────────────────

async fn serve_ide() -> Html<String> {
    Html(IDE_HTML.to_string())
}

use super::apis_code_html::IDE_HTML;

// ─── New P0 Endpoints ───────────────────────────────────────────────────

/// Rename or move a file/directory.
async fn api_rename(State(state): State<CodeState>, Json(req): Json<RenameReq>) -> Json<Value> {
    let from = match safe_path(&state.workspace, &req.from) {
        Some(p) => p,
        None => return Json(json!({"error": "Invalid source path"})),
    };
    let to = match safe_path(&state.workspace, &req.to) {
        Some(p) => p,
        None => return Json(json!({"error": "Invalid destination path"})),
    };

    if !from.exists() {
        return Json(json!({"error": "Source does not exist"}));
    }
    if to.exists() {
        return Json(json!({"error": "Destination already exists"}));
    }

    // Create parent dirs for destination
    if let Some(parent) = to.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match std::fs::rename(&from, &to) {
        Ok(_) => {
            tracing::info!("[APIS CODE] 📁 Renamed: {} → {}", req.from, req.to);
            Json(json!({"ok": true, "from": req.from, "to": req.to}))
        }
        Err(e) => Json(json!({"error": format!("Rename failed: {}", e)})),
    }
}

/// Find and replace within a file.
async fn api_find_replace(State(state): State<CodeState>, Json(req): Json<FindReplaceReq>) -> Json<Value> {
    let full = match safe_path(&state.workspace, &req.path) {
        Some(p) => p,
        None => return Json(json!({"error": "Invalid path"})),
    };

    if !full.is_file() {
        return Json(json!({"error": "Not a file"}));
    }

    let content = match std::fs::read_to_string(&full) {
        Ok(c) => c,
        Err(e) => return Json(json!({"error": format!("Read failed: {}", e)})),
    };

    let case_sensitive = req.case_sensitive.unwrap_or(true);

    let (new_content, count) = if req.regex {
        // Regex find-replace
        match regex::Regex::new(&req.find) {
            Ok(re) => {
                let mut count = 0;
                let result = re.replace_all(&content, |_caps: &regex::Captures| {
                    count += 1;
                    req.replace.clone()
                }).to_string();
                (result, count)
            }
            Err(e) => return Json(json!({"error": format!("Invalid regex: {}", e)})),
        }
    } else if case_sensitive {
        let count = content.matches(&req.find).count();
        let result = content.replace(&req.find, &req.replace);
        (result, count)
    } else {
        // Case-insensitive string replace
        let find_lower = req.find.to_lowercase();
        let mut result = String::new();
        let mut count = 0;
        let mut remaining = content.as_str();
        while let Some(pos) = remaining.to_lowercase().find(&find_lower) {
            result.push_str(&remaining[..pos]);
            result.push_str(&req.replace);
            remaining = &remaining[pos + req.find.len()..];
            count += 1;
        }
        result.push_str(remaining);
        (result, count)
    };

    if count == 0 {
        return Json(json!({"ok": true, "replacements": 0, "message": "No matches found"}));
    }

    match std::fs::write(&full, &new_content) {
        Ok(_) => {
            tracing::info!("[APIS CODE] 🔄 Find-replace in {}: {} replacements", req.path, count);
            Json(json!({"ok": true, "replacements": count, "path": req.path, "new_size": new_content.len()}))
        }
        Err(e) => Json(json!({"error": format!("Write failed: {}", e)})),
    }
}

/// Grep within a specific file with context lines.
async fn api_grep_file(State(state): State<CodeState>, Query(params): Query<GrepFileReq>) -> Json<Value> {
    let full = match safe_path(&state.workspace, &params.path) {
        Some(p) => p,
        None => return Json(json!({"error": "Invalid path"})),
    };

    if !full.is_file() {
        return Json(json!({"error": "Not a file"}));
    }

    let content = match std::fs::read_to_string(&full) {
        Ok(c) => c,
        Err(e) => return Json(json!({"error": format!("Read failed: {}", e)})),
    };

    let context = params.context_lines.unwrap_or(2);
    let query_lower = params.query.to_lowercase();
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    let mut matches = vec![];
    for (i, line) in lines.iter().enumerate() {
        if line.to_lowercase().contains(&query_lower) {
            let start = i.saturating_sub(context);
            let end = (i + context + 1).min(total_lines);
            let context_lines: Vec<Value> = (start..end).map(|j| {
                json!({
                    "line": j + 1,
                    "content": lines[j],
                    "is_match": j == i,
                })
            }).collect();

            matches.push(json!({
                "line": i + 1,
                "content": line.trim(),
                "context": context_lines,
            }));
        }
    }

    let total_matches = matches.len().min(100);
    Json(json!({
        "matches": matches.into_iter().take(100).collect::<Vec<_>>(),
        "total_matches": total_matches,
        "path": params.path,
        "query": params.query,
    }))
}

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

    #[test]
    fn test_rename_requires_valid_paths() {
        // Traversal should be blocked
        assert!(safe_path("/tmp/ws", "../escape").is_none());
        assert!(safe_path("/tmp/ws", "/absolute/path").is_none());
    }

    #[test]
    fn test_find_replace_string() {
        let content = "Hello world, hello World";
        // Case-sensitive
        let result = content.replace("Hello", "Hi");
        assert_eq!(result, "Hi world, hello World");
        assert_eq!(content.matches("Hello").count(), 1);
    }

    #[test]
    fn test_find_replace_case_insensitive() {
        let content = "Hello World, hello world";
        let find = "hello";
        let replace = "Hi";
        let find_lower = find.to_lowercase();
        let mut result = String::new();
        let mut count = 0;
        let mut remaining = content;
        while let Some(pos) = remaining.to_lowercase().find(&find_lower) {
            result.push_str(&remaining[..pos]);
            result.push_str(replace);
            remaining = &remaining[pos + find.len()..];
            count += 1;
        }
        result.push_str(remaining);
        assert_eq!(count, 2);
        assert_eq!(result, "Hi World, Hi world");
    }

    #[test]
    fn test_find_replace_regex() {
        let re = regex::Regex::new(r"\bfn\s+(\w+)").unwrap();
        let content = "fn main() {}\nfn helper() {}";
        let mut count = 0;
        let result = re.replace_all(content, |_caps: &regex::Captures| {
            count += 1;
            "pub fn replaced".to_string()
        }).to_string();
        assert_eq!(count, 2);
        assert!(result.contains("pub fn replaced"));
    }

    #[test]
    fn test_grep_context() {
        let lines = vec!["line 1", "line 2", "MATCH here", "line 4", "line 5"];
        let query = "match";
        let context = 1;
        let mut matches = vec![];
        for (i, line) in lines.iter().enumerate() {
            if line.to_lowercase().contains(query) {
                let start = i.saturating_sub(context);
                let end = (i + context + 1).min(lines.len());
                matches.push((i + 1, start..end));
            }
        }
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, 3); // line 3
        assert_eq!(matches[0].1, 1..4); // context lines 2-4
    }

    #[test]
    fn test_template_blog_generates_valid_html() {
        let html = generate_template_blog("TestBlog", "#ffc107");
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("TestBlog"));
        assert!(html.contains("#ffc107"));
        assert!(html.len() > 500);
    }

    #[test]
    fn test_template_portfolio_generates_valid_html() {
        let html = generate_template_portfolio("MyPortfolio", "#42a5f5");
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("MyPortfolio"));
        assert!(html.contains("Developer"));
    }

    #[test]
    fn test_template_landing_generates_valid_html() {
        let html = generate_template_landing("LaunchPad", "#4caf50");
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("LaunchPad"));
        assert!(html.contains("Get Started"));
    }

    #[test]
    fn test_template_docs_generates_valid_html() {
        let html = generate_template_docs("HiveDocs", "#ff9800");
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("HiveDocs"));
        assert!(html.contains("Getting Started"));
        assert!(html.contains("sidebar"));
    }

    #[test]
    fn test_all_templates_self_contained() {
        // Templates must NOT reference external CDNs
        let templates = vec![
            generate_template_blog("T", "#fff"),
            generate_template_portfolio("T", "#fff"),
            generate_template_landing("T", "#fff"),
            generate_template_docs("T", "#fff"),
        ];
        for html in &templates {
            assert!(!html.contains("cdn."), "Template references external CDN");
            assert!(!html.contains("googleapis"), "Template references Google APIs");
            assert!(html.contains("<style>"), "Template missing inline CSS");
        }
    }
}
