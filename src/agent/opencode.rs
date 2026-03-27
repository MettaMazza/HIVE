//! OpenCode Bridge — Lifecycle manager + HTTP API wrapper for the OpenCode coding agent.
//!
//! Apis uses this to launch, drive, and manage OpenCode sessions for coding projects.
//! OpenCode runs as a background server process on port 4096, driven via its HTTP API.

use crate::models::tool::{ToolResult, ToolStatus};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

// ─── Constants ─────────────────────────────────────────────────────────────

const OPENCODE_PORT: u16 = 4096;
const OPENCODE_HOST: &str = "127.0.0.1";
const _IDLE_TIMEOUT_SECS: u64 = 1800; // 30 minutes

// ─── Config Generation ────────────────────────────────────────────────────

fn generate_opencode_config(_project_dir: &Path) -> String {
    serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "provider": {
            "ollama": {
                "npm": "@ai-sdk/openai-compatible",
                "name": "Ollama (HIVE Local)",
                "options": {
                    "baseURL": "http://localhost:11434/v1"
                },
                "models": {
                    "qwen3.5:35b": { "name": "Qwen3.5 35B (A3B MoE)" },
                    "qwen3:32b": { "name": "Qwen3 32B" },
                    "qwen3:14b": { "name": "Qwen3 14B" },
                    "qwen3:8b": { "name": "Qwen3 8B" },
                    "llama3.1:8b": { "name": "Llama 3.1 8B" }
                }
            }
        },
        "enabled_providers": ["ollama"],
        "permission": {
            "edit": "allow",
            "bash": "allow",
            "skill": "allow",
            "webfetch": "allow",
            "todowrite": "allow"
        },
        "server": {
            "port": OPENCODE_PORT,
            "hostname": OPENCODE_HOST
        }
    }).to_string()
}

// ─── Bridge State ─────────────────────────────────────────────────────────

#[derive(Debug)]
struct ServerState {
    child_pid: Option<u32>,
    project_dir: Option<PathBuf>,
    last_activity: std::time::Instant,
}

pub struct OpenCodeBridge {
    workspace_dir: PathBuf,
    state: RwLock<ServerState>,
    base_url: String,
}

impl OpenCodeBridge {
    pub fn new(project_root: &str) -> Self {
        let workspace_dir = PathBuf::from(project_root).join("workspace/opencode");
        let _ = std::fs::create_dir_all(&workspace_dir);
        Self {
            workspace_dir,
            state: RwLock::new(ServerState {
                child_pid: None,
                project_dir: None,
                last_activity: std::time::Instant::now(),
            }),
            base_url: format!("http://{}:{}", OPENCODE_HOST, OPENCODE_PORT),
        }
    }

    // ─── Lifecycle ─────────────────────────────────────────────────────

