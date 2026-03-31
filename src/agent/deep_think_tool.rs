use crate::models::tool::{ToolResult, ToolStatus};
use crate::providers::ollama::OllamaProvider;
use crate::providers::Provider;
use tokio::sync::mpsc;

/// Deep Think — on-demand access to the large reasoning model.
///
/// Apis runs on 35b for speed. When it hits something that needs
/// heavier reasoning (complex code, math, architecture, deep analysis),
/// it invokes this tool to ask the 122b model a focused question and
/// gets back a considered answer.
///
/// Usage: `deep_think` with description = the question/problem to reason about.
/// The tool sends it to HIVE_DEEP_MODEL (default: qwen3.5:122b) and returns the response.
pub async fn execute_deep_think(
    task_id: String,
    desc: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("🧠 Deep Think — routing to large model...\n".to_string()).await;
    }

    if desc.trim().is_empty() {
        return ToolResult {
            task_id,
            output: "Error: No question provided. Pass the problem to reason about in the description.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Empty input".into()),
        };
    }

    let model = std::env::var("HIVE_DEEP_MODEL")
        .unwrap_or_else(|_| "qwen3.5:122b".into());

    tracing::info!("[DEEP_THINK] 🧠 Sending to {} ({} chars)", model, desc.len());

    let provider = OllamaProvider::with_model(&model);

    let system_prompt = "You are a deep reasoning assistant. Think carefully and thoroughly about the problem presented. Provide a clear, well-structured analysis. Be precise and thorough.";

    let event = crate::models::message::Event {
        platform: "system:deep_think".into(),
        scope: crate::models::scope::Scope::Private { user_id: "deep_think".into() },
        author_name: "Apis".into(),
        author_id: "apis_deep_think".into(),
        content: desc.clone(),
        timestamp: Some(chrono::Utc::now().to_rfc3339()),
        message_index: None,
    };

    let start = tokio::time::Instant::now();

    match provider.generate(
        system_prompt,
        &[],
        &event,
        "",
        telemetry_tx.clone(),
        Some(4096),
    ).await {
        Ok(response) => {
            let elapsed = start.elapsed();
            tracing::info!("[DEEP_THINK] ✅ Response received in {:.1}s ({} chars)",
                elapsed.as_secs_f64(), response.len());

            if let Some(ref tx) = telemetry_tx {
                let _ = tx.send(format!("🧠 Deep Think complete ({:.1}s)\n", elapsed.as_secs_f64())).await;
            }

            ToolResult {
                task_id,
                output: response,
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => {
            tracing::error!("[DEEP_THINK] ❌ Failed: {}", e);
            ToolResult {
                task_id,
                output: format!("Deep Think failed: {}", e),
                tokens_used: 0,
                status: ToolStatus::Failed(format!("{}", e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_empty_input() {
        let r = execute_deep_think("1".into(), "".into(), None).await;
        assert_eq!(r.status, ToolStatus::Failed("Empty input".into()));
    }
}
