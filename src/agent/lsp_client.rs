use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

/// Manages a single LSP language server process.
/// Communicates via JSON-RPC 2.0 over stdio with Content-Length framing.
pub struct LspClient {
    process: Child,
    stdin: tokio::io::BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    request_id: AtomicU64,
    pub language: String,
    pub root_uri: String,
    pub last_activity: std::time::Instant,
    initialized: bool,
}

/// Global registry of active language server clients.
/// Keyed by language name (e.g. "rust", "python", "typescript").
static LSP_REGISTRY: std::sync::LazyLock<Arc<Mutex<HashMap<String, LspClient>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

/// Detect which language a file extension maps to.
pub fn detect_language(file_path: &str) -> Option<String> {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "rs" => Some("rust".into()),
        "py" | "pyi" => Some("python".into()),
        "ts" | "tsx" | "js" | "jsx" => Some("typescript".into()),
        "go" => Some("go".into()),
        _ => None,
    }
}

/// Find the binary for a given language server.
fn find_server_binary(language: &str) -> Option<String> {
    let candidates: Vec<&str> = match language {
        "rust" => vec!["rust-analyzer"],
        "python" => vec!["pyright-langserver", "pylsp"],
        "typescript" => vec!["typescript-language-server"],
        "go" => vec!["gopls"],
        _ => return None,
    };

    for candidate in candidates {
        if which_exists(candidate) {
            return Some(candidate.to_string());
        }
    }

    // For rust-analyzer, also check cargo bin dir
    if language == "rust" {
        let home = std::env::var("HOME").unwrap_or_default();
        let cargo_path = format!("{}/.cargo/bin/rust-analyzer", home);
        if std::path::Path::new(&cargo_path).exists() {
            return Some(cargo_path);
        }
    }

    None
}