    /// Check if the OpenCode server is currently responding.
    pub async fn is_running(&self) -> bool {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap_or_default();
        client.get(&format!("{}/session", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Launch OpenCode TUI in its own Terminal.app window.
    pub async fn launch(&self, project_dir: &Path) -> Result<String, String> {
        if self.is_running().await {
            let state = self.state.read().await;
            if state.project_dir.as_deref() == Some(project_dir) {
                return Ok("OpenCode already running for this project.".into());
            }
            // Different project — shut down first
            drop(state);
            self.shutdown().await?;
        }

        // Ensure opencode.json exists in project dir
        let config_path = project_dir.join("opencode.json");
        if !config_path.exists() {
            let config = generate_opencode_config(project_dir);
            std::fs::write(&config_path, config)
                .map_err(|e| format!("Failed to write opencode.json: {}", e))?;
        }

        // Launch OpenCode in its OWN Terminal.app window via osascript
        let escaped_dir = project_dir.display().to_string().replace("\"", "\\\"");
        let applescript = format!(
            r#"tell application "Terminal"
    activate
    do script "cd \"{}\" && opencode"
end tell"#,
            escaped_dir
        );

        let _ = tokio::process::Command::new("osascript")
            .args(["-e", &applescript])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to launch Terminal: {}", e))?;

        // Wait for opencode to start, then find its PID
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        let pid = tokio::process::Command::new("pgrep")
            .args(["-n", "opencode"])
            .output()
            .await
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u32>().ok());

        let mut state = self.state.write().await;
        state.child_pid = pid;
        state.project_dir = Some(project_dir.to_path_buf());
        state.last_activity = std::time::Instant::now();
        drop(state);

        tracing::info!("[OPENCODE] TUI launched in new Terminal window for {:?} (pid: {:?})", project_dir, pid);
        Ok(format!("✅ OpenCode launched in new Terminal window for: {}", project_dir.display()))
    }

    /// Shut down the OpenCode server.
    pub async fn shutdown(&self) -> Result<String, String> {
        let mut state = self.state.write().await;

        // Kill by tracked PID first
        if let Some(pid) = state.child_pid.take() {
            let _ = tokio::process::Command::new("kill")
                .arg(pid.to_string())
                .output()
                .await;
            tracing::info!("[OPENCODE] Killed tracked process (pid {})", pid);
        }

        // Also kill any remaining opencode processes (belt + suspenders)
        let _ = tokio::process::Command::new("pkill")
            .args(["-f", "opencode"])
            .output()
            .await;

        state.project_dir = None;
        tracing::info!("[OPENCODE] Server shut down");
        Ok("✅ OpenCode shut down.".into())
    }

    /// Get server status.
    pub async fn status(&self) -> String {
        let state = self.state.read().await;
        let running = self.is_running().await;
        if running {
            format!("✅ OpenCode running on port {}\nProject: {}\nUptime: {}s",
                OPENCODE_PORT,
                state.project_dir.as_ref().map(|p| p.display().to_string()).unwrap_or("none".into()),
                state.last_activity.elapsed().as_secs()
            )
        } else {
            "⛔ OpenCode server is not running.".into()
        }
    }

    // ─── Session API ──────────────────────────────────────────────────

    async fn touch(&self) {
        let mut state = self.state.write().await;
        state.last_activity = std::time::Instant::now();
    }

    /// Create a new session.
    pub async fn create_session(&self, title: &str) -> Result<serde_json::Value, String> {
        self.touch().await;
        let client = reqwest::Client::new();
        let resp = client.post(&format!("{}/session", self.base_url))
            .json(&serde_json::json!({ "title": title }))
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?;
        
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Session create failed ({}): {}", status, body));
        }
        resp.json().await.map_err(|e| format!("JSON parse error: {}", e))
    }

    /// Send a prompt to a session.
    pub async fn send_prompt(&self, session_id: &str, text: &str, model_id: Option<&str>) -> Result<serde_json::Value, String> {
        self.touch().await;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .unwrap_or_default();

        let mut body = serde_json::json!({
            "parts": [{ "type": "text", "text": text }]
        });

        if let Some(model) = model_id {
            body["model"] = serde_json::json!({
                "providerID": "ollama",
                "modelID": model
            });
        }

        let resp = client.post(&format!("{}/session/{}/prompt", self.base_url, session_id))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?;
        
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Prompt failed ({}): {}", status, body));
        }
        resp.json().await.map_err(|e| format!("JSON parse error: {}", e))
    }

    /// Get messages from a session.
    pub async fn get_messages(&self, session_id: &str) -> Result<serde_json::Value, String> {
        let client = reqwest::Client::new();
        let resp = client.get(&format!("{}/session/{}/messages", self.base_url, session_id))
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?;
        
        if !resp.status().is_success() {
            return Err(format!("Messages fetch failed: {}", resp.status()));
        }
        resp.json().await.map_err(|e| format!("JSON parse error: {}", e))
    }

    /// List all sessions.
    pub async fn list_sessions(&self) -> Result<serde_json::Value, String> {
        let client = reqwest::Client::new();
        let resp = client.get(&format!("{}/session", self.base_url))
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?;
        resp.json().await.map_err(|e| format!("JSON parse error: {}", e))
    }

    /// Abort a session.
    pub async fn abort_session(&self, session_id: &str) -> Result<String, String> {
        let client = reqwest::Client::new();
        let resp = client.post(&format!("{}/session/{}/abort", self.base_url, session_id))
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?;
        
        if resp.status().is_success() {
            Ok("Session aborted.".into())
        } else {
            Err(format!("Abort failed: {}", resp.status()))
        }
    }

