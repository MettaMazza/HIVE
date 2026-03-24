#![allow(clippy::ptr_arg)]
use crate::models::tool::{ToolResult, ToolStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ─── ForgedToolDef ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgedToolDef {
    pub name: String,
    pub description: String,
    pub language: String,       // "python" or "bash"
    pub script_filename: String, // e.g. "weather_check.py"
    pub timeout_secs: u64,
    pub created_at: f64,
    pub created_by: String,
    pub version: u32,
    pub enabled: bool,
}

// ─── Forge Registry ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
struct ForgeRegistryData {
    tools: Vec<ForgedToolDef>,
}


pub struct ToolForge {
    data: RwLock<ForgeRegistryData>,
    pub tools_dir: PathBuf,
}

impl ToolForge {
    pub fn new(project_root: &str) -> Self {
        let tools_dir = PathBuf::from(project_root).join("memory/tools");
        let data = Self::load(&tools_dir);
        Self {
            data: RwLock::new(data),
            tools_dir,
        }
    }

    fn registry_path(tools_dir: &PathBuf) -> PathBuf {
        tools_dir.join("registry.json")
    }

    fn load(tools_dir: &PathBuf) -> ForgeRegistryData {
        let path = Self::registry_path(tools_dir);
        if path.exists()
            && let Ok(raw) = std::fs::read_to_string(&path)
                && let Ok(data) = serde_json::from_str::<ForgeRegistryData>(&raw) {
                    return data;
                }
        ForgeRegistryData::default()
    }

    fn save(data: &ForgeRegistryData, tools_dir: &PathBuf) {
        let _ = std::fs::create_dir_all(tools_dir);
        let path = Self::registry_path(tools_dir);
        if let Ok(json) = serde_json::to_string_pretty(data) {
            let _ = std::fs::write(path, json);
        }
    }

    /// Get all enabled tools (for hot-loading into agent registry).
    pub async fn get_enabled_tools(&self) -> Vec<ForgedToolDef> {
        let data = self.data.read().await;
        data.tools.iter().filter(|t| t.enabled).cloned().collect()
    }

    /// Get a single tool by name.
    pub async fn get_tool(&self, name: &str) -> Option<ForgedToolDef> {
        let data = self.data.read().await;
        data.tools.iter().find(|t| t.name == name).cloned()
    }

    /// Create a new forged tool. Returns success message or error.
    pub async fn create_tool(
        &self,
        name: String,
        description: String,
        language: String,
        code: String,
        created_by: String,
    ) -> Result<String, String> {
        // Validate name
        if name.is_empty() || name.contains("..") || name.contains('/') || name.contains(' ') {
            return Err("Invalid tool name. Must be alphanumeric with underscores, no spaces or path chars.".into());
        }
        if !["python", "bash"].contains(&language.as_str()) {
            return Err("Language must be 'python' or 'bash'.".into());
        }

        let mut data = self.data.write().await;
        if data.tools.iter().any(|t| t.name == name) {
            return Err(format!("Tool '{}' already exists. Use edit to update it.", name));
        }

        let ext = if language == "python" { "py" } else { "sh" };
        let script_filename = format!("{}.{}", name, ext);
        let script_path = self.tools_dir.join(&script_filename);

        // Sandbox compilation check
        let tmp_dir = std::env::temp_dir().join("hive_forge_test");
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;
        let tmp_file = tmp_dir.join(format!("test_compile_{}.{}", name, ext));
        let _ = tokio::fs::write(&tmp_file, &code).await;

        let (cmd, args) = if language == "python" {
            ("python3", vec!["-m", "py_compile", tmp_file.to_str().unwrap()])
        } else {
            ("bash", vec!["-n", tmp_file.to_str().unwrap()])
        };

        match tokio::process::Command::new(cmd).args(&args).output().await {
            Ok(output) => {
                if !output.status.success() {
                    let err_msg = String::from_utf8_lossy(&output.stderr);
                    let _ = tokio::fs::remove_file(&tmp_file).await;
                    return Err(format!("Syntax Error in {} code:\n{}", language.to_uppercase(), err_msg));
                }
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp_file).await;
                return Err(format!("Failed to run syntax checker ({}): {}", cmd, e));
            }
        }
        let _ = tokio::fs::remove_file(&tmp_file).await;

        // Write script
        let _ = tokio::fs::create_dir_all(&self.tools_dir).await;
        tokio::fs::write(&script_path, &code).await.map_err(|e| format!("Failed to write script: {}", e))?;

        // Make bash scripts executable
        if language == "bash" {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = tokio::fs::metadata(&script_path).await {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o755);
                    let _ = tokio::fs::set_permissions(&script_path, perms).await;
                }
            }
        }

        let def = ForgedToolDef {
            name: name.clone(),
            description,
            language,
            script_filename,
            timeout_secs: 60,
            created_at: now_ts(),
            created_by,
            version: 1,
            enabled: true,
        };

        data.tools.push(def);
        Self::save(&data, &self.tools_dir);
        Ok(format!("✅ Forged tool '{}' created and enabled.", name))
    }

    /// Edit an existing tool's code. Bumps version.
    pub async fn edit_tool(&self, name: &str, code: String) -> Result<String, String> {
        let mut data = self.data.write().await;
        let tool = data.tools.iter_mut().find(|t| t.name == name)
            .ok_or_else(|| format!("Tool '{}' not found.", name))?;

        // Sandbox check
        let tmp_dir = std::env::temp_dir().join("hive_forge_test");
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;
        let tmp_file = tmp_dir.join(format!("test_compile_{}", tool.script_filename));
        let _ = tokio::fs::write(&tmp_file, &code).await;

        let (cmd, args) = if tool.language == "python" {
            ("python3", vec!["-m", "py_compile", tmp_file.to_str().unwrap()])
        } else {
            ("bash", vec!["-n", tmp_file.to_str().unwrap()])
        };

        if let Ok(output) = tokio::process::Command::new(cmd).args(&args).output().await {
            if !output.status.success() {
                let err_msg = String::from_utf8_lossy(&output.stderr);
                let _ = tokio::fs::remove_file(&tmp_file).await;
                return Err(format!("Syntax Error after edit:\n{}", err_msg));
            }
        }
        let _ = tokio::fs::remove_file(&tmp_file).await;

        let script_path = self.tools_dir.join(&tool.script_filename);
        tokio::fs::write(&script_path, &code).await.map_err(|e| format!("Failed to write: {}", e))?;
        tool.version += 1;
        tool.created_at = now_ts();
        let new_version = tool.version;
        let tool_name = tool.name.clone();

        Self::save(&data, &self.tools_dir);
        Ok(format!("✅ Tool '{}' updated to v{}.", tool_name, new_version))
    }

    /// Enable or disable a tool.
    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<String, String> {
        let mut data = self.data.write().await;
        let tool = data.tools.iter_mut().find(|t| t.name == name)
            .ok_or_else(|| format!("Tool '{}' not found.", name))?;
        tool.enabled = enabled;
        Self::save(&data, &self.tools_dir);
        let state = if enabled { "enabled" } else { "disabled" };
        Ok(format!("✅ Tool '{}' {}.", name, state))
    }

    /// Delete a tool and its script.
    pub async fn delete_tool(&self, name: &str) -> Result<String, String> {
        let mut data = self.data.write().await;
        let tool = data.tools.iter().find(|t| t.name == name)
            .ok_or_else(|| format!("Tool '{}' not found.", name))?;
        let script_path = self.tools_dir.join(&tool.script_filename);
        let _ = tokio::fs::remove_file(&script_path).await;
        data.tools.retain(|t| t.name != name);
        Self::save(&data, &self.tools_dir);
        Ok(format!("🗑️ Tool '{}' deleted.", name))
    }

    /// List all forged tools.
    pub async fn list_tools(&self) -> String {
        let data = self.data.read().await;
        if data.tools.is_empty() {
            return "No forged tools.".into();
        }
        let mut out = String::from("FORGED TOOLS:\n");
        for t in &data.tools {
            let status = if t.enabled { "✅" } else { "⛔" };
            out.push_str(&format!(
                "{} {} [{}] v{} — {}\n",
                status, t.name, t.language, t.version, t.description
            ));
        }
        out
    }
}

