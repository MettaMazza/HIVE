#![allow(clippy::collapsible_if)]
use reqwest::Client;
use serde::{Deserialize, Serialize};
use async_trait::async_trait;
use tokio::sync::mpsc;


use crate::models::message::Event;
use super::{Provider, ProviderError};

// ─── VISION CACHE ────────────────────────────────────────────────
// Caches user-uploaded image bytes to disk so that images remain
// visible in the rolling context window on subsequent turns.
// Without this, history messages get `images: None` and the model
// loses the ability to "see" images from earlier in the conversation.

mod vision_cache {
    use std::path::PathBuf;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn cache_dir() -> PathBuf {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        current_dir.join("memory/cache/vision")
    }

    fn url_hash(url: &str) -> String {
        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Save base64-encoded image bytes to the vision cache, keyed by URL.
    pub async fn save(url: &str, b64_data: &str) {
        let dir = cache_dir();
        if tokio::fs::create_dir_all(&dir).await.is_err() {
            return;
        }
        let path = dir.join(format!("{}.b64", url_hash(url)));
        let _ = tokio::fs::write(&path, b64_data.as_bytes()).await;
    }

    /// Load base64-encoded image bytes from the vision cache, if available.
    pub async fn load(url: &str) -> Option<String> {
        let path = cache_dir().join(format!("{}.b64", url_hash(url)));
        tokio::fs::read_to_string(&path).await.ok()
    }

    /// Extract image URLs from a message content string containing [USER_ATTACHMENT] tags.
    pub fn extract_image_urls(content: &str) -> Vec<String> {
        let mut urls = Vec::new();
        for block in content.split("[USER_ATTACHMENT:").skip(1) {
            if let Some(end_idx) = block.find(']') {
                let tag_content = &block[..end_idx];
                if tag_content.contains("type: image/") {
                    if let Some(url_start) = tag_content.find("url: ") {
                        let url = tag_content[url_start + 5..].trim().to_string();
                        if !url.is_empty() {
                            urls.push(url);
                        }
                    }
                }
            }
        }
        urls
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct OllamaMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<String>>,
}

#[derive(Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
    /// Explicit context window size. Ollama defaults to 2048 if not set,
    /// which silently discards most of the prompt. qwen3.5 supports 256K natively.
    #[serde(skip_serializing_if = "Option::is_none")]
    num_ctx: Option<u32>,
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    keep_alive: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

/// A provider implementation for a local Ollama instance.
pub struct OllamaProvider {
    client: Client,
    endpoint: String,
    model: String,
}

impl OllamaProvider {
    /// Connects to a local Ollama instance defaulting to `qwen3.5:122b` as requested.
    pub fn new() -> Self {
        Self::with_model("qwen3.5:122b")
    }

    /// Creates an OllamaProvider targeting a specific model.
    /// Use this for platform-specific model routing (e.g., `qwen3.5:9b` for glasses).
    pub fn with_model(model: &str) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_else(|_| Client::new()),
            endpoint: "http://localhost:11434/api/chat".to_string(),
            model: model.to_string(),
        }
    }
    fn map_chunk_err(e: reqwest::Error) -> ProviderError {
        ProviderError::ConnectionError(e.to_string())
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    #[tracing::instrument(skip(self, system_prompt, history, telemetry_tx), fields(model=%self.model, user=%new_event.author_name))]
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

        // Format the securely-scoped history FIRST
        // Individual messages are capped to prevent one massive response from bloating all subsequent calls.
        // Full messages remain in working memory & disk — only the LLM prompt copy is capped.
        const HISTORY_MSG_CAP: usize = 8000;
        for event in history {
            let role = if event.author_name == "Apis" {
                "assistant"
            } else {
                "user"
            };

            let ts = event.timestamp.as_deref().unwrap_or("00:00:00");
            let mid = event.message_index.unwrap_or(0);
            let prefix = format!("[{} | Msg {}]", ts, mid);

            // Inject the author name softly into user messages so Apis knows who is talking
            let content = if role == "user" {
                format!("{} [AUTHOR: {} -> APIS]: {}", prefix, event.author_name, event.content)
            } else {
                // SPRINT 3: JSON Content Forcing
                // If Apis responded in plain text, wrap it into a mock `reply_to_request` execution.
                // This prevents "Monkey See, Monkey Do" plain text degradation on Turn 1.
                if !event.content.trim().starts_with("```json") && !event.content.trim().starts_with('{') {
                    // Properly escape the original string into a JSON string
                    let escaped_content = serde_json::to_string(&event.content).unwrap_or_else(|_| "\"Failed to escape\"".to_string());
                    format!(
                        "{} \n```json\n{{\n  \"tasks\": [\n    {{\n      \"task_id\": \"hist_{}\",\n      \"tool_type\": \"reply_to_request\",\n      \"description\": {},\n      \"depends_on\": []\n    }}\n  ]\n}}\n```",
                        prefix,
                        mid,
                        escaped_content
                    )
                } else {
                    format!("{} \n{}", prefix, event.content)
                }
            };

            // Cap oversized history messages to prevent prompt bloat from prior mega-responses.
            let capped_content = if content.len() > HISTORY_MSG_CAP {
                let truncated: String = content.chars().take(HISTORY_MSG_CAP).collect();
                format!("{}...\n[Message truncated from {} to {} chars for context efficiency. Full version retained in memory.]", truncated, content.len(), HISTORY_MSG_CAP)
            } else {
                content
            };

            // ─── VISION CACHE: Attach cached image bytes for history messages ───
            // Without this, history messages lose their image pixels and the model
            // can only see the text tag `[USER_ATTACHMENT: ...]` — not the actual image.
            let cached_images = {
                let image_urls = vision_cache::extract_image_urls(&capped_content);
                let mut cached = Vec::new();
                for url in &image_urls {
                    if let Some(b64) = vision_cache::load(url).await {
                        cached.push(b64);
                    }
                }
                if cached.is_empty() { None } else { Some(cached) }
            };

            messages.push(OllamaMessage {
                role: role.to_string(),
                content: capped_content,
                images: cached_images,
            });
        }

        // Pinned System Prompt: Load the strict operational rules AFTER the history
        // to combat LLM "lost in the middle" recency bias on massive context windows.
        messages.push(OllamaMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
            images: None,
        });

