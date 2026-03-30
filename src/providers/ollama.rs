#![allow(clippy::collapsible_if)]
use std::sync::Arc;
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
    keep_alive: i64,
    /// Ollama native output format enforcement.
    /// Set to `"json"` to enforce valid JSON at the GBNF grammar level.
    /// This prevents the model from producing non-JSON tokens regardless
    /// of context size or prompt pressure.
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<serde_json::Value>,
    /// Controls whether thinking models (qwen3.5) emit reasoning tokens.
    /// Set to `false` for Observer audit calls — the Skeptic is a simple
    /// classifier that doesn't need extended reasoning chains.
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

/// A provider implementation for a local Ollama instance.
pub struct OllamaProvider {
    client: Client,
    endpoint: String,
    model: String,
    /// External stop flag — checked during streaming to abort long generations.
    stop_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl OllamaProvider {
    /// Connects to a local Ollama instance.
    /// Model defaults to `qwen3.5:35b` but can be overridden via `HIVE_MODEL` env var.
    pub fn new() -> Self {
        let model = std::env::var("HIVE_MODEL")
            .or_else(|_| std::env::var("OLLAMA_MODEL"))
            .unwrap_or_else(|_| "qwen3.5:35b".into());
        Self::with_model(&model)
    }

    /// Creates an OllamaProvider targeting a specific model.
    /// Use this for platform-specific model routing (e.g., `qwen3.5:9b` for glasses).
    pub fn with_model(model: &str) -> Self {
        let base_url = std::env::var("HIVE_OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        let endpoint = format!("{}/api/chat", base_url);
        Self {
            client: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
            endpoint,
            model: model.to_string(),
            stop_flag: None,
        }
    }

    /// Set the external stop flag. Checked during streaming to abort on /stop.
    pub fn set_stop_flag(&mut self, flag: Arc<std::sync::atomic::AtomicBool>) {
        self.stop_flag = Some(flag);
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    #[tracing::instrument(skip(self, system_prompt, history, new_event, agent_context, telemetry_tx, max_tokens), fields(model=%self.model, user=%new_event.author_name))]
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

            // History messages never carry image bytes. The model already processed
            // each image on the turn it was sent (via current-event attachment below).
            // Re-attaching cached bytes to every historical message on every call
            // caused JPEG format crashes, stream corruption, and 5x latency spikes.
            // The text metadata [USER_ATTACHMENT: ...] is preserved so the model
            // still knows images were exchanged.
            messages.push(OllamaMessage {
                role: role.to_string(),
                content: capped_content,
                images: None,
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

        // Reminder of JSON output structure. The actual JSON enforcement is now handled
        // at the GBNF grammar level via `format: "json"` in the OllamaRequest.
        // This text hint reinforces the structure without implying the model should
        // reply_to_request on every turn (which caused 9-minute reasoning spirals).
        if !agent_context.contains("[=== INTERNAL ENGINE INSTRUCTION: SWITCH TO AUDIT MODE ===]") {
            final_user_message.push_str("\n\n[SYSTEM: JSON output enforced. Execute your next planned action.]");
        } else {
            final_user_message.push_str("\n\n[SYSTEM: Output your audit verdict as JSON.]");
        }

        // Add the current triggering event
        messages.push(OllamaMessage {
            role: "user".to_string(),
            content: final_user_message,
            images: images_opt,
        });

        // Detect call type for per-call Ollama parameter tuning.
        let is_audit = agent_context.contains("[=== INTERNAL ENGINE INSTRUCTION: SWITCH TO AUDIT MODE ===]");
        let is_internal = new_event.platform.starts_with("system:");

        let payload = OllamaRequest {
            model: self.model.clone(),
            messages,
            stream: true,
            keep_alive: -1,
            // format:json enforces GBNF grammar — only for calls that output JSON.
            // Synthesis outputs narrative prose; forcing JSON hangs the model for 5+ min.
            format: if is_internal { None } else { Some(serde_json::Value::String("json".into())) },
            // Thinking tokens: disabled for audit (simple classifier), disabled for
            // internal tasks (synthesis is a summarizer, not a reasoner).
            think: if is_audit || is_internal { Some(false) } else { None },
            options: Some(OllamaOptions {
                num_predict: max_tokens,
                num_ctx: Some(131_072),
            }),
        };

        let pre_send = tokio::time::Instant::now();
        let prompt_bytes: usize = payload.messages.iter().map(|m| m.content.len()).sum();
        let msg_count = payload.messages.len();
        tracing::info!("[PROVIDER] 📤 Sending request to Ollama — {} messages, {} prompt bytes, model={}", msg_count, prompt_bytes, payload.model);

        let mut res = self.client.post(&self.endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("[PROVIDER] ❌ send() failed after {:.1}s: {}", pre_send.elapsed().as_secs_f64(), e);
                ProviderError::ConnectionError(e.to_string())
            })?;

        let send_elapsed = pre_send.elapsed();
        let status = res.status();
        let content_type = res.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("unknown").to_string();
        let content_length = res.headers().get("content-length").and_then(|v| v.to_str().ok()).unwrap_or("chunked").to_string();
        tracing::info!("[PROVIDER] 📥 HTTP {} received in {:.3}s — content-type={}, content-length={}", status, send_elapsed.as_secs_f64(), content_type, content_length);

        if !status.is_success() {
            let text = res.text().await.unwrap_or_default();
            return Err(ProviderError::ParseError(format!("Ollama error {}: {}", status, text)));
        }

        let mut first_token_received = false;
        let mut full_response = String::new();
        let mut raw_buffer = String::new();
        let mut final_prompt_tokens = 0;
        let mut final_eval_tokens = 0;
        let mut ttft_duration = tokio::time::Duration::from_secs(0);
        let start_time = tokio::time::Instant::now();
        let mut total_chunks: u64 = 0;

        let mut chunk_retries: u8 = 0;
        let mut thinking_buffer = String::new(); // Track thinking tokens for spiral detection
        let mut spiral_detected = false;
        loop {
            // Check stop flag every iteration
            if let Some(ref flag) = self.stop_flag {
                if flag.load(std::sync::atomic::Ordering::SeqCst) {
                    tracing::warn!("[PROVIDER] 🛑 Stop flag detected during streaming at chunk {}. Aborting.", total_chunks);
                    break;
                }
            }
            let chunk_result = res.chunk().await;
            let chunk = match chunk_result {
                Ok(Some(c)) => {
                    total_chunks += 1;
                    if total_chunks == 1 {
                        tracing::info!("[PROVIDER] 🟢 First chunk received — {:.3}s after send, {:.3}s total, {} bytes",
                            start_time.elapsed().as_secs_f64(),
                            pre_send.elapsed().as_secs_f64(),
                            c.len());
                    }
                    c
                },
                Ok(None) => break, // Stream ended normally
                Err(e) => {
                    chunk_retries += 1;
                    let total_elapsed = pre_send.elapsed().as_secs_f64();
                    tracing::warn!("[PROVIDER] Chunk read error (retry {}/3) — {:.3}s total elapsed, {} chunks received, {} response chars accumulated: {}",
                        chunk_retries, total_elapsed, total_chunks, full_response.len(), e);
                    if chunk_retries >= 3 {
                        if !full_response.is_empty() {
                            tracing::warn!("[PROVIDER] Returning partial response ({} chars, {} chunks) after {} chunk errors at {:.1}s: {}",
                                full_response.len(), total_chunks, chunk_retries, total_elapsed, e);
                            break;
                        }
                        return Err(ProviderError::ConnectionError(format!(
                            "Chunk read failed after {} retries ({:.1}s elapsed, {} chunks received): {}",
                            chunk_retries, total_elapsed, total_chunks, e
                        )));
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(500 * chunk_retries as u64)).await;
                    continue;
                }
            };

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
                            // Spiral detection: track thinking tokens
                            if !thinking.is_empty() {
                                thinking_buffer.push_str(thinking);
                                // Check every 500 chars for repetition
                                if thinking_buffer.len() > 500 {
                                    if detect_thought_spiral(&thinking_buffer) {
                                        tracing::warn!("[PROVIDER] 🌀 Thought spiral detected at {} thinking chars, {} chunks. Force-stopping.", thinking_buffer.len(), total_chunks);
                                        spiral_detected = true;
                                        break;
                                    }
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

        // If a thought spiral was detected, return the error with partial context
        if spiral_detected {
            let summary = if thinking_buffer.len() > 200 {
                thinking_buffer.chars().take(200).collect::<String>()
            } else {
                thinking_buffer.clone()
            };
            return Err(ProviderError::ThoughtSpiral(summary));
        }

        Ok(full_response.trim().to_string())
    }
}

/// Detect repetitive thought spirals in thinking token output.
/// Returns true if any substring of 80+ chars appears 3+ times.
fn detect_thought_spiral(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let min_pattern_len = 80;
    
    if chars.len() < min_pattern_len * 3 {
        return false;
    }

    // Check from the end of the buffer — spirals are at the tail
    // Take the last 600 chars and look for a repeating pattern
    let start = if chars.len() > 600 { chars.len() - 600 } else { 0 };
    let window = &chars[start..];
    
    // Try pattern lengths from 80 to 200
    for pat_len in (min_pattern_len..=200).step_by(20) {
        if window.len() < pat_len * 3 {
            continue;
        }
        let pattern = &window[..pat_len];
        // Count how many times this pattern appears in the window
        let mut count = 0;
        for i in 0..=(window.len() - pat_len) {
            if &window[i..i + pat_len] == pattern {
                count += 1;
                if count >= 3 {
                    return true;
                }
            }
        }
    }
    false
}


#[cfg(test)]
#[path = "ollama_tests.rs"]
mod tests;
