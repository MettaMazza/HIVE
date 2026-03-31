use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::models::message::Event;
use super::{Provider, ProviderError};
use super::openai::OpenAiProvider;

/// xAI (Grok) provider — uses the OpenAI-compatible API format with xAI's base URL.
/// Configure via env vars: XAI_API_KEY, XAI_MODEL (default: grok-3)
pub struct XaiProvider {
    inner: OpenAiProvider,
}

impl XaiProvider {
    pub fn new(timeout_secs: u64, system_name: String) -> Result<Self, ProviderError> {
        let api_key = std::env::var("XAI_API_KEY")
            .map_err(|_| ProviderError::ConnectionError("XAI_API_KEY not set".into()))?;
        let model = std::env::var("XAI_MODEL")
            .unwrap_or_else(|_| "grok-3".into());

        Ok(Self {
            inner: OpenAiProvider::with_config(
                api_key,
                model,
                "https://api.x.ai/v1".to_string(),
                timeout_secs,
                system_name,
            ),
        })
    }
}

#[async_trait]
impl Provider for XaiProvider {
    async fn generate(
        &self,
        system_prompt: &str,
        history: &[Event],
        new_event: &Event,
        agent_context: &str,
        telemetry_tx: Option<mpsc::Sender<String>>,
        max_tokens: Option<u32>,
    ) -> Result<String, ProviderError> {
        self.inner.generate(system_prompt, history, new_event, agent_context, telemetry_tx, max_tokens).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::scope::Scope;
    use crate::providers::openai::OpenAiProvider;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_xai_provider_delegates_to_openai() {
        let mock_server = MockServer::start().await;

        // Construct XaiProvider with mock server URL via the inner OpenAiProvider
        let provider = XaiProvider {
            inner: OpenAiProvider::with_config(
                "test-xai-key".into(),
                "grok-3".into(),
                mock_server.uri(),
                30,
                "Apis".into(),
            ),
        };

        let mock_response = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello from Grok!\"}}]}\n\ndata: [DONE]\n";

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
            .mount(&mock_server)
            .await;

        let event = Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Hi!".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        let res = provider.generate("sys", &[], &event, "", None, None).await.unwrap();
        assert_eq!(res, "Hello from Grok!");
    }
}