// ─── Execute a forged tool ─────────────────────────────────────────────────

pub async fn execute_forged_tool(
    task_id: String,
    description: String,
    tool_def: ForgedToolDef,
    tools_dir: PathBuf,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("🔧 Forged Tool `{}` executing...\n", tool_def.name)).await;
    }

    let script_path = tools_dir.join(&tool_def.script_filename);
    if !script_path.exists() {
        return ToolResult {
            task_id,
            output: format!("Forged tool '{}' script not found at {:?}", tool_def.name, script_path),
            tokens_used: 0,
            status: ToolStatus::Failed("Script missing".into()),
        };
    }

    // Parse description tags into JSON for stdin
    let input_json = tags_to_json(&description);

    let cmd = if tool_def.language == "python" { "python3" } else { "bash" };
    let path_str = script_path.to_string_lossy().to_string();

    match tokio::time::timeout(
        std::time::Duration::from_secs(tool_def.timeout_secs),
        async {
            let mut child = tokio::process::Command::new(cmd)
                .arg(&path_str)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to spawn: {}", e))?;

            // Write input JSON to stdin
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(input_json.as_bytes()).await;
                drop(stdin);
            }

            let output = child.wait_with_output().await
                .map_err(|e| format!("Process error: {}", e))?;

            Ok::<_, String>(output)
        }
    ).await {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                ToolResult {
                    task_id,
                    output: format!("--- {} OUTPUT ---\n{}{}", 
                        tool_def.name.to_uppercase(), 
                        stdout,
                        if stderr.is_empty() { String::new() } else { format!("\n[stderr] {}", stderr) }
                    ),
                    tokens_used: 0,
                    status: ToolStatus::Success,
                }
            } else {
                ToolResult {
                    task_id,
                    output: format!("--- {} FAILED (exit: {}) ---\n{}\n{}", 
                        tool_def.name.to_uppercase(), output.status, stdout, stderr),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Non-zero exit".into()),
                }
            }
        }
        Ok(Err(e)) => ToolResult {
            task_id,
            output: format!("Failed to execute forged tool: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e),
        },
        Err(_) => ToolResult {
            task_id,
            output: format!("Forged tool '{}' timed out after {}s. Process killed.", 
                tool_def.name, tool_def.timeout_secs),
            tokens_used: 0,
            status: ToolStatus::Failed("Timeout".into()),
        },
    }
}

