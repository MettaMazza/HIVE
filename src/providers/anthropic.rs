use reqwest::Client;
use serde::{Deserialize, Serialize};
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::models::message::Event;
use super::{Provider, ProviderError};

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    system: String,
    max_tokens: u32,
    stream: bool,
}

#[derive(Deserialize)]
struct ContentBlockDelta {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicStreamEvent {
    #[serde(default, rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<ContentBlockDelta>,
}

/// Anthropic Claude provider.
/// Configure via env vars: ANTHROPIC_API_KEY, ANTHROPIC_MODEL (default: claude-sonnet-4-20250514)
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    model: String,
}

impl AnthropicProvider {
    pub fn new() -> Result<Self, ProviderError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| ProviderError::ConnectionError("ANTHROPIC_API_KEY not set".into()))?;
        let model = std::env::var("ANTHROPIC_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-20250514".into());

        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_else(|_| Client::new()),
            api_key,
            model,
        })
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
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
            messages.push(AnthropicMessage { role: role.to_string(), content: capped });
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

        messages.push(AnthropicMessage { role: "user".to_string(), content: final_content });

        let payload = AnthropicRequest {
            model: self.model.clone(),
            messages,
            system: system_prompt.to_string(),
            max_tokens: max_tokens.unwrap_or(4096),
            stream: true,
        };

        let mut res = self.client.post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::ConnectionError(e.to_string()))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(ProviderError::ParseError(format!("Anthropic API error {}: {}", status, text)));
        }

        let mut full_response = String::new();
        let mut raw_buffer = String::new();

        while let Some(chunk) = res.chunk().await.map_err(|e| ProviderError::ConnectionError(e.to_string()))? {
            let chunk_str = String::from_utf8_lossy(&chunk);
            raw_buffer.push_str(&chunk_str);

            while let Some(newline_pos) = raw_buffer.find('\n') {
                let line: String = raw_buffer.drain(..=newline_pos).collect();
                let line_trimmed = line.trim();

                if line_trimmed.is_empty() {
                    continue;
                }

                if let Some(json_str) = line_trimmed.strip_prefix("data: ") {
                    if let Ok(event) = serde_json::from_str::<AnthropicStreamEvent>(json_str) {
                        if event.event_type == "content_block_delta" {
                            if let Some(ref delta) = event.delta {
                                if let Some(ref text) = delta.text {
                                    full_response.push_str(text);
                                }
                            }
                        }
                    }
                }
            }
        }

        let _ = telemetry_tx;

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
    async fn test_anthropic_provider_success() {
        let mock_server = MockServer::start().await;

        let _provider = AnthropicProvider {
            client: Client::new(),
            api_key: "test-key".into(),
            model: "claude-sonnet-4-20250514".into(),
        };

        let mock_response = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Hello from Claude!\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n";

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
            .mount(&mock_server)
            .await;

        // For testing we need to override the URL — but since Anthropic hardcodes the URL,
        // we test the streaming parser directly
        let event = Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Hi!".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        // Test that the stream event parser works
        let raw = "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Hello from Claude!\"}}";
        let json_str = raw.strip_prefix("data: ").unwrap();
        let parsed: AnthropicStreamEvent = serde_json::from_str(json_str).unwrap();
        assert_eq!(parsed.delta.unwrap().text.unwrap(), "Hello from Claude!");
        
        let _ = event; // used for structural completeness
    }
}