/// Check if a binary exists on PATH.
fn which_exists(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

impl LspClient {
    /// Spawn a new language server process and perform the LSP initialize handshake.
    pub async fn spawn(language: &str, root_dir: &str) -> Result<Self, String> {
        let binary = find_server_binary(language)
            .ok_or_else(|| format!("No language server found for '{}'. Install rust-analyzer/pyright/typescript-language-server.", language))?;

        tracing::info!("[LSP] Spawning '{}' for language '{}' in root '{}'", binary, language, root_dir);

        // Build command with language-specific args
        let mut cmd = tokio::process::Command::new(&binary);
        cmd.stdin(std::process::Stdio::piped())
           .stdout(std::process::Stdio::piped())
           .stderr(std::process::Stdio::null())
           .kill_on_drop(true);

        // typescript-language-server needs --stdio flag
        if language == "typescript" {
            cmd.arg("--stdio");
        }
        // pyright-langserver needs --stdio flag
        if binary.contains("pyright") {
            cmd.arg("--stdio");
        }

        let mut process = cmd.spawn()
            .map_err(|e| format!("Failed to spawn '{}': {}", binary, e))?;

        let stdin = process.stdin.take()
            .ok_or_else(|| "Failed to capture stdin".to_string())?;
        let stdout = process.stdout.take()
            .ok_or_else(|| "Failed to capture stdout".to_string())?;

        let root_path = std::path::Path::new(root_dir);
        let root_uri = format!("file://{}", root_path.canonicalize()
            .unwrap_or_else(|_| root_path.to_path_buf())
            .display());

        let mut client = Self {
            process,
            stdin: tokio::io::BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            request_id: AtomicU64::new(1),
            language: language.to_string(),
            root_uri: root_uri.clone(),
            last_activity: std::time::Instant::now(),
            initialized: false,
        };

        // Perform initialize handshake
        let init_params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "definition": { "dynamicRegistration": false },
                    "references": { "dynamicRegistration": false },
                    "documentSymbol": { "dynamicRegistration": false },
                    "hover": { "dynamicRegistration": false },
                    "publishDiagnostics": { "relatedInformation": true }
                }
            }
        });

        let init_result = client.send_request("initialize", init_params).await;
        match init_result {
            Ok(_) => {
                // Send initialized notification
                client.send_notification("initialized", serde_json::json!({})).await?;
                client.initialized = true;
                tracing::info!("[LSP] Server '{}' for '{}' initialized successfully", binary, language);
                Ok(client)
            }
            Err(e) => {
                let _ = client.process.kill().await;
                Err(format!("LSP initialize handshake failed: {}", e))
            }
        }
    }

    /// Send a JSON-RPC request and wait for the response.
    async fn send_request(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        self.last_activity = std::time::Instant::now();

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        let body = serde_json::to_string(&msg)
            .map_err(|e| format!("JSON serialize error: {}", e))?;

        // Write with Content-Length header
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stdin.write_all(header.as_bytes()).await
            .map_err(|e| format!("Write header failed: {}", e))?;
        self.stdin.write_all(body.as_bytes()).await
            .map_err(|e| format!("Write body failed: {}", e))?;
        self.stdin.flush().await
            .map_err(|e| format!("Flush failed: {}", e))?;

        // Read response with timeout
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.read_response(id)
        ).await
        .map_err(|_| format!("LSP request '{}' timed out after 30s", method))?;

        response
    }

    /// Send a JSON-RPC notification (no response expected).
    async fn send_notification(&mut self, method: &str, params: serde_json::Value) -> Result<(), String> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        let body = serde_json::to_string(&msg)
            .map_err(|e| format!("JSON serialize error: {}", e))?;

        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stdin.write_all(header.as_bytes()).await
            .map_err(|e| format!("Write notification failed: {}", e))?;
        self.stdin.write_all(body.as_bytes()).await
            .map_err(|e| format!("Write notification body failed: {}", e))?;
        self.stdin.flush().await
            .map_err(|e| format!("Flush failed: {}", e))?;
        Ok(())
    }

    /// Read a JSON-RPC response, skipping notifications until we find our request ID.
    async fn read_response(&mut self, expected_id: u64) -> Result<serde_json::Value, String> {
        loop {
            // Read Content-Length header
            let mut header_line = String::new();
            loop {
                header_line.clear();
                let bytes_read = self.stdout.read_line(&mut header_line).await
                    .map_err(|e| format!("Read header line failed: {}", e))?;
                if bytes_read == 0 {
                    return Err("LSP server closed connection".into());
                }
                let trimmed = header_line.trim();
                if trimmed.is_empty() {
                    // End of headers — but we need Content-Length first
                    continue;
                }
                if trimmed.starts_with("Content-Length:") {
                    break;
                }
            }

            let content_length: usize = header_line.trim()
                .strip_prefix("Content-Length:")
                .ok_or("Missing Content-Length")?
                .trim()
                .parse()
                .map_err(|e| format!("Invalid Content-Length: {}", e))?;

            // Read past the blank line separator
            let mut blank = String::new();
            self.stdout.read_line(&mut blank).await
                .map_err(|e| format!("Read blank line failed: {}", e))?;

            // Read the JSON body
            let mut body = vec![0u8; content_length];
            self.stdout.read_exact(&mut body).await
                .map_err(|e| format!("Read body failed: {}", e))?;

            let parsed: serde_json::Value = serde_json::from_slice(&body)
                .map_err(|e| format!("JSON parse error: {}", e))?;

            // Check if this is a response to our request
            if let Some(resp_id) = parsed.get("id").and_then(|v| v.as_u64()) {
                if resp_id == expected_id {
                    if let Some(error) = parsed.get("error") {
                        let msg = error.get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown error");
                        return Err(format!("LSP error: {}", msg));
                    }
                    return Ok(parsed.get("result").cloned().unwrap_or(serde_json::Value::Null));
                }
            }
            // Otherwise it's a notification (diagnostics, etc.) — skip and keep reading
        }
    }

    /// Construct a TextDocumentPositionParams-style JSON object.
    fn position_params(file_path: &str, line: u32, col: u32) -> serde_json::Value {
        let uri = if file_path.starts_with("file://") {
            file_path.to_string()
        } else {
            let abs = std::path::Path::new(file_path)
                .canonicalize()
                .unwrap_or_else(|_| std::path::PathBuf::from(file_path));
            format!("file://{}", abs.display())
        };
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line.saturating_sub(1), "character": col.saturating_sub(1) }
        })
    }

    /// textDocument/definition — go to definition
    pub async fn goto_definition(&mut self, file: &str, line: u32, col: u32) -> Result<String, String> {
        let params = Self::position_params(file, line, col);
        let result = self.send_request("textDocument/definition", params).await?;
        Self::format_locations(&result, "Definition")
    }

    /// textDocument/references — find all references
    pub async fn find_references(&mut self, file: &str, line: u32, col: u32) -> Result<String, String> {
        let mut params = Self::position_params(file, line, col);
        // references needs a "context" field
        params.as_object_mut().unwrap().insert(
            "context".into(),
            serde_json::json!({ "includeDeclaration": true })
        );
        let result = self.send_request("textDocument/references", params).await?;
        Self::format_locations(&result, "References")
    }

    /// textDocument/documentSymbol — list all symbols in a file
    pub async fn document_symbols(&mut self, file: &str) -> Result<String, String> {
        let uri = if file.starts_with("file://") {
            file.to_string()
        } else {
            let abs = std::path::Path::new(file)
                .canonicalize()
                .unwrap_or_else(|_| std::path::PathBuf::from(file));
            format!("file://{}", abs.display())
        };
        let params = serde_json::json!({
            "textDocument": { "uri": uri }
        });
        let result = self.send_request("textDocument/documentSymbol", params).await?;
        Self::format_symbols(&result)
    }

    /// textDocument/hover — get type info and documentation
    pub async fn hover(&mut self, file: &str, line: u32, col: u32) -> Result<String, String> {
        let params = Self::position_params(file, line, col);
        let result = self.send_request("textDocument/hover", params).await?;

        if result.is_null() {
            return Ok("No hover info available at this position.".into());
        }

        let contents = result.get("contents");
        match contents {
            Some(c) if c.is_string() => Ok(c.as_str().unwrap().to_string()),
            Some(c) if c.is_object() => {
                // MarkupContent { kind, value }
                Ok(c.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string())
            }
            Some(c) if c.is_array() => {
                let parts: Vec<String> = c.as_array().unwrap().iter().map(|item| {
                    if item.is_string() {
                        item.as_str().unwrap().to_string()
                    } else {
                        item.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string()
                    }
                }).collect();
                Ok(parts.join("\n"))
            }
            _ => Ok("No hover info available.".into()),
        }
    }

    /// Send textDocument/didOpen notification to warm up analysis.
    pub async fn did_open(&mut self, file: &str, content: &str) -> Result<(), String> {
        let uri = if file.starts_with("file://") {
            file.to_string()
        } else {
            let abs = std::path::Path::new(file)
                .canonicalize()
                .unwrap_or_else(|_| std::path::PathBuf::from(file));
            format!("file://{}", abs.display())
        };
        let language_id = match self.language.as_str() {
            "rust" => "rust",
            "python" => "python",
            "typescript" => "typescript",
            "go" => "go",
            _ => "plaintext",
        };
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 1,
                "text": content
            }
        });
        self.send_notification("textDocument/didOpen", params).await
    }

    /// Graceful shutdown.
    pub async fn shutdown(&mut self) {
        let _ = self.send_request("shutdown", serde_json::Value::Null).await;
        let _ = self.send_notification("exit", serde_json::Value::Null).await;
        let _ = self.process.kill().await;
        tracing::info!("[LSP] Server for '{}' shut down", self.language);
    }

    // ── Formatting helpers ──────────────────────────────────────────

    fn format_locations(result: &serde_json::Value, label: &str) -> Result<String, String> {
        if result.is_null() {
            return Ok(format!("No {} found.", label.to_lowercase()));
        }

        let locations = if result.is_array() {
            result.as_array().unwrap().clone()
        } else {
            vec![result.clone()]
        };

        if locations.is_empty() {
            return Ok(format!("No {} found.", label.to_lowercase()));
        }

        let mut out = format!("=== {} ({} found) ===\n", label, locations.len());
        for (i, loc) in locations.iter().enumerate() {
            let uri = loc.get("uri")
                .or_else(|| loc.get("targetUri"))
                .and_then(|u| u.as_str())
                .unwrap_or("?");

            let range = loc.get("range")
                .or_else(|| loc.get("targetRange"));

            let (line, col) = if let Some(r) = range {
                let start = r.get("start").unwrap_or(r);
                let l = start.get("line").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
                let c = start.get("character").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
                (l, c)
            } else {
                (0, 0)
            };

            // Strip file:// prefix for readability
            let path = uri.strip_prefix("file://").unwrap_or(uri);
            out.push_str(&format!("  {}. {}:{}:{}\n", i + 1, path, line, col));

            if i >= 24 {
                out.push_str(&format!("  ... and {} more\n", locations.len() - 25));
                break;
            }
        }
        Ok(out)
    }

    fn format_symbols(result: &serde_json::Value) -> Result<String, String> {
        if result.is_null() || (result.is_array() && result.as_array().unwrap().is_empty()) {
            return Ok("No symbols found.".into());
        }

        let symbols = result.as_array()
            .ok_or_else(|| "Unexpected symbol response format".to_string())?;

        let mut out = format!("=== Document Symbols ({}) ===\n", symbols.len());
        for sym in symbols {
            let name = sym.get("name").and_then(|n| n.as_str()).unwrap_or("?");
            let kind_num = sym.get("kind").and_then(|k| k.as_u64()).unwrap_or(0);
            let kind = symbol_kind_name(kind_num);

            let line = sym.get("range")
                .or_else(|| sym.get("location").and_then(|l| l.get("range")))
                .and_then(|r| r.get("start"))
                .and_then(|s| s.get("line"))
                .and_then(|l| l.as_u64())
                .map(|l| l + 1)
                .unwrap_or(0);

            out.push_str(&format!("  [{}] {} (line {})\n", kind, name, line));

            // Recursively format children if present
            if let Some(children) = sym.get("children").and_then(|c| c.as_array()) {
                for child in children {
                    let child_name = child.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                    let child_kind = symbol_kind_name(child.get("kind").and_then(|k| k.as_u64()).unwrap_or(0));
                    let child_line = child.get("range")
                        .and_then(|r| r.get("start"))
                        .and_then(|s| s.get("line"))
                        .and_then(|l| l.as_u64())
                        .map(|l| l + 1)
                        .unwrap_or(0);
                    out.push_str(&format!("    [{}] {} (line {})\n", child_kind, child_name, child_line));
                }
            }
        }
        Ok(out)
    }
}