/// Parse tag:[value] pairs from description into a JSON string for stdin.
fn tags_to_json(desc: &str) -> String {
    let mut map = HashMap::new();
    let mut remaining = desc;
    while let Some(tag_end) = remaining.find(":[") {
        // Find tag name (word before :[)
        let before = &remaining[..tag_end];
        let tag_name = before.rsplit_once(|c: char| c.is_whitespace())
            .map(|(_, t)| t)
            .unwrap_or(before);
        
        let after_bracket = &remaining[tag_end + 2..];
        if let Some(close) = after_bracket.find(']') {
            let value = &after_bracket[..close];
            map.insert(tag_name.to_string(), value.trim().to_string());
            remaining = &after_bracket[close + 1..];
        } else {
            break;
        }
    }
    // Also include full description as "raw_description"
    map.insert("raw_description".into(), desc.to_string());
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".into())
}

// ─── Tool Forge Agent Tool ─────────────────────────────────────────────────

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

/// Extract code content — special handling since code may contain brackets.
fn extract_code(desc: &str) -> Option<String> {
    let pattern = "code:[";
    if let Some(start_idx) = desc.find(pattern) {
        let after = &desc[start_idx + pattern.len()..];
        // Find the LAST ] to handle code containing brackets
        if let Some(end_idx) = after.rfind(']') {
            return Some(after[..end_idx].trim().to_string());
        }
    }
    None
}

