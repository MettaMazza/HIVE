use reqwest::Client;
use serde::{Deserialize, Serialize};
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::models::message::Event;
use super::{Provider, ProviderError};

#[derive(Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    #[serde(default)]
    delta: Option<OpenAiDelta>,
    #[serde(default)]
    message: Option<OpenAiDelta>,
}

#[derive(Deserialize)]
struct OpenAiChunk {
    choices: Vec<OpenAiChoice>,
}

/// OpenAI-compatible provider (works with GPT-4o, GPT-4, etc.)
/// Configure via env vars: OPENAI_API_KEY, OPENAI_MODEL (default: gpt-4o)
pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAiProvider {
    pub fn new() -> Result<Self, ProviderError> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| ProviderError::ConnectionError("OPENAI_API_KEY not set".into()))?;
        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".into());
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".into());
        
        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_else(|_| Client::new()),
            api_key,
            model,
            base_url,
        })
    }

    /// Create with explicit parameters (used by xAI/Grok which shares the OpenAI API format)
    pub fn with_config(api_key: String, model: String, base_url: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_else(|_| Client::new()),
            api_key,
            model,
            base_url,
        }
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    async fn generate(
        &self,
        system_prompt: &str,
        history: &[Event],
        new_event: &Event,
        agent_context: &str,
        telemetry_tx: Option<mpsc::Sender<String>>,
        max_tokens: Option<u32>,
    ) -> Result<String, ProviderError> {
        let mut messages = Vec::new();

        // System prompt
        messages.push(OpenAiMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        });

        // History
        const HISTORY_MSG_CAP: usize = 2000;
        for event in history {
            let role = if event.author_name == "Apis" { "assistant" } else { "user" };
            let content = if role == "user" {
                format!("[AUTHOR: {} -> APIS]: {}", event.author_name, event.content)
            } else {
                event.content.clone()
            };
            let capped = if content.len() > HISTORY_MSG_CAP {
                let truncated: String = content.chars().take(HISTORY_MSG_CAP).collect();
                format!("{}... [truncated]", truncated)
            } else {
                content
            };
            messages.push(OpenAiMessage { role: role.to_string(), content: capped });
        }

        // Current event + agent context
        let mut final_content = format!("[AUTHOR: {} -> APIS]: {}", new_event.author_name, new_event.content);
        if !agent_context.is_empty() {
            final_content.push_str("\n\n[ISOLATED EXECUTION TIMELINE]\n");
            final_content.push_str(agent_context);
        }

        if !agent_context.contains("[=== INTERNAL ENGINE INSTRUCTION: SWITCH TO AUDIT MODE ===]") {
            final_content.push_str("\n\n[SYSTEM ENFORCEMENT: You must output EXACTLY ONE valid JSON block. Do not output raw conversational text. Use the `reply_to_request` tool to speak to the user.]");
        } else {
            final_content.push_str("\n\n[SYSTEM ENFORCEMENT: Output EXACTLY ONE valid JSON block representing your audit verdict.]");
        }

        messages.push(OpenAiMessage { role: "user".to_string(), content: final_content });

        let payload = OpenAiRequest {
            model: self.model.clone(),
            messages,
            stream: true,
            max_tokens,
        };

        let url = format!("{}/chat/completions", self.base_url);
        let mut res = self.client.post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::ConnectionError(e.to_string()))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(ProviderError::ParseError(format!("OpenAI API error {}: {}", status, text)));
        }

        let mut full_response = String::new();
        let mut raw_buffer = String::new();

        while let Some(chunk) = res.chunk().await.map_err(|e| ProviderError::ConnectionError(e.to_string()))? {
            let chunk_str = String::from_utf8_lossy(&chunk);
            raw_buffer.push_str(&chunk_str);

            while let Some(newline_pos) = raw_buffer.find('\n') {
                let line: String = raw_buffer.drain(..=newline_pos).collect();
                let line_trimmed = line.trim();
                
                if line_trimmed.is_empty() || line_trimmed == "data: [DONE]" {
                    continue;
                }

                let json_str = if let Some(stripped) = line_trimmed.strip_prefix("data: ") {
                    stripped
                } else {
                    line_trimmed
                };

                if let Ok(chunk) = serde_json::from_str::<OpenAiChunk>(json_str) {
                    for choice in &chunk.choices {
                        if let Some(ref delta) = choice.delta {
                            if let Some(ref content) = delta.content {
                                full_response.push_str(content);
                            }
                        }
                        if let Some(ref message) = choice.message {
                            if let Some(ref content) = message.content {
                                full_response.push_str(content);
                            }
                        }
                    }
                }
            }
        }

        // Drain any remaining buffer
        if !raw_buffer.trim().is_empty() && raw_buffer.trim() != "data: [DONE]" {
            if let Some(stripped) = raw_buffer.trim().strip_prefix("data: ") {
                if let Ok(chunk) = serde_json::from_str::<OpenAiChunk>(stripped) {
                    for choice in &chunk.choices {
                        if let Some(ref delta) = choice.delta {
                            if let Some(ref content) = delta.content {
                                full_response.push_str(content);
                            }
                        }
                    }
                }
            }
        }

        let _ = telemetry_tx; // Telemetry available for future use

        Ok(full_response.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::scope::Scope;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_openai_provider_success() {
        let mock_server = MockServer::start().await;

        let provider = OpenAiProvider::with_config(
            "test-key".into(),
            "gpt-4o".into(),
            mock_server.uri(),
        );

        let mock_response = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello from GPT!\"}}]}\n\ndata: [DONE]\n";

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
        assert_eq!(res, "Hello from GPT!");
    }

    #[tokio::test]
    async fn test_openai_provider_http_error() {
        let mock_server = MockServer::start().await;

        let provider = OpenAiProvider::with_config(
            "test-key".into(),
            "gpt-4o".into(),
            mock_server.uri(),
        );

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
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

        let res = provider.generate("sys", &[], &event, "", None, None).await;
        assert!(matches!(res, Err(ProviderError::ParseError(_))));
    }
}
