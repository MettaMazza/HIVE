use crate::models::tool::{ToolResult, ToolStatus};
use crate::models::scope::Scope;
use crate::memory::MemoryStore;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::io::{AsyncBufReadExt, BufReader};

pub async fn execute_review_reasoning(
    task_id: String,
    desc: String,
    _memory: Arc<MemoryStore>,
    scope: Scope,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("🧠 Native Reasoning Review Tool executing...\n")).await;
    }
    tracing::debug!("[AGENT:reasoning] ▶ task_id={}", task_id);

    // Parse parameters
    let mut limit: usize = 5;
    if let Some(turns_str) = desc.split("turns_ago:[").nth(1) {
        if let Some(num_str) = turns_str.split("]").next() {
            if let Ok(num) = num_str.parse::<usize>() {
                limit = num;
            }
        }
    }
    if let Some(limit_str) = desc.split("limit:[").nth(1) {
        if let Some(num_str) = limit_str.split("]").next() {
            if let Ok(num) = num_str.parse::<usize>() {
                limit = num;
            }
        }
    }

    // Resolve the timeline file path from the current scope
    let timeline_path = match &scope {
        Scope::Public { channel_id, user_id } => {
            std::path::PathBuf::from(format!("memory/public_{}/{}/timeline.jsonl", channel_id, user_id))
        }
        Scope::Private { user_id } => {
            std::path::PathBuf::from(format!("memory/private_{}/timeline.jsonl", user_id))
        }
    };

    // Read the persistent timeline file and extract only internal reasoning traces
    let file = match tokio::fs::File::open(&timeline_path).await {
        Ok(f) => f,
        Err(_) => {
            return ToolResult {
                task_id,
                output: "No persistent timeline found — no reasoning traces available.".to_string(),
                tokens_used: 0,
                status: ToolStatus::Success,
            };
        }
    };

    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut all_traces = Vec::new();

    while let Ok(Some(line)) = lines.next_line().await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
            if json["author_id"].as_str() == Some("internal") {
                if let Some(content) = json["content"].as_str() {
                    all_traces.push(content.to_string());
                }
            }
        }
    }

    if all_traces.is_empty() {
        return ToolResult {
            task_id,
            output: "No reasoning traces found in the persistent timeline.".to_string(),
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    // Return the most recent N traces (from the end of the file)
    let start = if all_traces.len() > limit { all_traces.len() - limit } else { 0 };
    let slice = &all_traces[start..];

    let mut out = String::new();
    for (i, trace) in slice.iter().enumerate() {
        out.push_str(&format!("--- REASONING TRACE {} of {} ---\n{}\n\n", start + i + 1, all_traces.len(), trace));
    }

    ToolResult {
        task_id,
        output: out,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}
