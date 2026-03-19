use std::sync::Arc;
use crate::models::tool::{ToolTemplate, ToolResult, ToolStatus};
use crate::providers::Provider;
use crate::models::message::Event;
use crate::models::scope::Scope;

pub struct ToolExecutor {
    pub provider: Arc<dyn Provider>,
    pub template: ToolTemplate,
}

#[cfg(not(tarpaulin_include))]
impl ToolExecutor {
    pub fn new(provider: Arc<dyn Provider>, template: ToolTemplate) -> Self {
        Self { provider, template }
    }

    pub async fn execute(&self, task_id: &str, task_description: &str, context: &str, telemetry_tx: Option<tokio::sync::mpsc::Sender<String>>) -> ToolResult {
        tracing::debug!("[AGENT:ToolExecutor] ▶ Executing template='{}' task_id='{}' desc_len={}",
            self.template.name, task_id, task_description.len());
        let system_prompt = format!(
            "{}\n\n[CONTEXT PROVIDED BY QUEEN]\n{}\n\n[YOUR TASK]\n{}",
            self.template.system_prompt,
            context,
            task_description
        );

        let dummy_event = Event {
            platform: "agent".into(),
            scope: Scope::Private { user_id: "tool".into() },
            author_name: "Planner".into(),
            author_id: "test".into(),
            content: "Execute task.".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        // We use the shared provider for execution, but could swap model if template.model_override is set.
        let result = self.provider.generate(&system_prompt, &[], &dummy_event, "", telemetry_tx, None).await;

        match result {
            Ok(output) => {
                tracing::debug!("[AGENT:ToolExecutor] ◀ task_id='{}' template='{}' status=Success output_len={}",
                    task_id, self.template.name, output.len());
                // In the future we will count tokens, for now mock it to 0
                ToolResult {
                    task_id: task_id.to_string(),
                    output,
                    tokens_used: 0, 
                    status: ToolStatus::Success,
                }
            }
            Err(e) => {
                tracing::error!("[AGENT:ToolExecutor] ❌ task_id='{}' template='{}' error={:?}",
                    task_id, self.template.name, e);
                ToolResult {
                    task_id: task_id.to_string(),
                    output: String::new(),
                    tokens_used: 0,
                    status: ToolStatus::Failed(format!("Tool iteration failed: {:?}", e)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{MockProvider, ProviderError};

    #[tokio::test]
    async fn test_tool_execute_success() {
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|sys, _, _, _ctx, _, _| {
                assert!(sys.contains("You are a test tool"));
                assert!(sys.contains("Context"));
                assert!(sys.contains("Task"));
                Ok("Success output".to_string())
            });

        let template = ToolTemplate {
            name: "test".into(),
            system_prompt: "You are a test tool".into(),
            tools: vec![],
        };

        let executor = ToolExecutor::new(Arc::new(mock_provider), template);
        let result = executor.execute("task_1", "Task", "Context", None).await;

        assert_eq!(result.task_id, "task_1");
        assert_eq!(result.output, "Success output");
        assert_eq!(result.status, ToolStatus::Success);
    }

    #[tokio::test]
    async fn test_tool_execute_failure() {
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_, _, _, _, _, _| Err(ProviderError::ConnectionError("Boom".into())));

        let template = ToolTemplate {
            name: "test".into(),
            system_prompt: "sys".into(),
            tools: vec![],
        };

        let executor = ToolExecutor::new(Arc::new(mock_provider), template);
        let result = executor.execute("task_2", "Task desc", "Ctx", None).await;

        assert_eq!(result.task_id, "task_2");
        assert!(matches!(result.status, ToolStatus::Failed(_)));
    }
}
