use crate::models::tool::{ToolResult, ToolStatus};
use crate::memory::MemoryStore;
use std::sync::Arc;
use tokio::sync::mpsc;

fn extract_tag(desc: &str, tag: &str) -> Option<String> {
    if let Some(start_idx) = desc.find(tag) {
        let after_tag = &desc[start_idx + tag.len()..];
        if after_tag.starts_with('[')
            && let Some(end_idx) = after_tag.find(']') {
                return Some(after_tag[1..end_idx].trim().to_string());
            }
    }
    None
}

pub async fn execute_manage_routine(
    task_id: String,
    description: String,
    memory: Arc<MemoryStore>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("📋 Manage Routine Drone executing...\n".to_string()).await;
    }
    tracing::debug!("[AGENT:routines] ▶ task_id={}", task_id);

    let action = extract_tag(&description, "action:").unwrap_or("list".to_string());
    let routine_name = extract_tag(&description, "name:").unwrap_or("".to_string());
    let scope_str = extract_tag(&description, "scope:").unwrap_or("public_general".to_string());
    
    // Safety check against path traversal
    if routine_name.contains("..") || routine_name.contains('/') {
        return ToolResult { task_id, output: "Error: Invalid routine name.".into(), tokens_used: 0, status: ToolStatus::Failed("Path traversal".into()) };
    }

    // Double Scoping Storage path
    let mut routines_dir = memory.working.get_memory_dir();
    if scope_str.starts_with("public_") {
        routines_dir.push(format!("public_{}", scope_str.replace("public_", "")));
        routines_dir.push("system"); // System or active user mapping
    } else {
        routines_dir.push(format!("private_{}", scope_str.replace("private_", "")));
    }
    routines_dir.push("routines");

    let _ = tokio::fs::create_dir_all(&routines_dir).await;

    let output = match action.as_str() {
        "list" => {
            let mut entries = vec![];
            if let Ok(mut rd) = tokio::fs::read_dir(&routines_dir).await {
                while let Ok(Some(entry)) = rd.next_entry().await {
                    if let Ok(name) = entry.file_name().into_string()
                        && name.ends_with(".md") {
                            entries.push(name);
                        }
                }
            }
            if entries.is_empty() {
                "No routines found for this scope.".into()
            } else {
                format!("Available Routines:\n- {}", entries.join("\n- "))
            }
        }
        "create" => {
            if routine_name.is_empty() || !routine_name.ends_with(".md") {
                "Error: Routine name must end with .md".into()
            } else {
                let content = if let Some(idx) = description.find("content:[") {
                    let mut end = description.len();
                    if description.ends_with("]") {
                        end -= 1;
                    }
                    description[idx + 9..end].trim().to_string()
                } else {
                    return ToolResult { task_id, output: "Error: Missing content.".into(), tokens_used: 0, status: ToolStatus::Failed("No content".into()) };
                };
                
                let target_path = routines_dir.join(&routine_name);
                match tokio::fs::write(&target_path, content).await {
                    Ok(_) => format!("Successfully created routine: {}", routine_name),
                    Err(e) => format!("Failed to create routine: {}", e)
                }
            }
        }
        "read" => {
            if routine_name.is_empty() {
                "Error: Must specify name:[routine.md]".into()
            } else {
                let target_path = routines_dir.join(&routine_name);
                if !target_path.exists() {
                    format!("Error: Routine '{}' does not exist in the current scope.", routine_name)
                } else {
                    match tokio::fs::read_to_string(&target_path).await {
                        Ok(data) => format!("--- ROUTINE: {} ---\n{}", routine_name, data),
                        Err(e) => format!("Failed to read routine: {}", e)
                    }
                }
            }
        }
        _ => format!("Unknown action '{}'. Use create, list, or read.", action)
    };

    ToolResult { task_id, output, tokens_used: 0, status: ToolStatus::Success }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tag() {
        assert_eq!(extract_tag("name:[hello.md]", "name:"), Some("hello.md".into()));
        assert_eq!(extract_tag("action:[read] scope:[private_userX]", "action:"), Some("read".into()));
        assert_eq!(extract_tag("action:[read] scope:[private_userX]", "scope:"), Some("private_userX".into()));
        assert_eq!(extract_tag("invalid", "name:"), None);
    }

    #[tokio::test]
    async fn test_routines_tool_execute() {
        let mem = Arc::new(MemoryStore::default());
        
        // Test Traversal Protection
        let res = execute_manage_routine("1".into(), "name:[../etc/passwd]".into(), mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Failed("Path traversal".into()));

        // Test Create Error - Missing .md
        let res = execute_manage_routine("2".into(), "action:[create] name:[test]".into(), mem.clone(), None).await;
        assert!(res.output.contains("must end with .md"));

        // Test Create Error - Missing content
        let res = execute_manage_routine("3".into(), "action:[create] name:[test.md]".into(), mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Failed("No content".into()));

        // Test Create Success
        let res = execute_manage_routine("4".into(), "action:[create] name:[my_rule.md] content:[Wash hand]".into(), mem.clone(), None).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(res.output.contains("Successfully created"));

        // Test List
        let res = execute_manage_routine("5".into(), "action:[list]".into(), mem.clone(), None).await;
        assert!(res.output.contains("my_rule.md"));

        // Test Read
        let res = execute_manage_routine("6".into(), "action:[read] name:[my_rule.md]".into(), mem.clone(), None).await;
        assert!(res.output.contains("Wash hand"));

        // Test Read Missing
        let res = execute_manage_routine("7".into(), "action:[read] name:[ghost.md]".into(), mem.clone(), None).await;
        assert!(res.output.contains("does not exist"));

        // Test Unknown Action
        let res = execute_manage_routine("8".into(), "action:[fly]".into(), mem.clone(), None).await;
        assert!(res.output.contains("Unknown action"));
    }
}
