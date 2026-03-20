use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;

pub async fn execute_read_attachment(
    task_id: String,
    desc: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("📎 Fetching attachment (in-memory, no disk write)...\n".to_string()).await;
    }
    tracing::debug!("[AGENT:attachment] ▶ task_id={}", task_id);

    let url = crate::agent::preferences::extract_tag(&desc, "url:")
        .unwrap_or_else(|| {
            desc.split_whitespace()
                .find(|s| s.starts_with("http"))
                .map(|s| s.trim_matches(|c| c == '\'' || c == '"' || c == '`' || c == '(' || c == ')' || c == ']').to_string())
                .unwrap_or_default()
        });

    if url.is_empty() || !url.starts_with("http") {
        return ToolResult {
            task_id,
            output: "Error: No valid URL provided. Use url:[https://cdn.discordapp.com/...]".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing or invalid URL".into()),
        };
    }

    let allowed_domains = ["cdn.discordapp.com", "media.discordapp.net"];
    let is_allowed = allowed_domains.iter().any(|d| url.contains(d));
    if !is_allowed {
        return ToolResult {
            task_id,
            output: "Access Denied: read_attachment only accepts Discord CDN URLs.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Security: non-Discord URL".into()),
        };
    }

    const MAX_SIZE: usize = 10 * 1024 * 1024;
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
        
    match client.get(&url).send().await {
        Ok(resp) => {
            match resp.bytes().await {
                Ok(bytes) => {
                    let size = bytes.len();
                    if size > MAX_SIZE {
                        return ToolResult {
                            task_id,
                            output: format!("Rejected: file is {} bytes, exceeds 10MB safety limit.", size),
                            tokens_used: 0,
                            status: ToolStatus::Failed("File too large".into()),
                        };
                    }
                    if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                        ToolResult {
                            task_id,
                            output: text, // Return full content — 10MB hard limit is enforced above
                            tokens_used: 0,
                            status: ToolStatus::Success,
                        }
                    } else {
                        ToolResult {
                            task_id,
                            output: format!("Binary file ({} bytes). Cannot display as text. The file was inspected in-memory but contains non-UTF8 binary data.", size),
                            tokens_used: 0,
                            status: ToolStatus::Success,
                        }
                    }
                }
                Err(e) => ToolResult { task_id, output: format!("Failed to read response bytes: {}", e), tokens_used: 0, status: ToolStatus::Failed(e.to_string()) },
            }
        }
        Err(e) => ToolResult { task_id, output: format!("Failed to fetch attachment: {}. The CDN URL may have expired.", e), tokens_used: 0, status: ToolStatus::Failed(e.to_string()) },
    }
}
