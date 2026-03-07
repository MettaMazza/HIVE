use std::sync::Arc;
use crate::models::drone::{DroneTemplate, DroneResult, DroneStatus};
use crate::providers::Provider;
use crate::models::message::Event;
use crate::models::scope::Scope;

pub struct DroneExecutor {
    pub provider: Arc<dyn Provider>,
    pub template: DroneTemplate,
}

#[cfg(not(tarpaulin_include))]
impl DroneExecutor {
    pub fn new(provider: Arc<dyn Provider>, template: DroneTemplate) -> Self {
        Self { provider, template }
    }

    pub async fn execute(&self, task_id: &str, task_description: &str, context: &str, telemetry_tx: Option<tokio::sync::mpsc::Sender<String>>) -> DroneResult {
        let system_prompt = format!(
            "{}\n\n[CONTEXT PROVIDED BY QUEEN]\n{}\n\n[YOUR TASK]\n{}",
            self.template.system_prompt,
            context,
            task_description
        );

        let dummy_event = Event {
            platform: "swarm".into(),
            scope: Scope::Private { user_id: "drone".into() },
            author_name: "Queen".into(),
            author_id: "test".into(),
            content: "Execute task.".into(),
        };

        // We use the shared provider for execution, but could swap model if template.model_override is set.
        let result = self.provider.generate(&system_prompt, &[], &dummy_event, telemetry_tx).await;

        match result {
            Ok(output) => {
                // In the future we will count tokens, for now mock it to 0
                DroneResult {
                    task_id: task_id.to_string(),
                    output,
                    tokens_used: 0, 
                    status: DroneStatus::Success,
                }
            }
            Err(e) => {
                DroneResult {
                    task_id: task_id.to_string(),
                    output: String::new(),
                    tokens_used: 0,
                    status: DroneStatus::Failed(format!("Drone iteration failed: {:?}", e)),
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
    async fn test_drone_execute_success() {
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|sys, _, _, _| {
                assert!(sys.contains("You are a test drone"));
                assert!(sys.contains("Context"));
                assert!(sys.contains("Task"));
                Ok("Success output".to_string())
            });

        let template = DroneTemplate {
            name: "test".into(),
            system_prompt: "You are a test drone".into(),
            tools: vec![],
        };

        let executor = DroneExecutor::new(Arc::new(mock_provider), template);
        let result = executor.execute("task_1", "Task", "Context", None).await;

        assert_eq!(result.task_id, "task_1");
        assert_eq!(result.output, "Success output");
        assert_eq!(result.status, DroneStatus::Success);
    }

    #[tokio::test]
    async fn test_drone_execute_failure() {
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_, _, _, _| Err(ProviderError::ConnectionError("Boom".into())));

        let template = DroneTemplate {
            name: "test".into(),
            system_prompt: "sys".into(),
            tools: vec![],
        };

        let executor = DroneExecutor::new(Arc::new(mock_provider), template);
        let result = executor.execute("task_2", "Task desc", "Ctx", None).await;

        assert_eq!(result.task_id, "task_2");
        assert!(matches!(result.status, DroneStatus::Failed(_)));
    }
}
