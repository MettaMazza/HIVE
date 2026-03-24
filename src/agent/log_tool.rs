use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;

pub async fn execute_read_logs(
    task_id: String,
    desc: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("🧠 Native Log Reader Tool executing...\n")).await;
    }
    tracing::debug!("[AGENT:log_reader] ▶ task_id={}", task_id);
    
    let mut lines_to_read = 50;
    if let Some(lines_str) = desc.split("lines:[").nth(1)
        && let Some(num_str) = lines_str.split("]").next()
            && let Ok(num) = num_str.parse::<usize>() {
                lines_to_read = num;
            }

    // Find the latest rotating log file (hive.YYYY-MM-DD.log)
    let log_path = {
        let mut latest: Option<(String, std::path::PathBuf)> = None;
        if let Ok(mut entries) = tokio::fs::read_dir("logs").await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("hive.") && name.ends_with(".log") && name != "hive.log"
                    && latest.as_ref().is_none_or(|(prev, _)| name > *prev) {
                        latest = Some((name, entry.path()));
                    }
            }
        }
        latest.map(|(_, p)| p).unwrap_or_else(|| std::path::PathBuf::from("logs/hive.log"))
    };
    tracing::debug!("[AGENT:log_reader] Reading from: {}", log_path.display());

    match tokio::fs::read_to_string(&log_path).await {
        Ok(content) => {
            let mut lines: Vec<&str> = content.lines().collect();
            
            let regex_pattern = desc.split("regex:[").nth(1).and_then(|s| s.split("]").next());
            if let Some(pat) = regex_pattern {
                if let Ok(re) = regex::Regex::new(pat) {
                    lines = lines.into_iter().filter(|&l| re.is_match(l)).collect();
                } else {
                    return ToolResult { task_id, output: format!("Invalid regex pattern: {}", pat), tokens_used: 0, status: ToolStatus::Failed("Bad Regex".into()) };
                }
            }
            
            let len = lines.len();
            let start = len.saturating_sub(lines_to_read);
            let tail = &lines[start..];
            let output = tail.join("\n");
            
            ToolResult {
                task_id,
                output: if output.is_empty() { 
                    "Log file is empty.".to_string() 
                } else { 
                    format!("{}\n\n[LOGS COMPLETE (Tailed {} lines from {})]\n", output, lines.len() - start, log_path.display()) 
                },
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => {
            ToolResult {
                task_id,
                output: format!("Failed to read logs from {}: {}", log_path.display(), e),
                tokens_used: 0,
                status: ToolStatus::Failed(e.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_no_log_file() {
        let r = execute_read_logs("1".into(), "".into(), None).await;
        // Should return either empty or error gracefully
        assert!(r.output.len() > 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_with_lines_param() {
        let r = execute_read_logs("1".into(), "lines:[10]".into(), None).await;
        assert!(r.output.len() > 0);
    }
}
