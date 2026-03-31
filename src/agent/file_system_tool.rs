use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;

fn extract_payload(desc: &str, prefix: &str) -> Option<String> {
    if let Some(start_idx) = desc.find(prefix) {
        let after = &desc[start_idx + prefix.len()..];
        let mut depth = 1;
        for (i, ch) in after.char_indices() {
            match ch {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(after[..i].to_string());
                    }
                }
                _ => {}
            }
        }
    }
    None
}

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

    // ── CONTAINMENT CONE: Block operations on Docker infrastructure ──
    if let Some(protected) = crate::agent::containment::check_path(&path_str) {
        tracing::warn!("[CONTAINMENT] 🛑 Blocked file_system_operator access to '{}' (protected: {})", path_str, protected);
        return ToolResult {
            task_id,
            output: format!("CONTAINMENT VIOLATION: '{}' is part of the Docker containment boundary and cannot be modified. You may edit any other file freely.", protected),
            tokens_used: 0,
            status: ToolStatus::Failed("Containment Boundary".into()),
        };
    }

    let final_output;
    let mut is_err = false;
    
    match action.as_str() {
        "write" => {
            let content = extract_payload(&desc, "content:[").unwrap_or_default();
            
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
            let content = extract_payload(&desc, "content:[").unwrap_or_default();
            
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
        "patch" => {
            let find_content = extract_payload(&desc, "find:[").unwrap_or_default();
            let replace_content = extract_payload(&desc, "replace:[").unwrap_or_default();
            
            if find_content.is_empty() {
                final_output = "Error: Missing find:[...] payload for patch.".into();
                is_err = true;
            } else {
                match tokio::fs::read_to_string(&path).await {
                    Ok(mut text) => {
                        if !text.contains(&find_content) {
                            final_output = "Error: The target find:[...] block was not found in the file exactly as provided. Check spacing/indentation.".into();
                            is_err = true;
                        } else {
                            text = text.replacen(&find_content, &replace_content, 1);
                            if let Err(e) = tokio::fs::write(&path, text).await {
                                final_output = format!("Failed to write patch: {}", e);
                                is_err = true;
                            } else {
                                final_output = format!("Successfully patched file {}", path_str);
                            }
                        }
                    }
                    Err(e) => {
                        final_output = format!("Failed to read file for patching: {}", e);
                        is_err = true;
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_missing_params() {
        let r = execute_file_system_operator("1".into(), "".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_unknown_action() {
        let r = execute_file_system_operator("1".into(), "action:[explode] path:[/tmp/x]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_write_and_delete() {
        let dir = std::env::temp_dir().join(format!("hive_fst_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("test.txt");
        let path_str = path.to_str().unwrap();

        let r = execute_file_system_operator("1".into(), format!("action:[write] path:[{}] content:[hello world]", path_str), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(path.exists());

        let r2 = execute_file_system_operator("2".into(), format!("action:[delete] path:[{}]", path_str), None).await;
        assert_eq!(r2.status, ToolStatus::Success);
        assert!(!path.exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_append() {
        let dir = std::env::temp_dir().join(format!("hive_fst_ap_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("append.txt");
        let path_str = path.to_str().unwrap();

        let _ = execute_file_system_operator("1".into(), format!("action:[write] path:[{}] content:[first]", path_str), None).await;
        let _ = execute_file_system_operator("2".into(), format!("action:[append] path:[{}] content:[ second]", path_str), None).await;
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "first second");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_patch() {
        let dir = std::env::temp_dir().join(format!("hive_fst_pa_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("patch.txt");
        let path_str = path.to_str().unwrap();

        let _ = execute_file_system_operator("1".into(), format!("action:[write] path:[{}] content:[hello world]", path_str), None).await;
        let r = execute_file_system_operator("2".into(), format!("action:[patch] path:[{}] find:[hello] replace:[goodbye]", path_str), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "goodbye world");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_patch_not_found() {
        let dir = std::env::temp_dir().join(format!("hive_fst_pnf_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("pnf.txt");
        let path_str = path.to_str().unwrap();

        let _ = execute_file_system_operator("1".into(), format!("action:[write] path:[{}] content:[abc]", path_str), None).await;
        let r = execute_file_system_operator("2".into(), format!("action:[patch] path:[{}] find:[xyz] replace:[123]", path_str), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_delete_nonexistent() {
        let r = execute_file_system_operator("1".into(), "action:[delete] path:[/tmp/hive_nonexistent_99999]".into(), None).await;
        assert_eq!(r.status, ToolStatus::Success); // verified does not exist
    }
}