    // ─── Project Management ──────────────────────────────────────────

    /// Create a new project in the workspace.
    pub async fn create_project(&self, name: &str) -> Result<String, String> {
        if name.is_empty() || name.contains("..") || name.contains('/') {
            return Err("Invalid project name.".into());
        }
        let project_dir = self.workspace_dir.join(name);
        if project_dir.exists() {
            return Err(format!("Project '{}' already exists.", name));
        }
        
        tokio::fs::create_dir_all(&project_dir).await
            .map_err(|e| format!("Failed to create project dir: {}", e))?;

        // Init git
        let _ = tokio::process::Command::new("git")
            .args(["init"])
            .current_dir(&project_dir)
            .output()
            .await;

        // Write opencode.json
        let config = generate_opencode_config(&project_dir);
        tokio::fs::write(project_dir.join("opencode.json"), config).await
            .map_err(|e| format!("Failed to write config: {}", e))?;

        Ok(format!("✅ Project '{}' created at {}", name, project_dir.display()))
    }

    /// List all projects in the workspace.
    pub async fn list_projects(&self) -> String {
        let mut entries = match tokio::fs::read_dir(&self.workspace_dir).await {
            Ok(e) => e,
            Err(_) => return "No projects found.".into(),
        };

        let mut output = String::from("📁 OpenCode Projects:\n");
        let mut count = 0;
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') { continue; }
                let path = entry.path();
                let size = dir_size_human(&path);
                count += 1;
                output.push_str(&format!("  {} {} ({})\n", count, name, size));
            }
        }
        if count == 0 {
            return "No projects found. Use action:[create] to start one.".into();
        }
        output
    }

    /// Zip a project for delivery.
    pub async fn zip_project(&self, name: &str) -> Result<PathBuf, String> {
        let project_dir = self.workspace_dir.join(name);
        if !project_dir.exists() {
            return Err(format!("Project '{}' not found.", name));
        }
        let zip_path = self.workspace_dir.join(format!("{}.tar.gz", name));
        
        let output = tokio::process::Command::new("tar")
            .args(["-czf", &zip_path.to_string_lossy(), "-C", &self.workspace_dir.to_string_lossy(), name])
            .output()
            .await
            .map_err(|e| format!("tar failed: {}", e))?;

        if !output.status.success() {
            return Err(format!("tar failed: {}", String::from_utf8_lossy(&output.stderr)));
        }

        Ok(zip_path)
    }

    /// Open a project (launch server pointed at it).
    pub async fn open_project(&self, name: &str) -> Result<String, String> {
        let project_dir = self.workspace_dir.join(name);
        if !project_dir.exists() {
            return Err(format!("Project '{}' not found.", name));
        }
        self.launch(&project_dir).await
    }
}

/// Cleanup: kill OpenCode when HIVE exits so it doesn't orphan a Terminal window.
impl Drop for OpenCodeBridge {
    fn drop(&mut self) {
        // Synchronous kill — Drop can't be async
        if let Ok(state) = self.state.try_read() {
            if let Some(pid) = state.child_pid {
                let _ = std::process::Command::new("kill")
                    .arg(pid.to_string())
                    .output();
            }
        }
        // Belt + suspenders: pkill any remaining opencode processes
        let _ = std::process::Command::new("pkill")
            .args(["-f", "opencode"])
            .output();
        tracing::info!("[OPENCODE] Cleaned up on HIVE exit");
    }
}

fn dir_size_human(path: &Path) -> String {
    let size = walkdir_size(path);
    if size > 1_073_741_824 {
        format!("{:.1} GB", size as f64 / 1_073_741_824.0)
    } else if size > 1_048_576 {
        format!("{:.1} MB", size as f64 / 1_048_576.0)
    } else if size > 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{} B", size)
    }
}

fn walkdir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                total += p.metadata().map(|m| m.len()).unwrap_or(0);
            } else if p.is_dir() {
                total += walkdir_size(&p);
            }
        }
    }
    total
}

// ─── Tool Executor ─────────────────────────────────────────────────────────

