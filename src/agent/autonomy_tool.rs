use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;
use crate::engine::drives::DriveSystem;
use std::sync::Arc;

pub async fn execute_autonomy_activity(
    task_id: String,
    desc: String,
    drives: Option<Arc<DriveSystem>>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("🐝 Autonomy Activity Tool executing...\n".to_string()).await;
    }
    tracing::debug!("[AGENT:autonomy] ▶ task_id={}", task_id);

    let path = std::path::Path::new("memory/autonomy/activity.jsonl");
    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(_) => {
            return ToolResult {
                task_id,
                output: "No autonomous activity recorded yet. The autonomy log is empty.".to_string(),
                tokens_used: 0,
                status: ToolStatus::Success,
            };
        }
    };

    let entries: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

    if desc.contains("action:[summary]") {
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
        let mut session_count = 0;
        let mut total_turns = 0;
        let mut tools_used = std::collections::HashSet::new();
        let mut summaries = Vec::new();

        for line in &entries {
            if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(ts) = entry.get("timestamp").and_then(|v| v.as_str())
                    && let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts)
                        && dt < cutoff { continue; }
                session_count += 1;
                total_turns += entry.get("turn_count").and_then(|v| v.as_u64()).unwrap_or(0);
                if let Some(tools) = entry.get("tools_used").and_then(|v| v.as_array()) {
                    for t in tools {
                        if let Some(s) = t.as_str() { tools_used.insert(s.to_string()); }
                    }
                }
                if let Some(summary) = entry.get("summary").and_then(|v| v.as_str()) {
                    summaries.push(summary.to_string());
                }
            }
        }

        let output = format!(
            "📊 **Autonomy Summary (Last 24h)**\n• Sessions: {}\n• Total turns used: {}\n• Tools exercised: {}\n\n**Session Highlights:**\n{}",
            session_count,
            total_turns,
            tools_used.into_iter().collect::<Vec<_>>().join(", "),
            summaries.iter().enumerate().map(|(i, s)| format!("{}. {}", i + 1, s)).collect::<Vec<_>>().join("\n")
        );

        ToolResult {
            task_id,
            output: if session_count == 0 { "No autonomous activity in the last 24 hours.".to_string() } else { output },
            tokens_used: 0,
            status: ToolStatus::Success,
        }
    } else {
        let mut count = 10usize;
        if let Some(count_str) = desc.split("count:[").nth(1)
            && let Some(num_str) = count_str.split(']').next()
                && let Ok(n) = num_str.parse::<usize>() { count = n; }

        let start = if entries.len() > count { entries.len() - count } else { 0 };
        let recent = &entries[start..];
        let output = recent.join("\n");

        if recent.len() >= 3 && count > 0 && !desc.contains("action:[summary]") {
            let last_3: Vec<&str> = recent[recent.len()-3..].iter().map(|s| *s).collect();
            let mut summaries = vec![];
            for l in &last_3 {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(l) {
                    if let Some(s) = json.get("summary").and_then(|v| v.as_str()) {
                        summaries.push(s.to_string());
                    }
                }
            }
            if summaries.len() == 3 && summaries[0] == summaries[1] && summaries[1] == summaries[2] {
                if let Some(d) = drives {
                    d.modify_drive("uncertainty", 20.0).await;
                    if let Some(ref tx) = telemetry_tx {
                        let _ = tx.send("🚨 LOOP DETECTED. Uncertainty spiked by +20.0 to break hallucination cycle.\n".to_string()).await;
                    }
                }
            }
        }

        ToolResult {
            task_id,
            output: if output.is_empty() { "No autonomous activity recorded yet.".to_string() } else { output },
            tokens_used: 0,
            status: ToolStatus::Success,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_no_file() {
        // Run the tool
        let r = execute_autonomy_activity("1".into(), "".into(), None, None).await;
        assert_eq!(r.status, ToolStatus::Success);
        
        // If the file actually exists on the machine running tests, it will return lines instead of the empty message
        if !std::path::Path::new("memory/autonomy/activity.jsonl").exists() {
            assert!(r.output.contains("No autonomous activity"));
        } else {
            assert!(!r.output.is_empty());
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_summary_no_entries() {
        let r = execute_autonomy_activity("1".into(), "action:[summary]".into(), None, None).await;
        assert_eq!(r.status, ToolStatus::Success);
    }
}