/// Map LSP SymbolKind numbers to human-readable names.
fn symbol_kind_name(kind: u64) -> &'static str {
    match kind {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        15 => "string",
        16 => "number",
        17 => "boolean",
        18 => "array",
        19 => "object",
        20 => "key",
        21 => "null",
        22 => "enum_member",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type_param",
        _ => "unknown",
    }
}

// ── Public API for getting/creating clients from the registry ────────

/// Get the global LSP registry.
pub fn get_registry() -> Arc<Mutex<HashMap<String, LspClient>>> {
    LSP_REGISTRY.clone()
}
/// Get or create an LSP client for the given file's language.
pub async fn get_client_for_file(file_path: &str) -> Result<(), String> {
    let language = detect_language(file_path)
        .ok_or_else(|| format!("No LSP support for file type: {}", file_path))?;

    let mut registry = LSP_REGISTRY.lock().await;
    if registry.contains_key(&language) {
        return Ok(());
    }

    let root = std::env::var("HIVE_PROJECT_DIR")
        .unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string());

    let client = LspClient::spawn(&language, &root).await?;
    registry.insert(language, client);
    Ok(())
}

/// Execute an LSP action on the global registry.
pub async fn execute_action(action: &str, file: &str, line: u32, col: u32) -> Result<String, String> {
    let language = detect_language(file)
        .ok_or_else(|| format!("No LSP support for file type: {}", file))?;

    // Ensure client exists
    get_client_for_file(file).await?;

    let mut registry = LSP_REGISTRY.lock().await;
    let client = registry.get_mut(&language)
        .ok_or_else(|| format!("LSP client for '{}' not found in registry", language))?;

    match action {
        "definition" => client.goto_definition(file, line, col).await,
        "references" => client.find_references(file, line, col).await,
        "symbols" => client.document_symbols(file).await,
        "hover" => client.hover(file, line, col).await,
        _ => Err(format!("Unknown LSP action: {}", action)),
    }
}