        let mut final_user_message = format!("[AUTHOR: {} -> APIS]: {}", new_event.author_name, new_event.content);
        if !agent_context.is_empty() {
            final_user_message.push_str("\n\n[ISOLATED EXECUTION TIMELINE]\n");
            final_user_message.push_str(agent_context);
        }

        // Native Vision Support: Extract image URLs from [USER_ATTACHMENT] tags and fetch them.
        // Images are also cached locally so they remain visible in history on subsequent turns.
        let mut b64_images = Vec::new();
        let image_urls = vision_cache::extract_image_urls(&final_user_message);
        for url in &image_urls {
            // Try cache first to avoid redundant CDN fetches
            if let Some(cached_b64) = vision_cache::load(url).await {
                b64_images.push(cached_b64);
            } else if let Ok(resp) = self.client.get(url.as_str()).send().await {
                if let Ok(bytes) = resp.bytes().await {
                    use base64::{Engine as _, engine::general_purpose::STANDARD};
                    let b64 = STANDARD.encode(&bytes);
                    // Cache for future turns
                    vision_cache::save(url, &b64).await;
                    b64_images.push(b64);
                }
            }
        }

        let images_opt = if b64_images.is_empty() { None } else { Some(b64_images) };

        // Strict enforcement for Turn 1 "Monkey see, monkey do" conversational degradation
        // Skip this explicit reply_to_request coercion for internal audit runs so the
        // Observer doesn't waste 12+ seconds hallucinating a massive 500-token ReAct envelope.
        if !agent_context.contains("[=== INTERNAL ENGINE INSTRUCTION: SWITCH TO AUDIT MODE ===]") {
            final_user_message.push_str("\n\n[SYSTEM ENFORCEMENT: You must output EXACTLY ONE valid JSON block. Do not output raw conversational text. Use the `reply_to_request` tool to speak to the user.]");
        } else {
            // Give the Observer a lighter hint so it just prints the tiny verdict JSON.
            final_user_message.push_str("\n\n[SYSTEM ENFORCEMENT: Output EXACTLY ONE valid JSON block representing your audit verdict.]");
        }

        // Add the current triggering event
        messages.push(OllamaMessage {
            role: "user".to_string(),
            content: final_user_message,
            images: images_opt,
        });

        let payload = OllamaRequest {
            model: self.model.clone(),
            messages,
            stream: true,
            keep_alive: "-1".to_string(),
            options: Some(OllamaOptions {
                num_predict: max_tokens,
                num_ctx: Some(131_072),
            }),
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

        let mut first_token_received = false;
        let mut full_response = String::new();
        let mut raw_buffer = String::new();
        let mut final_prompt_tokens = 0;
        let mut final_eval_tokens = 0;
        let mut ttft_duration = tokio::time::Duration::from_secs(0);
        let prompt_bytes: usize = payload.messages.iter().map(|m| m.content.len()).sum();
        let start_time = tokio::time::Instant::now();

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
                            // NOTE: .content is the JSON plan — do NOT send to telemetry.
                            // Only .thinking tokens are streamed to telemetry.
                            // Tool results are batch-sent by each tool's own telemetry_tx.
                        }

