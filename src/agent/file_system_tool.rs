use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;

pub async fn execute_file_system_operator(
    task_id: String,
    desc: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("📁 Native File System Operator executing...\n".to_string()).await;
    }
    
    let action = crate::agent::preferences::extract_tag(&desc, "action:").unwrap_or_default();
    let path_str = crate::agent::preferences::extract_tag(&desc, "path:").unwrap_or_default();
    tracing::debug!("[AGENT:file_system] ▶ task_id={} action='{}' path='{}'", task_id, action, path_str);
    
    if action.is_empty() || path_str.is_empty() {
        return ToolResult {
            task_id,
            output: "Error: Missing action:[...] or path:[...]".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Invalid Args".into()),
        };
    }
    
    let path = std::path::Path::new(&path_str);
    let final_output;
    let mut is_err = false;
    
    match action.as_str() {
        "write" => {
            let content = if let Some(idx) = desc.find("content:[") {
                let mut content_body = desc[idx + 9..].to_string();
                if content_body.ends_with(']') {
                    content_body.pop();
                }
                content_body
            } else {
                "".to_string()
            };
            
            if let Some(parent) = path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            if let Err(e) = tokio::fs::write(&path, content).await {
                final_output = format!("Failed to write: {}", e);
                is_err = true;
            } else {
                final_output = format!("Successfully wrote to {}", path_str);
            }
        }
        "append" => {
            let content = if let Some(idx) = desc.find("content:[") {
                let mut content_body = desc[idx + 9..].to_string();
                if content_body.ends_with(']') {
                    content_body.pop();
                }
                content_body
            } else {
                "".to_string()
            };
            
            use tokio::io::AsyncWriteExt;
            match tokio::fs::OpenOptions::new().create(true).append(true).open(&path).await {
                Ok(mut file) => {
                    if let Err(e) = file.write_all(content.as_bytes()).await {
                        final_output = format!("Failed to append: {}", e);
                        is_err = true;
                    } else {
                        final_output = format!("Successfully appended to {}", path_str);
                    }
                }
                Err(e) => {
                    final_output = format!("Failed to open for append: {}", e);
                    is_err = true;
                }
            }
        }
        "delete" => {
            if path.is_file() {
                if let Err(e) = tokio::fs::remove_file(&path).await {
                    final_output = format!("Failed to delete file: {}", e);
                    is_err = true;
                } else {
                    final_output = format!("Successfully deleted file {}", path_str);
                }
            } else if path.is_dir() {
                if let Err(e) = tokio::fs::remove_dir_all(&path).await {
                    final_output = format!("Failed to delete directory: {}", e);
                    is_err = true;
                } else {
                    final_output = format!("Successfully deleted directory {}", path_str);
                }
            } else {
                final_output = format!("Successfully verified {} does not exist", path_str);
            }
        }
        _ => {
            final_output = format!("Unknown action: {}", action);
            is_err = true;
        }
    }
    
    ToolResult {
        task_id,
        output: final_output.clone(),
        tokens_used: 0,
        status: if is_err { ToolStatus::Failed(final_output) } else { ToolStatus::Success },
    }
}