/// Get status of all active LSP servers.
pub async fn get_status() -> String {
    let registry = LSP_REGISTRY.lock().await;
    if registry.is_empty() {
        return "No active LSP servers.".into();
    }
    let mut out = format!("=== Active LSP Servers ({}) ===\n", registry.len());
    for (lang, client) in registry.iter() {
        let idle = client.last_activity.elapsed().as_secs();
        out.push_str(&format!("  [{}] root={} idle {}s\n", lang, client.root_uri, idle));
    }
    out
}

/// Shutdown all active LSP servers.
pub async fn shutdown_all() {
    let mut registry = LSP_REGISTRY.lock().await;
    for (lang, client) in registry.iter_mut() {
        tracing::info!("[LSP] Shutting down server for '{}'", lang);
        client.shutdown().await;
    }
    registry.clear();
}

/// Shutdown servers that have been idle for more than the given duration.
pub async fn shutdown_idle(max_idle_secs: u64) {
    let mut registry = LSP_REGISTRY.lock().await;
    let idle_langs: Vec<String> = registry.iter()
        .filter(|(_, client)| client.last_activity.elapsed().as_secs() > max_idle_secs)
        .map(|(lang, _)| lang.clone())
        .collect();

    for lang in idle_langs {
        if let Some(mut client) = registry.remove(&lang) {
            tracing::info!("[LSP] Shutting down idle server for '{}' (idle {}s)", lang, client.last_activity.elapsed().as_secs());
            client.shutdown().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language_rust() {
        assert_eq!(detect_language("src/main.rs"), Some("rust".into()));
        assert_eq!(detect_language("foo/bar.rs"), Some("rust".into()));
    }

    #[test]
    fn test_detect_language_python() {
        assert_eq!(detect_language("script.py"), Some("python".into()));
        assert_eq!(detect_language("types.pyi"), Some("python".into()));
    }

    #[test]
    fn test_detect_language_typescript() {
        assert_eq!(detect_language("app.ts"), Some("typescript".into()));
        assert_eq!(detect_language("component.tsx"), Some("typescript".into()));
        assert_eq!(detect_language("index.js"), Some("typescript".into()));
        assert_eq!(detect_language("component.jsx"), Some("typescript".into()));
    }

    #[test]
    fn test_detect_language_go() {
        assert_eq!(detect_language("main.go"), Some("go".into()));
    }

    #[test]
    fn test_detect_language_unknown() {
        assert_eq!(detect_language("readme.md"), None);
        assert_eq!(detect_language("data.csv"), None);
        assert_eq!(detect_language("noext"), None);
    }

    #[test]
    fn test_symbol_kind_name() {
        assert_eq!(symbol_kind_name(12), "function");
        assert_eq!(symbol_kind_name(23), "struct");
        assert_eq!(symbol_kind_name(2), "module");
        assert_eq!(symbol_kind_name(6), "method");
        assert_eq!(symbol_kind_name(999), "unknown");
    }

    #[test]
    fn test_position_params() {
        let params = LspClient::position_params("src/main.rs", 10, 5);
        let pos = params.get("position").unwrap();
        // LSP is 0-indexed, our API is 1-indexed
        assert_eq!(pos.get("line").unwrap().as_u64().unwrap(), 9);
        assert_eq!(pos.get("character").unwrap().as_u64().unwrap(), 4);
    }

    #[test]
    fn test_format_locations_null() {
        let result = LspClient::format_locations(&serde_json::Value::Null, "Definition").unwrap();
        assert!(result.contains("No definition found"));
    }

    #[test]
    fn test_format_locations_empty_array() {
        let result = LspClient::format_locations(&serde_json::json!([]), "References").unwrap();
        assert!(result.contains("No references found"));
    }

    #[test]
    fn test_format_locations_with_results() {
        let locations = serde_json::json!([
            {
                "uri": "file:///project/src/main.rs",
                "range": { "start": { "line": 9, "character": 4 }, "end": { "line": 9, "character": 20 } }
            },
            {
                "uri": "file:///project/src/lib.rs",
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 10 } }
            }
        ]);
        let result = LspClient::format_locations(&locations, "References").unwrap();
        assert!(result.contains("References (2 found)"));
        assert!(result.contains("/project/src/main.rs:10:5"));
        assert!(result.contains("/project/src/lib.rs:1:1"));
    }

    #[test]
    fn test_format_symbols_empty() {
        let result = LspClient::format_symbols(&serde_json::json!([])).unwrap();
        assert!(result.contains("No symbols found"));
    }

    #[test]
    fn test_format_symbols_with_results() {
        let symbols = serde_json::json!([
            {
                "name": "main",
                "kind": 12,
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 5, "character": 1 } }
            },
            {
                "name": "MyStruct",
                "kind": 23,
                "range": { "start": { "line": 10, "character": 0 }, "end": { "line": 15, "character": 1 } },
                "children": [
                    {
                        "name": "new",
                        "kind": 6,
                        "range": { "start": { "line": 11, "character": 4 }, "end": { "line": 13, "character": 5 } }
                    }
                ]
            }
        ]);
        let result = LspClient::format_symbols(&symbols).unwrap();
        assert!(result.contains("[function] main (line 1)"));
        assert!(result.contains("[struct] MyStruct (line 11)"));
        assert!(result.contains("[method] new (line 12)"));
    }

    #[test]
    fn test_format_locations_single_object() {
        // Some servers return a single Location instead of an array
        let loc = serde_json::json!({
            "uri": "file:///project/src/foo.rs",
            "range": { "start": { "line": 4, "character": 0 }, "end": { "line": 4, "character": 10 } }
        });
        let result = LspClient::format_locations(&loc, "Definition").unwrap();
        assert!(result.contains("Definition (1 found)"));
        assert!(result.contains("/project/src/foo.rs:5:1"));
    }

    #[tokio::test]
    async fn test_get_status_empty() {
        let status = get_status().await;
        // May or may not be empty depending on other tests, but shouldn't panic
        assert!(!status.is_empty());
    }

    #[test]
    fn test_find_server_binary_unknown_language() {
        assert!(find_server_binary("brainfuck").is_none());
    }
}