                        // Some models stream reasoning separately in a 'thinking' key
                        if let Some(thinking) = msg.get("thinking").and_then(|v| v.as_str()) {
                            if let Some(ref tx) = telemetry_tx {
                                if !thinking.is_empty() {
                                    let _ = tx.send(thinking.to_string()).await;
                                }
                            }
                        }
                    }

                    if parsed.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
                        final_prompt_tokens = parsed.get("prompt_eval_count").and_then(|v| v.as_u64()).unwrap_or(0);
                        final_eval_tokens = parsed.get("eval_count").and_then(|v| v.as_u64()).unwrap_or(0);
                        break;
                    }

                    if !first_token_received && parsed.get("message").is_some() {
                        ttft_duration = start_time.elapsed();
                        first_token_received = true;
                    }
                } else {
                    return Err(ProviderError::ParseError("Failed to parse JSON stream chunk".into()));
                }
            }
        }

        let total_time = start_time.elapsed();
        let metrics = crate::engine::telemetry::LatencyMetrics {
            timestamp: chrono::Utc::now().to_rfc3339(),
            model: self.model.clone(),
            prompt_bytes,
            history_len: history.len(),
            ttft_ms: ttft_duration.as_millis() as u64,
            total_ms: total_time.as_millis() as u64,
            prompt_tokens: final_prompt_tokens,
            eval_tokens: final_eval_tokens,
        };

        tokio::spawn(async move {
            crate::engine::telemetry::log_latency(metrics).await;
        });

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
            Event { platform: "cli".into(), scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() }, author_name: "Apis".into(), author_id: "test".into(), content: "I am here.".into(), timestamp: None, message_index: None },
            Event { platform: "cli".into(), scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() }, author_name: "Alice".into(), author_id: "test".into(), content: "Hi!".into(), timestamp: None, message_index: None },
        ];
        
        // Single JSON response is technically a 1-line stream chunk
        let new_event = Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "What's up?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };
        let res = provider.generate("sys", &history, &new_event, "", None, None).await.unwrap();

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
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Bork?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

        assert!(matches!(res, Err(ProviderError::ParseError(_))));
    }

    #[tokio::test]
    async fn test_provider_connection_error() {
        let mut provider = OllamaProvider::new();
        provider.endpoint = "http://invalid.domain.that.does.not.exist:1234/api/chat".into();

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Bork?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

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
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Bork?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

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
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Bork?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

        assert_eq!(res.unwrap(), "");
    }

    #[tokio::test]
    async fn test_provider_reasoning_telemetry() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        let mock_response = "{\"message\": {\"role\": \"assistant\", \"thinking\": \"I am thinking...\", \"content\": \"Final answer\"}, \"done\": true}\n";

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
            .mount(&mock_server)
            .await;

        let (tx, mut rx) = mpsc::channel(10);
        
        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Bork?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", Some(tx), None).await;

        let first_recv = rx.recv().await.unwrap();
        assert_eq!(first_recv, "I am thinking...");
        assert_eq!(res.unwrap(), "Final answer");
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
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Bork?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

        assert_eq!(res.unwrap(), "");
    }

    #[tokio::test]
    async fn test_ollama_stream_fragmented() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        let mock_response = "{\"message\": {\"role\": \"assistant\", \"content\": \"part1\"}}\n{\"message\": {\"content\": \" part2\"}}\n{\"message\": {\"content\": \" done!\"}, \"done\": true}\n";

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
            .mount(&mock_server)
            .await;

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Stream?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

        assert_eq!(res.unwrap(), "part1 part2 done!");
    }

    #[tokio::test]
    async fn test_ollama_stream_disconnect() {
        let mock_server = MockServer::start().await;
        let mut provider = OllamaProvider::new();
        provider.endpoint = format!("{}/api/chat", mock_server.uri());

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable Drops Stream"))
            .mount(&mock_server)
            .await;

        let res = provider.generate("sys", &[], &Event {
            platform: "cli".into(),
            scope: Scope::Public { channel_id: "t".into(), user_id: "t".into() },
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Disconnect?".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }, "", None, None).await;

        assert!(res.is_err());
    }
}
