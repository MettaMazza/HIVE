use reqwest::Client;
use serde::{Deserialize, Serialize};
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::models::message::Event;
use super::{Provider, ProviderError};

#[derive(Serialize, Deserialize, Clone)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
}

/// A provider implementation for a local Ollama instance.
pub struct OllamaProvider {
    client: Client,
    endpoint: String,
    model: String,
}

impl OllamaProvider {
    /// Connects to a local Ollama instance defaulting to `qwen3.5:35b` as requested.
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            endpoint: "http://localhost:11434/api/chat".to_string(),
            model: "qwen3.5:35b".to_string(),
        }
    }
    #[cfg(not(tarpaulin_include))]
    fn map_chunk_err(e: reqwest::Error) -> ProviderError {
        ProviderError::ConnectionError(e.to_string())
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    async fn generate(
        &self,
        system_prompt: &str,
        history: &[Event],
        new_event: &Event,
        telemetry_tx: Option<mpsc::Sender<String>>,
    ) -> Result<String, ProviderError> {
        let mut messages = vec![
            OllamaMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            }
        ];

        // Format the securely-scoped history
        for event in history {
            let role = if event.author_name == "Apis" {
                "assistant"
            } else {
                "user"
            };

            // Inject the author name softly into user messages so Apis knows who is talking
            let content = if role == "user" {
                format!("{}: {}", event.author_name, event.content)
            } else {
                event.content.clone()
            };

            messages.push(OllamaMessage {
                role: role.to_string(),
                content,
            });
        }

        // Add the current triggering event
        messages.push(OllamaMessage {
            role: "user".to_string(),
            content: format!("{}: {}", new_event.author_name, new_event.content),
        });

        let payload = OllamaRequest {
            model: self.model.clone(),
            messages,
            stream: true,
        };

        let mut res = self.client.post(&self.endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::ConnectionError(e.to_string()))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(ProviderError::ParseError(format!("Ollama error {}: {}", status, text)));
        }

        let mut full_response = String::new();
        let mut raw_buffer = String::new();

        while let Some(chunk) = res.chunk().await.map_err(Self::map_chunk_err)? {
            let chunk_str = String::from_utf8_lossy(&chunk);
            raw_buffer.push_str(&chunk_str);

            while let Some(newline_pos) = raw_buffer.find('\n') {
                let line: String = raw_buffer.drain(..=newline_pos).collect();
                let line_trimmed = line.trim();
                if line_trimmed.is_empty() {
                    continue;
                }

                // If JSON line parses, extract tokens
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line_trimmed) {
                    if let Some(msg) = parsed.get("message") {
                        if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                            full_response.push_str(content);
                        }

                        let mut telemetry_token = String::new();
                        
                        // Qwen3.5 nests thinking inside the message object!
                        if let Some(thinking) = msg.get("thinking").and_then(|v| v.as_str()) {
                            telemetry_token.push_str(thinking);
                        } else if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                            telemetry_token.push_str(content);
                        }
                        
                        if let Some(ref tx) = telemetry_tx {
                            if !telemetry_token.is_empty() {
                                let _ = tx.send(telemetry_token).await;
                            }
                        }
                    }

                    if parsed.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
                        break;
                    }
                } else {
                    return Err(ProviderError::ParseError("Failed to parse JSON stream chunk".into()));
                }
            }
        }

        Ok(full_response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::scope::Scope;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_provider_success() {
        let mock_server = MockServer::start().await;
        
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        let mock_response = "{\"message\": {\"role\": \"assistant\", \"content\": \"Sure, here's your context.\"}, \"done\": true}\n";

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
            .mount(&mock_server)
            .await;

        let history = vec![
            Event { platform: "cli".into(), scope: Scope::Public, author_name: "Apis".into(), content: "I am here.".into() },
            Event { platform: "cli".into(), scope: Scope::Public, author_name: "Alice".into(), content: "Hi!".into() },
        ];
        
        // Single JSON response is technically a 1-line stream chunk
        let res = provider.generate("sys", &history, &Event {
            platform: "cli".into(),
            scope: Scope::Public,
            author_name: "Bob".into(),
            content: "What's up?".into(),
        }, None).await.unwrap();

        assert_eq!(res, "Sure, here's your context.");
    }

    #[tokio::test]
    async fn test_provider_http_error() {
        let mock_server = MockServer::start().await;
        
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&mock_server)
            .await;

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public,
            author_name: "Bob".into(),
            content: "Bork?".into(),
        }, None).await;

        assert!(matches!(res, Err(ProviderError::ParseError(_))));
    }

    #[tokio::test]
    async fn test_provider_connection_error() {
        let mut provider = OllamaProvider::new();
        provider.endpoint = "http://invalid.domain.that.does.not.exist:1234/api/chat".into();

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public,
            author_name: "Bob".into(),
            content: "Bork?".into(),
        }, None).await;

        assert!(matches!(res, Err(ProviderError::ConnectionError(_))));
    }

    #[tokio::test]
    async fn test_provider_parse_error() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string("invalid json body!\n"))
            .mount(&mock_server)
            .await;

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public,
            author_name: "Bob".into(),
            content: "Bork?".into(),
        }, None).await;

        assert!(matches!(res, Err(ProviderError::ParseError(_))));
    }

    #[tokio::test]
    async fn test_provider_early_eof() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .mount(&mock_server)
            .await;

        // No chunks, natural EOF. 
        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public,
            author_name: "Bob".into(),
            content: "Bork?".into(),
        }, None).await;

        assert_eq!(res.unwrap(), "");
    }

    #[tokio::test]
    async fn test_provider_reasoning_telemetry() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        let mock_response = "{\"message\": {\"role\": \"assistant\", \"thinking\": \"I am thinking...\"}, \"done\": true}\n";

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
            .mount(&mock_server)
            .await;

        let (tx, mut rx) = mpsc::channel(10);
        
        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public,
            author_name: "Bob".into(),
            content: "Bork?".into(),
        }, Some(tx)).await;

        assert_eq!(res.unwrap(), ""); // content was empty
        assert_eq!(rx.recv().await.unwrap(), "I am thinking...");
    }

    #[tokio::test]
    async fn test_provider_missing_content() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        let mock_response = "{\"message\": {\"role\": \"assistant\"}, \"done\": true}\n";

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
            .mount(&mock_server)
            .await;

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public,
            author_name: "Bob".into(),
            content: "Bork?".into(),
        }, None).await;

        assert_eq!(res.unwrap(), "");
    }
}