fn extract_tag(desc: &str, tag: &str) -> Option<String> {
    let pattern = format!("{}:[", tag);
    if let Some(start_idx) = desc.find(&pattern) {
        let after = &desc[start_idx + pattern.len()..];
        if let Some(end_idx) = after.find(']') {
            return Some(after[..end_idx].trim().to_string());
        }
    }
    None
}

/// Extract the message content — uses last ] to allow brackets in content.
fn extract_message(desc: &str) -> Option<String> {
    let pattern = "message:[";
    if let Some(start_idx) = desc.find(pattern) {
        let after = &desc[start_idx + pattern.len()..];
        if let Some(end_idx) = after.rfind(']') {
            return Some(after[..end_idx].trim().to_string());
        }
    }
    None
}

pub async fn execute_opencode_tool(
    task_id: String,
    description: String,
    bridge: Arc<OpenCodeBridge>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("💻 OpenCode processing...\n".into()).await;
    }

    let action = extract_tag(&description, "action")
        .unwrap_or_else(|| "status".into())
        .to_lowercase();

    let output = match action.as_str() {
        // ── Lifecycle ──
        "launch" | "open" => {
            let project = extract_tag(&description, "project")
                .unwrap_or_else(|| "default".into());
            match bridge.open_project(&project).await {
                Ok(msg) => msg,
                Err(e) => {
                    return ToolResult {
                        task_id, output: e.clone(), tokens_used: 0,
                        status: ToolStatus::Failed(e),
                    };
                }
            }
        }
        "shutdown" | "stop" => {
            match bridge.shutdown().await {
                Ok(msg) => msg,
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0,
                    status: ToolStatus::Failed(e),
                },
            }
        }
        "status" => bridge.status().await,

        // ── Sessions ──
        "create_session" => {
            let title = extract_tag(&description, "title")
                .unwrap_or_else(|| "Apis Session".into());
            
            // Auto-launch if not running
            if !bridge.is_running().await {
                let project = extract_tag(&description, "project")
                    .unwrap_or_else(|| "default".into());
                if let Err(_e) = bridge.open_project(&project).await {
                    // Try creating the project first
                    let _ = bridge.create_project(&project).await;
                    if let Err(e2) = bridge.open_project(&project).await {
                        return ToolResult {
                            task_id, output: format!("Failed to launch: {}", e2),
                            tokens_used: 0, status: ToolStatus::Failed(e2),
                        };
                    }
                }
            }

            match bridge.create_session(&title).await {
                Ok(session) => {
                    let id = session.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
                    format!("✅ Session created: {}\nID: {}", title, id)
                }
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0,
                    status: ToolStatus::Failed(e),
                },
            }
        }
        "prompt" => {
            let session_id = match extract_tag(&description, "session") {
                Some(id) => id,
                None => return ToolResult {
                    task_id, output: "Missing: session:[session_id]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing session".into()),
                },
            };
            let message = extract_message(&description)
                .or_else(|| extract_tag(&description, "text"))
                .unwrap_or_else(|| description.clone());
            let model = extract_tag(&description, "model");

            match bridge.send_prompt(&session_id, &message, model.as_deref()).await {
                Ok(resp) => {
                    // Extract the assistant's response text
                    let text = resp.get("parts")
                        .and_then(|p| p.as_array())
                        .and_then(|arr| arr.iter().find(|p| p.get("type").and_then(|t| t.as_str()) == Some("text")))
                        .and_then(|p| p.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("[no text response]");
                    format!("--- OPENCODE RESPONSE ---\n{}", text)
                }
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0,
                    status: ToolStatus::Failed(e),
                },
            }
        }
        "messages" => {
            let session_id = match extract_tag(&description, "session") {
                Some(id) => id,
                None => return ToolResult {
                    task_id, output: "Missing: session:[session_id]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing session".into()),
                },
            };
            match bridge.get_messages(&session_id).await {
                Ok(msgs) => serde_json::to_string_pretty(&msgs).unwrap_or("[]".into()),
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0,
                    status: ToolStatus::Failed(e),
                },
            }
        }
        "list_sessions" => {
            match bridge.list_sessions().await {
                Ok(sessions) => serde_json::to_string_pretty(&sessions).unwrap_or("[]".into()),
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0,
                    status: ToolStatus::Failed(e),
                },
            }
        }
        "abort" => {
            let session_id = match extract_tag(&description, "session") {
                Some(id) => id,
                None => return ToolResult {
                    task_id, output: "Missing: session:[session_id]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing session".into()),
                },
            };
            match bridge.abort_session(&session_id).await {
                Ok(msg) => msg,
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0,
                    status: ToolStatus::Failed(e),
                },
            }
        }

        // ── Projects ──
        "create_project" | "create" => {
            let name = match extract_tag(&description, "project")
                .or_else(|| extract_tag(&description, "name")) {
                Some(n) => n,
                None => return ToolResult {
                    task_id, output: "Missing: project:[name]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing project name".into()),
                },
            };
            match bridge.create_project(&name).await {
                Ok(msg) => msg,
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0,
                    status: ToolStatus::Failed(e),
                },
            }
        }
        "list_projects" | "list" => bridge.list_projects().await,
        "zip" => {
            let name = match extract_tag(&description, "project")
                .or_else(|| extract_tag(&description, "name")) {
                Some(n) => n,
                None => return ToolResult {
                    task_id, output: "Missing: project:[name]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing project name".into()),
                },
            };
            match bridge.zip_project(&name).await {
                Ok(path) => format!("✅ Project zipped: {}", path.display()),
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0,
                    status: ToolStatus::Failed(e),
                },
            }
        }

        _ => format!("Unknown action '{}'. Available: launch, shutdown, status, create_session, prompt, messages, list_sessions, abort, create, list, zip", action),
    };

    ToolResult {
        task_id,
        output,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tag() {
        let desc = "action:[create] project:[my-app] model:[qwen3:32b]";
        assert_eq!(extract_tag(desc, "action"), Some("create".into()));
        assert_eq!(extract_tag(desc, "project"), Some("my-app".into()));
        assert_eq!(extract_tag(desc, "model"), Some("qwen3:32b".into()));
        assert_eq!(extract_tag(desc, "missing"), None);
    }

    #[test]
    fn test_extract_message() {
        let desc = "action:[prompt] session:[abc] message:[Build a React app with state management]";
        assert_eq!(extract_message(desc), Some("Build a React app with state management".into()));
    }

    #[test]
    fn test_extract_message_with_brackets() {
        let desc = "message:[Create an array like [1, 2, 3] in Python]";
        assert_eq!(extract_message(desc), Some("Create an array like [1, 2, 3] in Python".into()));
    }

    #[test]
    fn test_generate_config() {
        let config = generate_opencode_config(Path::new("/tmp/test"));
        let parsed: serde_json::Value = serde_json::from_str(&config).unwrap();
        assert_eq!(
            parsed["enabled_providers"][0].as_str(),
            Some("ollama")
        );
        assert_eq!(
            parsed["server"]["port"].as_u64(),
            Some(OPENCODE_PORT as u64)
        );
    }

    #[test]
    fn test_dir_size_human() {
        assert_eq!(dir_size_human(Path::new("/nonexistent")), "0 B");
    }

    #[tokio::test]
    async fn test_bridge_project_create() {
        let bridge = OpenCodeBridge::new("/tmp/hive_opencode_test");
        let _ = std::fs::remove_dir_all("/tmp/hive_opencode_test/workspace/opencode/test_proj");
        
        let result = bridge.create_project("test_proj").await;
        assert!(result.is_ok());
        assert!(PathBuf::from("/tmp/hive_opencode_test/workspace/opencode/test_proj").exists());
        
        // Duplicate should fail
        let result2 = bridge.create_project("test_proj").await;
        assert!(result2.is_err());

        // Invalid name should fail
        assert!(bridge.create_project("../evil").await.is_err());
        assert!(bridge.create_project("").await.is_err());

        // Cleanup
        let _ = std::fs::remove_dir_all("/tmp/hive_opencode_test");
    }

    #[tokio::test]
    async fn test_bridge_status_when_stopped() {
        let bridge = OpenCodeBridge::new("/tmp/hive_opencode_test2");
        let status = bridge.status().await;
        assert!(status.contains("not running"));
        let _ = std::fs::remove_dir_all("/tmp/hive_opencode_test2");
    }
}
