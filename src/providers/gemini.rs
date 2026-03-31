use reqwest::Client;
use serde::{Deserialize, Serialize};
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::models::message::Event;
use super::{Provider, ProviderError};

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

#[derive(Deserialize)]
struct GeminiResponsePart {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct GeminiResponseContent {
    #[serde(default)]
    parts: Vec<GeminiResponsePart>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    #[serde(default)]
    content: Option<GeminiResponseContent>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
}

/// Google Gemini provider.
/// Configure via env vars: GEMINI_API_KEY, GEMINI_MODEL (default: gemini-2.0-flash)
pub struct GeminiProvider {
    client: Client,
    api_key: String,
    model: String,
    system_name: String,
}

impl GeminiProvider {
    pub fn new(timeout_secs: u64, system_name: String) -> Result<Self, ProviderError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| ProviderError::ConnectionError("GEMINI_API_KEY not set".into()))?;
        let model = std::env::var("GEMINI_MODEL")
            .unwrap_or_else(|_| "gemini-2.0-flash".into());

        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .build()
                .unwrap_or_else(|_| Client::new()),
            api_key,
            model,
            system_name,
        })
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    async fn generate(
        &self,
        system_prompt: &str,
        history: &[Event],
        new_event: &Event,
        agent_context: &str,
        telemetry_tx: Option<mpsc::Sender<String>>,
        max_tokens: Option<u32>,
    ) -> Result<String, ProviderError> {
        let mut contents = Vec::new();

        // History
        const HISTORY_MSG_CAP: usize = 8000;
        for event in history {
            let role = if event.author_name == self.system_name { "model" } else { "user" };
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
            contents.push(GeminiContent {
                role: role.to_string(),
                parts: vec![GeminiPart { text: capped }],
            });
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

        contents.push(GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart { text: final_content }],
        });

        let payload = GeminiRequest {
            contents,
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiPart { text: system_prompt.to_string() }],
            }),
            generation_config: max_tokens.map(|n| GenerationConfig { max_output_tokens: Some(n) }),
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let res = self.client.post(&url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::ConnectionError(e.to_string()))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(ProviderError::ParseError(format!("Gemini API error {}: {}", status, text)));
        }

        let body = res.text().await.map_err(|e| ProviderError::ParseError(e.to_string()))?;
        let parsed: GeminiResponse = serde_json::from_str(&body)
            .map_err(|e| ProviderError::ParseError(format!("Failed to parse Gemini response: {}", e)))?;

        let mut full_response = String::new();
        for candidate in &parsed.candidates {
            if let Some(ref content) = candidate.content {
                for part in &content.parts {
                    if let Some(ref text) = part.text {
                        full_response.push_str(text);
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

    #[test]
    fn test_gemini_response_parsing() {
        let raw = r#"{"candidates":[{"content":{"parts":[{"text":"Hello from Gemini!"}]}}]}"#;
        let parsed: GeminiResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(
            parsed.candidates[0].content.as_ref().unwrap().parts[0].text.as_ref().unwrap(),
            "Hello from Gemini!"
        );
    }
}
