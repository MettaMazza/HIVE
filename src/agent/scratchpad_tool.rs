use crate::models::tool::{ToolResult, ToolStatus};
use crate::memory::MemoryStore;
use crate::models::scope::Scope;
use crate::agent::preferences::extract_tag;
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn execute_manage_scratchpad(
    task_id: String,
    description: String,
    memory: Arc<MemoryStore>,
    telemetry_tx: Option<mpsc::Sender<String>>,
    current_scope: &Scope,
    drives: Option<Arc<crate::engine::drives::DriveSystem>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("📝 Scratchpad Drone executing...\n".to_string()).await;
    }
    tracing::debug!("[AGENT:scratchpad] ▶ task_id={}", task_id);

    let action = extract_tag(&description, "action:").unwrap_or_default().to_lowercase();
    let content = extract_tag(&description, "content:").unwrap_or_default();

    if action.is_empty() {
        return ToolResult { 
            task_id, 
            output: "Error: Missing 'action:' field.".to_string(), 
            tokens_used: 0, 
            status: ToolStatus::Failed("Missing action field".into()) 
        };
    }

    match action.as_str() {
        "read" => {
            let data = memory.scratch.read(current_scope).await;
            if data.trim().is_empty() {
                ToolResult { task_id, output: "Scratchpad is completely empty.".to_string(), tokens_used: 0, status: ToolStatus::Success }
            } else {
                ToolResult { task_id, output: format!("[SCRATCHPAD CONTENTS]\n\n{}", data), tokens_used: 0, status: ToolStatus::Success }
            }
        }
        "write" => {
            if content.is_empty() {
                return ToolResult { task_id, output: "Error: Missing 'content:' field for write action.".to_string(), tokens_used: 0, status: ToolStatus::Failed("Missing field".into()) };
            }
            match memory.scratch.write(current_scope, &content).await {
                Ok(_) => {
                    if let Some(ref d) = drives {
                        d.modify_drive("uncertainty", -5.0).await;
                    }
                    ToolResult { task_id, output: "Scratchpad overwritten successfully.".to_string(), tokens_used: 0, status: ToolStatus::Success }
                },
                Err(e) => ToolResult { task_id, output: format!("Failed to write: {}", e), tokens_used: 0, status: ToolStatus::Failed(e.to_string()) },
            }
        }
        "append" => {
            if content.is_empty() {
                return ToolResult { task_id, output: "Error: Missing 'content:' field for append action.".to_string(), tokens_used: 0, status: ToolStatus::Failed("Missing field".into()) };
            }
            match memory.scratch.append(current_scope, &content).await {
                Ok(_) => {
                    if let Some(ref d) = drives {
                        d.modify_drive("uncertainty", -5.0).await;
                    }
                    ToolResult { task_id, output: "Scratchpad appended successfully.".to_string(), tokens_used: 0, status: ToolStatus::Success }
                },
                Err(e) => ToolResult { task_id, output: format!("Failed to append: {}", e), tokens_used: 0, status: ToolStatus::Failed(e.to_string()) },
            }
        }
        "clear" => {
            match memory.scratch.clear(current_scope).await {
                Ok(_) => ToolResult { task_id, output: "Scratchpad completely cleared.".to_string(), tokens_used: 0, status: ToolStatus::Success },
                Err(e) => ToolResult { task_id, output: format!("Failed to clear: {}", e), tokens_used: 0, status: ToolStatus::Failed(e.to_string()) },
            }
        }
        _ => ToolResult {
            task_id,
            output: format!("Unknown action '{}'. Valid actions: read, write, append, clear.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_manage_scratchpad() {
        let mem = Arc::new(MemoryStore::default());
        let scope = Scope::Private { user_id: "test_sp_user".into() };

        // Test missing action
        let res = execute_manage_scratchpad("1".into(), "content:[hello]".into(), mem.clone(), None, &scope, None).await;
        assert_eq!(res.status, ToolStatus::Failed("Missing action field".into()));

        // Test unknown action
        let res = execute_manage_scratchpad("1".into(), "action:[foo] content:[hello]".into(), mem.clone(), None, &scope, None).await;
        assert_eq!(res.status, ToolStatus::Failed("Unknown action".into()));

        // Test empty read
        let res = execute_manage_scratchpad("2".into(), "action:[read]".into(), mem.clone(), None, &scope, None).await;
        assert!(res.output.contains("completely empty"));

        // Test write missing content
        let res = execute_manage_scratchpad("3".into(), "action:[write]".into(), mem.clone(), None, &scope, None).await;
        assert_eq!(res.status, ToolStatus::Failed("Missing field".into()));

        // Test write
        let res = execute_manage_scratchpad("4".into(), "action:[write] content:[Hello World]".into(), mem.clone(), None, &scope, None).await;
        assert_eq!(res.status, ToolStatus::Success);

        // Test read after write
        let res = execute_manage_scratchpad("5".into(), "action:[read]".into(), mem.clone(), None, &scope, None).await;
        assert!(res.output.contains("Hello World"));

        // Test append
        let res = execute_manage_scratchpad("6".into(), "action:[append] content:[... Goodbye!]".into(), mem.clone(), None, &scope, None).await;
        assert_eq!(res.status, ToolStatus::Success);

        // Test read after append
        let res = execute_manage_scratchpad("7".into(), "action:[read]".into(), mem.clone(), None, &scope, None).await;
        assert!(res.output.contains("Hello World... Goodbye!"));

        // Test clear
        let res = execute_manage_scratchpad("8".into(), "action:[clear]".into(), mem.clone(), None, &scope, None).await;
        assert_eq!(res.status, ToolStatus::Success);

        // Test read after clear
        let res = execute_manage_scratchpad("9".into(), "action:[read]".into(), mem.clone(), None, &scope, None).await;
        assert!(res.output.contains("completely empty"));
    }
}
