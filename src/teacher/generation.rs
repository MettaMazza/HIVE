use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use tokio::io::AsyncWriteExt;
use crate::models::message::Event;
use crate::models::scope::Scope;
use crate::teacher::Teacher;

/// A single golden example: first-pass Observer approval, perfect interaction.
#[derive(Debug, Serialize, Deserialize)]
pub struct GoldenExample {
    pub ts: String,
    pub system_prompt: String,
    pub user_msg: String,
    pub agent_ctx: String,
    pub response: String,
    pub tools: Vec<String>,
    pub attempts: usize,
}

impl Teacher {
    /// Capture a golden example (first-pass Observer approval). Public scope only.
    pub async fn capture_golden(
        &self,
        system_prompt: &str,
        event: &Event,
        agent_ctx: &str,
        response: &str,
    ) {
        // Privacy guard: never train on DM content
        if matches!(event.scope, Scope::Private { .. }) {
            return;
        }

        let example = GoldenExample {
            ts: chrono::Utc::now().to_rfc3339(),
            system_prompt: system_prompt.to_string(),
            user_msg: event.content.clone(),
            agent_ctx: agent_ctx.to_string(),
            response: response.to_string(),
            tools: Self::extract_tools(agent_ctx),
            attempts: 1,
        };

        match serde_json::to_string(&example) {
            Ok(json) => {
                match tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.golden_path)
                    .await
                {
                    Ok(mut file) => {
                        if let Err(e) = file.write_all(format!("{}\n", json).as_bytes()).await {
                            tracing::error!("[TEACHER] ❌ Failed to write golden example to '{}': {}", self.golden_path.display(), e);
                        } else {
                            self.golden_count.fetch_add(1, Ordering::Relaxed);
                            tracing::info!("[TEACHER] 🏆 Golden example captured ({} buffered)", self.golden_count.load(Ordering::Relaxed));
                        }
                    }
                    Err(e) => tracing::error!("[TEACHER] ❌ Failed to open golden buffer '{}': {}", self.golden_path.display(), e),
                }
            }
            Err(e) => tracing::error!("[TEACHER] ❌ Failed to serialize golden example: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::scope::Scope;
    use crate::models::message::Event;
    use tempfile::TempDir;

    fn test_event(scope: Scope) -> Event {
        Event {
            platform: "test".into(),
            scope,
            author_name: "Tester".into(),
            author_id: "123".into(),
            content: "Hello Apis".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }
    }

    #[tokio::test]
    async fn test_capture_golden_public() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let event = test_event(Scope::Public {
            channel_id: "ch1".into(),
            user_id: "u1".into(),
        });

        teacher.capture_golden("system", &event, "agent ctx", "response").await;
        assert_eq!(teacher.get_counts(), (1, 0));

        let content: String = tokio::fs::read_to_string(tmp.path().join("golden_buffer.jsonl")).await.unwrap();
        assert!(content.contains("Hello Apis"));
        assert!(content.contains("response"));
    }

    #[tokio::test]
    async fn test_capture_golden_private_skipped() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let event = test_event(Scope::Private {
            user_id: "u1".into(),
        });

        teacher.capture_golden("system", &event, "ctx", "resp").await;
        assert_eq!(teacher.get_counts(), (0, 0));
    }

    #[tokio::test]
    async fn test_multiple_golden_captures() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let event = test_event(Scope::Public {
            channel_id: "ch1".into(),
            user_id: "u1".into(),
        });

        teacher.capture_golden("sys1", &event, "Tool: researcher", "resp1").await;
        teacher.capture_golden("sys2", &event, "ctx2", "resp2").await;
        teacher.capture_golden("sys3", &event, "ctx3", "resp3").await;
        assert_eq!(teacher.get_counts(), (3, 0));

        let content = tokio::fs::read_to_string(tmp.path().join("golden_buffer.jsonl")).await.unwrap();
        let lines: Vec<&str> = content.trim().split('\n').collect();
        assert_eq!(lines.len(), 3);
    }
}
