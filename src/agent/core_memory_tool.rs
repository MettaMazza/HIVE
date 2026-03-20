use crate::models::tool::{ToolResult, ToolStatus};
use crate::memory::MemoryStore;
use crate::models::scope::Scope;
use crate::agent::preferences::extract_tag;
use std::sync::Arc;
use tokio::sync::mpsc;
use chrono::Utc;

pub async fn execute_read_core_memory(
    task_id: String,
    description: String,
    memory: Arc<MemoryStore>,
    telemetry_tx: Option<mpsc::Sender<String>>,
    current_scope: &Scope,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("⏱️ Core Memory Drone executing...\n".to_string()).await;
    }
    tracing::debug!("[AGENT:core_memory] ▶ task_id={}", task_id);

    let action = extract_tag(&description, "action:").unwrap_or_default().to_lowercase();

    if action.is_empty() {
        return ToolResult { 
            task_id, 
            output: "Error: Missing 'action:' field.".to_string(), 
            tokens_used: 0, 
            status: ToolStatus::Failed("Missing action field".into()) 
        };
    }

    match action.as_str() {
        "temporal" => {
            let temp = memory.temporal.read().await;
            
            let now = Utc::now();
            let session_uptime = (now - temp.uptime_start).num_seconds() as f64;
            let system_uptime = temp.state.total_uptime_seconds + session_uptime;
            
            let hours = (system_uptime / 3600.0).floor();
            let minutes = ((system_uptime % 3600.0) / 60.0).floor();
            
            let sess_hours = (session_uptime / 3600.0).floor();
            let sess_minutes = ((session_uptime % 3600.0) / 60.0).floor();

            let last_boot = temp.state.last_boot.clone().unwrap_or_else(|| "Unknown".to_string());

            let report = format!(
                "[TEMPORAL AWARENESS REPORT]\n\
                Last System Boot: {}\n\
                Total Cumulative Uptime: {}h {}m\n\
                Current Session Awake Time: {}h {}m\n\
                Total Boot Count: {}",
                last_boot,
                hours, minutes,
                sess_hours, sess_minutes,
                temp.state.total_boots
            );

            ToolResult { task_id, output: report, tokens_used: 0, status: ToolStatus::Success }
        }
        "tokens" => {
            let tokens = memory.working.current_tokens().await;
            let history_len = memory.working.get_history(current_scope).await.len();
            
            // Assume the standard context limit is roughly 120_000 for standard processing 
            // before the rolling window evicts things aggressively.
            let pressure_pct = (tokens as f64 / 120_000.0) * 100.0;

            let report = format!(
                "[WORKING CONTEXT WINDOW REPORT]\n\
                Active Messages in Window: {}\n\
                Estimated Tokens Utilized: ~{}\n\
                Context Pressure: {:.2}%\n\n\
                If pressure approaches 100%, older messages are being rolling-evicted to the persistent timeline.",
                history_len, tokens, pressure_pct
            );

            ToolResult { task_id, output: report, tokens_used: 0, status: ToolStatus::Success }
        }
        _ => ToolResult {
            task_id,
            output: format!("Unknown action '{}'. Valid actions: temporal, tokens.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_read_core_memory() {
        let mem = Arc::new(MemoryStore::default());
        let scope = Scope::Private { user_id: "test".into() };

        // Test missing action
        let res = execute_read_core_memory("1".into(), "".into(), mem.clone(), None, &scope).await;
        assert_eq!(res.status, ToolStatus::Failed("Missing action field".into()));

        // Test temporal
        let res = execute_read_core_memory("2".into(), "action:[temporal]".into(), mem.clone(), None, &scope).await;
        assert_eq!(res.status, ToolStatus::Success);
        assert!(res.output.contains("[TEMPORAL AWARENESS REPORT]"));
        assert!(res.output.contains("Last System Boot: "));
        
        // Test tokens
        let res = execute_read_core_memory("3".into(), "action:[tokens]".into(), mem.clone(), None, &scope).await;
        assert_eq!(res.status, ToolStatus::Success);
        assert!(res.output.contains("[WORKING CONTEXT WINDOW REPORT]"));
        assert!(res.output.contains("Active Messages in Window: 0")); // empty initially
    }
}
