use crate::models::tool::{ToolResult, ToolStatus};
use crate::models::scope::Scope;
use crate::memory::MemoryStore;
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn execute_review_reasoning(
    task_id: String,
    desc: String,
    memory: Arc<MemoryStore>,
    scope: Scope,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("🧠 Native Reasoning Review Tool executing...\n")).await;
    }
    tracing::debug!("[AGENT:reasoning] ▶ task_id={}", task_id);

    let mut turns_ago = 5;
    if let Some(turns_str) = desc.split("turns_ago:[").nth(1)
        && let Some(num_str) = turns_str.split("]").next()
            && let Ok(num) = num_str.parse::<usize>() {
                turns_ago = num;
            }

    let history = memory.working.get_history(&scope).await;
    let mut extracted = Vec::new();
    for event in history.iter().rev() {
        if event.author_name == "Apis (Internal Timeline)" {
            extracted.push(event.content.clone());
        }
    }

    if extracted.is_empty() {
        return ToolResult {
            task_id,
            output: "No reasoning traces found in active memory.".to_string(),
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    let start_idx = if turns_ago >= extracted.len() { extracted.len() - 1 } else { turns_ago };
    
    let slice = if start_idx + 5 <= extracted.len() {
        &extracted[start_idx..start_idx + 5]
    } else {
        &extracted[start_idx..]
    };

    let mut out = String::new();
    for (i, trace) in slice.iter().enumerate() {
        out.push_str(&format!("--- TRACE {} TURNS AGO ---\n{}\n\n", start_idx + i, trace));
    }

    ToolResult {
        task_id,
        output: out,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}
