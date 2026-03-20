use crate::models::tool::{ToolResult, ToolStatus};
use crate::models::message::Event;
use crate::providers::Provider;
use crate::models::scope::Scope;
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn execute_synthesizer(
    task_id: String,
    description: String,
    context: String,
    provider: Arc<dyn Provider>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx
            .send(format!("🔀 Synthesizer Drone: '{}'\n", description.trim()))
            .await;
    }
    tracing::debug!("[AGENT:synthesizer] ▶ task_id={}", task_id);

    let system_prompt = format!(
        "You are the Synthesizer Drone. Your job is to read the drone outputs provided \
         in the context block below and produce a single, compact synthesis.\n\
         Be concise. Do not pad. Output only the synthesis — no preamble, no labels.\n\n\
         [SYNTHESIS INSTRUCTION]: {}\n\n\
         [DRONE OUTPUTS TO SYNTHESISE]:\n{}",
        description.trim(),
        context
    );

    let dummy_event = Event {
        platform: "swarm".into(),
        scope: Scope::Private {
            user_id: "synthesizer".into(),
        },
        author_name: "Synthesizer".into(),
        author_id: "system".into(),
        content: description.clone(),
    };

    match provider
        .generate(&system_prompt, &[], &dummy_event, "", telemetry_tx, None)
        .await
    {
        Ok(output) => {
            ToolResult {
                task_id,
                output,
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => ToolResult {
            task_id,
            output: String::new(),
            tokens_used: 0,
            status: ToolStatus::Failed(format!("Synthesizer failed: {:?}", e)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::MockProvider;

    #[tokio::test]
    async fn test_execute_synthesizer_success() {
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_, _, _, _, _, _| Ok("Synthesized Output".to_string()));

        let provider = Arc::new(mock_provider);
        let res = execute_synthesizer("1".into(), "Summarize".into(), "Some context".into(), provider, None).await;

        assert_eq!(res.status, ToolStatus::Success);
        assert_eq!(res.output, "Synthesized Output");
    }

    #[tokio::test]
    async fn test_execute_synthesizer_failure() {
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_, _, _, _, _, _| Err(crate::providers::ProviderError::ConnectionError("Timeout".into())));

        let provider = Arc::new(mock_provider);
        let res = execute_synthesizer("1".into(), "Summarize".into(), "Some context".into(), provider, None).await;

        match res.status {
            ToolStatus::Failed(err) => assert!(err.contains("Synthesizer failed")),
            _ => panic!("Expected failure"),
        }
    }
}
