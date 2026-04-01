use crate::models::tool::{ToolResult, ToolStatus};
use crate::agent::preferences::extract_tag;
use tokio::sync::mpsc;

/// LSP Tool — Language Server Protocol integration for IDE-grade code intelligence.
/// Provides go-to-definition, find-references, document-symbols, hover, and server status.
pub async fn execute_lsp_tool(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("🔍 LSP Tool executing...\n".to_string()).await;
    }

    let action = extract_tag(&description, "action:").unwrap_or_default();

    if action.is_empty() {
        return ToolResult {
            task_id,
            output: "Error: Missing action:[...]. Valid actions: definition, references, symbols, hover, diagnostics, status.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing action".into()),
        };
    }

    // Status doesn't need a file
    if action == "status" {
        let status = crate::agent::lsp_client::get_status().await;
        return ToolResult {
            task_id,
            output: status,
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    let file = extract_tag(&description, "file:").unwrap_or_default();
    if file.is_empty() {
        return ToolResult {
            task_id,
            output: "Error: Missing file:[...] for LSP action.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing file".into()),
        };
    }

    // Resolve the file path
    let resolved_file = resolve_file_path(&file);

    match action.as_str() {
        "definition" | "references" | "hover" => {
            let line: u32 = extract_tag(&description, "line:")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
            let col: u32 = extract_tag(&description, "col:")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);

            if let Some(ref tx) = telemetry_tx {
                let _ = tx.send(format!("  📍 {}:{} col:{} action:{}\n", resolved_file, line, col, action)).await;
            }

            match crate::agent::lsp_client::execute_action(&action, &resolved_file, line, col).await {
                Ok(output) => ToolResult {
                    task_id,
                    output,
                    tokens_used: 0,
                    status: ToolStatus::Success,
                },
                Err(e) => ToolResult {
                    task_id,
                    output: format!("LSP {} failed: {}", action, e),
                    tokens_used: 0,
                    status: ToolStatus::Failed(e),
                },
            }
        }
        "symbols" => {
            if let Some(ref tx) = telemetry_tx {
                let _ = tx.send(format!("  📋 Listing symbols in {}\n", resolved_file)).await;
            }

            match crate::agent::lsp_client::execute_action("symbols", &resolved_file, 0, 0).await {
                Ok(output) => ToolResult {
                    task_id,
                    output,
                    tokens_used: 0,
                    status: ToolStatus::Success,
                },
                Err(e) => ToolResult {
                    task_id,
                    output: format!("LSP symbols failed: {}", e),
                    tokens_used: 0,
                    status: ToolStatus::Failed(e),
                },
            }
        }
        "diagnostics" => {
            // For diagnostics, we use the rust-analyzer / pyright diagnostics
            // This is a simplified version — full diagnostics require tracking
            // publishDiagnostics notifications asynchronously.
            // For now, we compile and parse the output.
            let language = crate::agent::lsp_client::detect_language(&resolved_file);
            let output = match language.as_deref() {
                Some("rust") => {
                    match tokio::process::Command::new("cargo")
                        .args(["check", "--message-format=short"])
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .kill_on_drop(true)
                        .output()
                        .await
                    {
                        Ok(out) => {
                            let stdout = String::from_utf8_lossy(&out.stdout);
                            let stderr = String::from_utf8_lossy(&out.stderr);
                            let combined = format!("{}{}", stdout, stderr);
                            // Filter to relevant file if specified
                            if !file.is_empty() && file != "." {
                                let relevant: Vec<&str> = combined.lines()
                                    .filter(|l| l.contains(&file) || l.starts_with("error") || l.starts_with("warning"))
                                    .collect();
                                if relevant.is_empty() {
                                    format!("No diagnostics for '{}'.", file)
                                } else {
                                    format!("=== Diagnostics for {} ===\n{}", file, relevant.join("\n"))
                                }
                            } else {
                                format!("=== Project Diagnostics ===\n{}", combined.trim())
                            }
                        }
                        Err(e) => format!("Failed to run cargo check: {}", e),
                    }
                }
                Some("python") => {
                    // Try pyright or mypy
                    let result = tokio::process::Command::new("pyright")
                        .arg(&resolved_file)
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .kill_on_drop(true)
                        .output()
                        .await;
                    match result {
                        Ok(out) => {
                            let output = String::from_utf8_lossy(&out.stdout);
                            format!("=== Python Diagnostics ===\n{}", output.trim())
                        }
                        Err(_) => "pyright not installed. Install with: npm install -g pyright".into(),
                    }
                }
                Some("typescript") => {
                    let result = tokio::process::Command::new("npx")
                        .args(["tsc", "--noEmit", "--pretty"])
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .kill_on_drop(true)
                        .output()
                        .await;
                    match result {
                        Ok(out) => {
                            let output = String::from_utf8_lossy(&out.stdout);
                            format!("=== TypeScript Diagnostics ===\n{}", output.trim())
                        }
                        Err(e) => format!("Failed to run tsc: {}", e),
                    }
                }
                _ => format!("No diagnostics support for file type: {}", file),
            };

            ToolResult {
                task_id,
                output,
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        _ => ToolResult {
            task_id,
            output: format!("Unknown LSP action: '{}'. Valid: definition, references, symbols, hover, diagnostics, status.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        },
    }
}

/// Resolve a relative file path to an absolute path using HIVE_PROJECT_DIR.
fn resolve_file_path(file: &str) -> String {
    if std::path::Path::new(file).is_absolute() {
        return file.to_string();
    }
    let project_root = std::env::var("HIVE_PROJECT_DIR")
        .unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string());
    let resolved = std::path::Path::new(&project_root).join(file);
    resolved.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lsp_missing_action() {
        let r = execute_lsp_tool("1".into(), "".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Missing action"));
    }

    #[tokio::test]
    async fn test_lsp_unknown_action() {
        let r = execute_lsp_tool("1".into(), "action:[explode] file:[foo.rs]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Unknown LSP action"));
    }

    #[tokio::test]
    async fn test_lsp_status() {
        let r = execute_lsp_tool("1".into(), "action:[status]".into(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        // Should return server status or "No active" message
        assert!(!r.output.is_empty());
    }

    #[tokio::test]
    async fn test_lsp_missing_file() {
        let r = execute_lsp_tool("1".into(), "action:[definition]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Missing file"));
    }

    #[tokio::test]
    async fn test_lsp_definition_no_server() {
        // .md files have no LSP support
        let r = execute_lsp_tool("1".into(), "action:[definition] file:[readme.md] line:[1] col:[1]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("No LSP support"));
    }

    #[tokio::test]
    async fn test_lsp_symbols_no_server() {
        let r = execute_lsp_tool("1".into(), "action:[symbols] file:[data.csv]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[test]
    fn test_resolve_file_path_absolute() {
        let result = resolve_file_path("/absolute/path/to/file.rs");
        assert_eq!(result, "/absolute/path/to/file.rs");
    }

    #[test]
    fn test_resolve_file_path_relative() {
        let result = resolve_file_path("src/main.rs");
        assert!(result.contains("src/main.rs"));
        assert!(result.starts_with('/'));
    }

    #[tokio::test]
    async fn test_lsp_diagnostics_unknown_language() {
        let r = execute_lsp_tool("1".into(), "action:[diagnostics] file:[data.csv]".into(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("No diagnostics support"));
    }
}
