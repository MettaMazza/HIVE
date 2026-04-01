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
    
    // Rollback and checkpoints actions don't require a path
    if action != "rollback" && action != "checkpoints" {
        if action.is_empty() || path_str.is_empty() {
            return ToolResult {
                task_id,
                output: "Error: Missing action:[...] or path:[...]".into(),
                tokens_used: 0,
                status: ToolStatus::Failed("Invalid Args".into()),
            };
        }
    } else if action.is_empty() {
        return ToolResult {
            task_id,
            output: "Error: Missing action:[...]".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Invalid Args".into()),
        };
    }
    
    let path = std::path::Path::new(&path_str);

    // ── CONTAINMENT CONE: Block operations on Docker infrastructure ──
    if !path_str.is_empty() {
        if let Some(protected) = crate::agent::containment::check_path(&path_str) {
            tracing::warn!("[CONTAINMENT] 🛑 Blocked file_system_operator access to '{}' (protected: {})", path_str, protected);
            return ToolResult {
                task_id,
                output: format!("CONTAINMENT VIOLATION: '{}' is part of the Docker containment boundary and cannot be modified. You may edit any other file freely.", protected),
                tokens_used: 0,
                status: ToolStatus::Failed("Containment Boundary".into()),
            };
        }
    }

    // Lazy-init checkpoint manager (only for actions that need it)
    let checkpoint_mgr = crate::engine::checkpoint::CheckpointManager::new();

    let mut final_output;
    let mut is_err = false;
    
    match action.as_str() {
        "write" => {
            let content = extract_payload(&desc, "content:[").unwrap_or_default();
            
            // Auto-snapshot before overwriting an existing file
            if path.exists() {
                match checkpoint_mgr.snapshot(path).await {
                    Ok(id) => {
                        if let Some(ref tx) = telemetry_tx {
                            let _ = tx.send(format!("  📸 Checkpoint '{}' created before write.\n", id)).await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[CHECKPOINT] Pre-write snapshot failed (non-fatal): {}", e);
                    }
                }
            }

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
                // Auto-snapshot before deleting
                match checkpoint_mgr.snapshot(path).await {
                    Ok(id) => {
                        if let Some(ref tx) = telemetry_tx {
                            let _ = tx.send(format!("  📸 Checkpoint '{}' created before delete.\n", id)).await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[CHECKPOINT] Pre-delete snapshot failed (non-fatal): {}", e);
                    }
                }
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
                            // Auto-snapshot before patching
                            match checkpoint_mgr.snapshot(path).await {
                                Ok(id) => {
                                    if let Some(ref tx) = telemetry_tx {
                                        let _ = tx.send(format!("  📸 Checkpoint '{}' created before patch.\n", id)).await;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("[CHECKPOINT] Pre-patch snapshot failed (non-fatal): {}", e);
                                }
                            }
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
        "multi_patch" => {
            // Parse multiple find/replace pairs: find_1:[...] replace_1:[...] find_2:[...] replace_2:[...]
            match tokio::fs::read_to_string(&path).await {
                Ok(mut text) => {
                    // Auto-snapshot before multi-patch
                    match checkpoint_mgr.snapshot(path).await {
                        Ok(id) => {
                            if let Some(ref tx) = telemetry_tx {
                                let _ = tx.send(format!("  📸 Checkpoint '{}' created before multi_patch.\n", id)).await;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("[CHECKPOINT] Pre-multi_patch snapshot failed (non-fatal): {}", e);
                        }
                    }

                    let mut applied = 0;
                    let mut errors = Vec::new();

                    for i in 1..=20 {
                        let find_key = format!("find_{}:[", i);
                        let replace_key = format!("replace_{}:[", i);
                        let find = extract_payload(&desc, &find_key);
                        let replace = extract_payload(&desc, &replace_key);

                        match (find, replace) {
                            (Some(f), Some(r)) => {
                                if !text.contains(&f) {
                                    errors.push(format!("Pair {}: find text not found in file", i));
                                } else {
                                    text = text.replacen(&f, &r, 1);
                                    applied += 1;
                                }
                            }
                            (Some(_), None) => {
                                errors.push(format!("Pair {}: missing replace_{}:[...]", i, i));
                            }
                            _ => break, // No more pairs
                        }
                    }

                    if applied > 0 {
                        if let Err(e) = tokio::fs::write(&path, text).await {
                            final_output = format!("Failed to write multi_patch: {}", e);
                            is_err = true;
                        } else if errors.is_empty() {
                            final_output = format!("Successfully applied {} patch(es) to {}", applied, path_str);
                        } else {
                            final_output = format!("Applied {} patch(es) to {} with {} error(s): {}", 
                                applied, path_str, errors.len(), errors.join("; "));
                        }
                    } else {
                        final_output = format!("No patches applied. Errors: {}", errors.join("; "));
                        is_err = true;
                    }
                }
                Err(e) => {
                    final_output = format!("Failed to read file for multi_patch: {}", e);
                    is_err = true;
                }
            }
        }
        "insert_after" => {
            let anchor = extract_payload(&desc, "anchor:[").unwrap_or_default();
            let content = extract_payload(&desc, "content:[").unwrap_or_default();

            if anchor.is_empty() {
                final_output = "Error: Missing anchor:[...] for insert_after.".into();
                is_err = true;
            } else {
                match tokio::fs::read_to_string(&path).await {
                    Ok(text) => {
                        if let Some(pos) = text.find(&anchor) {
                            // Auto-snapshot
                            match checkpoint_mgr.snapshot(path).await {
                                Ok(id) => {
                                    if let Some(ref tx) = telemetry_tx {
                                        let _ = tx.send(format!("  📸 Checkpoint '{}' created before insert_after.\n", id)).await;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("[CHECKPOINT] Pre-insert_after snapshot failed (non-fatal): {}", e);
                                }
                            }
                            let insert_pos = pos + anchor.len();
                            let mut new_text = String::with_capacity(text.len() + content.len());
                            new_text.push_str(&text[..insert_pos]);
                            new_text.push_str(&content);
                            new_text.push_str(&text[insert_pos..]);
                            if let Err(e) = tokio::fs::write(&path, new_text).await {
                                final_output = format!("Failed to write insert_after: {}", e);
                                is_err = true;
                            } else {
                                final_output = format!("Successfully inserted content after anchor in {}", path_str);
                            }
                        } else {
                            final_output = "Error: The anchor:[...] text was not found in the file.".into();
                            is_err = true;
                        }
                    }
                    Err(e) => {
                        final_output = format!("Failed to read file for insert_after: {}", e);
                        is_err = true;
                    }
                }
            }
        }
        "insert_before" => {
            let anchor = extract_payload(&desc, "anchor:[").unwrap_or_default();
            let content = extract_payload(&desc, "content:[").unwrap_or_default();

            if anchor.is_empty() {
                final_output = "Error: Missing anchor:[...] for insert_before.".into();
                is_err = true;
            } else {
                match tokio::fs::read_to_string(&path).await {
                    Ok(text) => {
                        if let Some(pos) = text.find(&anchor) {
                            // Auto-snapshot
                            match checkpoint_mgr.snapshot(path).await {
                                Ok(id) => {
                                    if let Some(ref tx) = telemetry_tx {
                                        let _ = tx.send(format!("  📸 Checkpoint '{}' created before insert_before.\n", id)).await;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("[CHECKPOINT] Pre-insert_before snapshot failed (non-fatal): {}", e);
                                }
                            }
                            let mut new_text = String::with_capacity(text.len() + content.len());
                            new_text.push_str(&text[..pos]);
                            new_text.push_str(&content);
                            new_text.push_str(&text[pos..]);
                            if let Err(e) = tokio::fs::write(&path, new_text).await {
                                final_output = format!("Failed to write insert_before: {}", e);
                                is_err = true;
                            } else {
                                final_output = format!("Successfully inserted content before anchor in {}", path_str);
                            }
                        } else {
                            final_output = "Error: The anchor:[...] text was not found in the file.".into();
                            is_err = true;
                        }
                    }
                    Err(e) => {
                        final_output = format!("Failed to read file for insert_before: {}", e);
                        is_err = true;
                    }
                }
            }
        }
        "rollback" => {
            let checkpoint_id = crate::agent::preferences::extract_tag(&desc, "checkpoint:").unwrap_or_default();
            if checkpoint_id.is_empty() {
                final_output = "Error: Missing checkpoint:[...] ID for rollback.".into();
                is_err = true;
            } else {
                match checkpoint_mgr.rollback(&checkpoint_id).await {
                    Ok(msg) => final_output = msg,
                    Err(e) => {
                        final_output = format!("Rollback failed: {}", e);
                        is_err = true;
                    }
                }
            }
        }
        "checkpoints" => {
            // Auto-prune stale checkpoints (>24h) on each list call
            let pruned = checkpoint_mgr.prune(24).await;
            let limit: usize = crate::agent::preferences::extract_tag(&desc, "limit:")
                .and_then(|s| s.parse().ok())
                .unwrap_or(10);
            let entries = checkpoint_mgr.list(limit).await;
            if entries.is_empty() {
                final_output = if pruned > 0 {
                    format!("No active checkpoints (pruned {} stale entries).", pruned)
                } else {
                    "No checkpoints found.".into()
                };
            } else {
                let mut out = format!("Recent checkpoints ({}):\n", entries.len());
                for entry in &entries {
                    out.push_str(&format!(
                        "  - [{}] {} ({} bytes) — {}\n",
                        entry.id, entry.original_path, entry.size_bytes, entry.created_at
                    ));
                }
                if pruned > 0 {
                    out.push_str(&format!("(Auto-pruned {} stale checkpoint(s) older than 24h)\n", pruned));
                }
                final_output = out;
            }
        }
        _ => {
            final_output = format!("Unknown action: {}", action);
            is_err = true;
        }
    }
    
    // Auto-diagnostics: If the action compiled successfully and modified a file, request LSP diagnostics
    if !is_err && matches!(action.as_str(), "write" | "append" | "patch" | "multi_patch" | "insert_after" | "insert_before" | "delete") {
        if !path_str.is_empty() && crate::agent::lsp_client::detect_language(&path_str).is_some() {
            let diag_desc = format!("action:[diagnostics] file:[{}]", path_str);
            let diag_result = crate::agent::lsp_tool::execute_lsp_tool("internal".into(), diag_desc, None).await;
            if diag_result.status == ToolStatus::Success {
                final_output.push_str("\n\n");
                final_output.push_str(&diag_result.output);
            }
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

    #[tokio::test(flavor = "multi_thread")]
    async fn test_multi_patch() {
        let dir = std::env::temp_dir().join(format!("hive_fst_mp_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("multi.txt");
        let path_str = path.to_str().unwrap();

        let _ = execute_file_system_operator("1".into(), format!("action:[write] path:[{}] content:[alpha beta gamma]", path_str), None).await;
        let r = execute_file_system_operator("2".into(), format!(
            "action:[multi_patch] path:[{}] find_1:[alpha] replace_1:[ALPHA] find_2:[gamma] replace_2:[GAMMA]", path_str
        ), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "ALPHA beta GAMMA");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_multi_patch_partial_failure() {
        let dir = std::env::temp_dir().join(format!("hive_fst_mpf_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("partial.txt");
        let path_str = path.to_str().unwrap();

        let _ = execute_file_system_operator("1".into(), format!("action:[write] path:[{}] content:[hello world]", path_str), None).await;
        // find_1 matches, find_2 does NOT match
        let r = execute_file_system_operator("2".into(), format!(
            "action:[multi_patch] path:[{}] find_1:[hello] replace_1:[hi] find_2:[missing_text] replace_2:[x]", path_str
        ), None).await;
        // Should still succeed because at least 1 patch applied
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("1 patch(es)"));
        assert!(r.output.contains("error(s)"));
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "hi world");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_multi_patch_all_fail() {
        let dir = std::env::temp_dir().join(format!("hive_fst_maf_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("allfail.txt");
        let path_str = path.to_str().unwrap();

        let _ = execute_file_system_operator("1".into(), format!("action:[write] path:[{}] content:[abc]", path_str), None).await;
        let r = execute_file_system_operator("2".into(), format!(
            "action:[multi_patch] path:[{}] find_1:[xyz] replace_1:[123]", path_str
        ), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_insert_after() {
        let dir = std::env::temp_dir().join(format!("hive_fst_ia_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("insert_after.txt");
        let path_str = path.to_str().unwrap();

        let _ = execute_file_system_operator("1".into(), format!("action:[write] path:[{}] content:[fn main() {{\n    println!(\"hello\");\n}}]", path_str), None).await;
        let r = execute_file_system_operator("2".into(), format!(
            "action:[insert_after] path:[{}] anchor:[fn main() {{] content:[\n    // inserted comment]", path_str
        ), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("// inserted comment"));
        assert!(content.contains("fn main() {"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_insert_after_anchor_not_found() {
        let dir = std::env::temp_dir().join(format!("hive_fst_ianf_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("no_anchor.txt");
        let path_str = path.to_str().unwrap();

        let _ = execute_file_system_operator("1".into(), format!("action:[write] path:[{}] content:[abc]", path_str), None).await;
        let r = execute_file_system_operator("2".into(), format!(
            "action:[insert_after] path:[{}] anchor:[xyz] content:[new stuff]", path_str
        ), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("not found"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_insert_before() {
        let dir = std::env::temp_dir().join(format!("hive_fst_ib_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = dir.join("insert_before.txt");
        let path_str = path.to_str().unwrap();

        let _ = execute_file_system_operator("1".into(), format!("action:[write] path:[{}] content:[world]", path_str), None).await;
        let r = execute_file_system_operator("2".into(), format!(
            "action:[insert_before] path:[{}] anchor:[world] content:[hello ]", path_str
        ), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "hello world");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_insert_before_missing_anchor() {
        let r = execute_file_system_operator("1".into(), "action:[insert_before] path:[/tmp/x] anchor:[] content:[y]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Missing anchor"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_insert_after_missing_anchor() {
        let r = execute_file_system_operator("1".into(), "action:[insert_after] path:[/tmp/x] anchor:[] content:[y]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Missing anchor"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_rollback_missing_id() {
        let r = execute_file_system_operator("1".into(), "action:[rollback] checkpoint:[]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Missing checkpoint"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_rollback_nonexistent_id() {
        let r = execute_file_system_operator("1".into(), "action:[rollback] checkpoint:[fake_id_123]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Rollback failed"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_checkpoints_empty() {
        // Checkpoints action with no path needed — just list
        let r = execute_file_system_operator("1".into(), "action:[checkpoints]".into(), None).await;
        // This may show existing checkpoints or "No checkpoints found"
        assert_eq!(r.status, ToolStatus::Success);
    }
}