pub async fn execute_tool_forge(
    task_id: String,
    description: String,
    forge: Arc<ToolForge>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("🔧 Tool Forge processing...\n".into()).await;
    }
    tracing::debug!("[AGENT:tool_forge] ▶ task_id={} desc_len={}", task_id, description.len());

    let action = extract_tag(&description, "action")
        .unwrap_or_else(|| "list".into())
        .to_lowercase();

    let output = match action.as_str() {
        "create" => {
            let name = match extract_tag(&description, "name") {
                Some(n) => n,
                None => return ToolResult {
                    task_id, output: "Missing: name:[tool_name]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing name".into()),
                },
            };
            let desc = extract_tag(&description, "description").unwrap_or_else(|| name.clone());
            let language = extract_tag(&description, "language").unwrap_or_else(|| "python".into());
            let code = match extract_code(&description) {
                Some(c) => c,
                None => return ToolResult {
                    task_id, output: "Missing: code:[THE CODE]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing code".into()),
                },
            };
            let created_by = extract_tag(&description, "created_by").unwrap_or_else(|| "unknown".into());

            match forge.create_tool(name, desc, language, code, created_by).await {
                Ok(msg) => msg,
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0, status: ToolStatus::Failed(e),
                },
            }
        }

        "dry_run" => {
            let name = extract_tag(&description, "name").unwrap_or_else(|| "dry_run_test".into());
            let language = extract_tag(&description, "language").unwrap_or_else(|| "python".into());
            let code = match extract_code(&description) {
                Some(c) => c,
                None => return ToolResult {
                    task_id, output: "Missing: code:[THE CODE]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing code".into()),
                },
            };

            let ext = if language == "python" { "py" } else { "sh" };
            let tmp_dir = std::env::temp_dir().join("hive_forge_test");
            let _ = tokio::fs::create_dir_all(&tmp_dir).await;
            let tmp_file = tmp_dir.join(format!("test_compile_dryrun_{}.{}", name, ext));
            let _ = tokio::fs::write(&tmp_file, &code).await;

            let (cmd, args) = if language == "python" {
                ("python3", vec!["-m", "py_compile", tmp_file.to_str().unwrap()])
            } else {
                ("bash", vec!["-n", tmp_file.to_str().unwrap()])
            };

            let output = match tokio::process::Command::new(cmd).args(&args).output().await {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if out.status.success() {
                        format!("✅ Dry Run Syntax OK for {} ({}).\nOutputs:\n{}\n{}", name, language, stdout, stderr)
                    } else {
                        format!("❌ Syntax Error in {}:\n{}", language.to_uppercase(), stderr)
                    }
                }
                Err(e) => format!("Failed to run syntax checker ({}): {}", cmd, e)
            };
            
            let _ = tokio::fs::remove_file(&tmp_file).await;
            output
        }

        "test" => {
            let name = match extract_tag(&description, "name") {
                Some(n) => n,
                None => return ToolResult {
                    task_id, output: "Missing: name:[tool_name]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing name".into()),
                },
            };
            let tool_def = match forge.get_tool(&name).await {
                Some(d) => d,
                None => return ToolResult {
                    task_id, output: format!("Tool '{}' not found.", name),
                    tokens_used: 0, status: ToolStatus::Failed("Not found".into()),
                },
            };
            let test_input = extract_tag(&description, "input").unwrap_or_else(|| "test".into());
            execute_forged_tool(
                task_id.clone(), test_input, tool_def, forge.tools_dir.clone(), telemetry_tx,
            ).await.output
        }

        "edit" => {
            let name = match extract_tag(&description, "name") {
                Some(n) => n,
                None => return ToolResult {
                    task_id, output: "Missing: name:[tool_name]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing name".into()),
                },
            };
            let code = match extract_code(&description) {
                Some(c) => c,
                None => return ToolResult {
                    task_id, output: "Missing: code:[UPDATED CODE]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing code".into()),
                },
            };
            match forge.edit_tool(&name, code).await {
                Ok(msg) => msg,
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0, status: ToolStatus::Failed(e),
                },
            }
        }

        "enable" => {
            let name = match extract_tag(&description, "name") {
                Some(n) => n,
                None => return ToolResult {
                    task_id, output: "Missing: name:[tool_name]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing name".into()),
                },
            };
            match forge.set_enabled(&name, true).await {
                Ok(msg) => msg,
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0, status: ToolStatus::Failed(e),
                },
            }
        }

        "disable" => {
            let name = match extract_tag(&description, "name") {
                Some(n) => n,
                None => return ToolResult {
                    task_id, output: "Missing: name:[tool_name]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing name".into()),
                },
            };
            match forge.set_enabled(&name, false).await {
                Ok(msg) => msg,
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0, status: ToolStatus::Failed(e),
                },
            }
        }

        "delete" => {
            let name = match extract_tag(&description, "name") {
                Some(n) => n,
                None => return ToolResult {
                    task_id, output: "Missing: name:[tool_name]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing name".into()),
                },
            };
            match forge.delete_tool(&name).await {
                Ok(msg) => msg,
                Err(e) => return ToolResult {
                    task_id, output: e.clone(), tokens_used: 0, status: ToolStatus::Failed(e),
                },
            }
        }

        "list" => forge.list_tools().await,

        _ => format!("Unknown action '{}'. Available: create, test, edit, enable, disable, delete, list", action),
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
    fn test_tags_to_json() {
        let input = "city:[London] units:[metric]";
        let json = tags_to_json(input);
        let parsed: HashMap<String, String> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.get("city"), Some(&"London".to_string()));
        assert_eq!(parsed.get("units"), Some(&"metric".to_string()));
    }

    #[test]
    fn test_extract_code() {
        let desc = "action:[create] name:[test] code:[print('hello')]";
        assert_eq!(extract_code(desc), Some("print('hello')".into()));

        // Code with brackets inside
        let desc2 = "code:[arr = [1, 2, 3]]";
        assert_eq!(extract_code(desc2), Some("arr = [1, 2, 3]".into()));
    }

    #[tokio::test]
    async fn test_forge_crud() {
        let _ = std::fs::remove_dir_all("/tmp/hive_test_forge/memory/tools");
        let forge = ToolForge::new("/tmp/hive_test_forge");
        
        // Create
        let result = forge.create_tool(
            "test_tool".into(),
            "A test tool".into(),
            "python".into(),
            "import sys, json\nargs = json.loads(sys.stdin.read())\nprint(json.dumps({'result': 'ok'}))".into(),
            "test".into(),
        ).await;
        assert!(result.is_ok());

        // List
        let list = forge.list_tools().await;
        assert!(list.contains("test_tool"));

        // Get
        let tool = forge.get_tool("test_tool").await;
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().version, 1);

        // Edit
        let result = forge.edit_tool("test_tool", "print('v2')".into()).await;
        assert!(result.is_ok());
        let tool = forge.get_tool("test_tool").await.unwrap();
        assert_eq!(tool.version, 2);

        // Disable
        forge.set_enabled("test_tool", false).await.unwrap();
        let enabled = forge.get_enabled_tools().await;
        assert!(enabled.is_empty());

        // Enable
        forge.set_enabled("test_tool", true).await.unwrap();
        let enabled = forge.get_enabled_tools().await;
        assert_eq!(enabled.len(), 1);

        // Duplicate create fails
        let result = forge.create_tool(
            "test_tool".into(), "".into(), "python".into(), "".into(), "test".into(),
        ).await;
        assert!(result.is_err());

        // Delete
        forge.delete_tool("test_tool").await.unwrap();
        assert!(forge.get_tool("test_tool").await.is_none());
    }

    #[tokio::test]
    async fn test_forge_validation() {
        let forge = ToolForge::new("/tmp/hive_test_forge_val");

        // Path traversal
        let result = forge.create_tool("../evil".into(), "".into(), "python".into(), "".into(), "".into()).await;
        assert!(result.is_err());

        // Invalid language
        let result = forge.create_tool("good".into(), "".into(), "ruby".into(), "".into(), "".into()).await;
        assert!(result.is_err());

        // Space in name
        let result = forge.create_tool("bad name".into(), "".into(), "python".into(), "".into(), "".into()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_forged_tool() {
        let _ = std::fs::remove_dir_all("/tmp/hive_test_forge_exec/memory/tools");
        let forge = ToolForge::new("/tmp/hive_test_forge_exec");
        forge.create_tool(
            "echo_tool".into(),
            "Echoes input".into(),
            "python".into(),
            "import sys, json\nargs = json.loads(sys.stdin.read())\nprint(json.dumps({'echo': args.get('message', 'empty')}))".into(),
            "test".into(),
        ).await.unwrap();

        let tool_def = forge.get_tool("echo_tool").await.unwrap();
        let result = execute_forged_tool(
            "t1".into(),
            "message:[hello world]".into(),
            tool_def,
            forge.tools_dir.clone(),
            None,
        ).await;
        assert_eq!(result.status, ToolStatus::Success);
        assert!(result.output.contains("hello world"));
    }
}
