//! Ollama Embedding Client — calls /api/embed for vector embeddings.
//!
//! Used by the HIVE memory system to generate 768-dim vectors for semantic search.
//! Requires `nomic-embed-text` (or another embedding model) pulled in Ollama.

use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct EmbedClient {
    client: Client,
    base_url: String,
    model: String,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Serialize)]
struct EmbedBatchRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

impl EmbedClient {
    /// Create a new embedding client from environment variables.
    /// Returns `None` if `HIVE_EMBED_MODEL` is not set or empty.
    pub fn from_env() -> Option<Self> {
        let model = std::env::var("HIVE_EMBED_MODEL").ok()?;
        if model.is_empty() {
            return None;
        }

        let base_url = std::env::var("HIVE_OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());

        tracing::info!("[EMBED] 🧠 Embedding client initialized: model={}, url={}", model, base_url);

        Some(Self {
            client: Client::new(),
            base_url,
            model,
        })
    }

    /// Embed a single text string. Returns a 768-dim vector.
    /// For texts longer than 2048 chars, embeds the first chunk only.
    /// Use `embed_chunked` for full coverage of long texts.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let chunk = if text.len() > 2048 {
            &text[..text.floor_char_boundary(2048)]
        } else {
            text
        };

        let url = format!("{}/api/embed", self.base_url);
        let req = EmbedRequest {
            model: &self.model,
            input: chunk,
        };

        let resp = self.client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("Embed HTTP error: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Embed API error ({}): {}", status, body));
        }

        let parsed: EmbedResponse = resp
            .json()
            .await
            .map_err(|e| format!("Embed parse error: {}", e))?;

        parsed.embeddings.into_iter().next()
            .ok_or_else(|| "Embed response contained no embeddings".to_string())
    }

    /// Embed multiple texts in a single API call.
    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{}/api/embed", self.base_url);

        // Truncate each text to 2048 chars
        let truncated: Vec<&str> = texts.iter()
            .map(|t| if t.len() > 2048 { &t[..t.floor_char_boundary(2048)] } else { t })
            .collect();

        let req = EmbedBatchRequest {
            model: &self.model,
            input: truncated,
        };

        let resp = self.client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("Embed batch HTTP error: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Embed batch API error ({}): {}", status, body));
        }

        let parsed: EmbedResponse = resp
            .json()
            .await
            .map_err(|e| format!("Embed batch parse error: {}", e))?;

        Ok(parsed.embeddings)
    }

    /// Get the model name (for logging).
    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// Split text into chunks of up to `chunk_size` chars, breaking at word boundaries.
    pub fn chunk_text(text: &str, chunk_size: usize) -> Vec<String> {
        if text.len() <= chunk_size {
            return vec![text.to_string()];
        }

        let mut chunks = Vec::new();
        let mut start = 0;

        while start < text.len() {
            let end = std::cmp::min(start + chunk_size, text.len());
            // Try to break at a word boundary (space or newline)
            let break_at = if end < text.len() {
                text[start..end].rfind(|c: char| c == ' ' || c == '\n')
                    .map(|pos| start + pos + 1)
                    .unwrap_or(end)
            } else {
                end
            };
            let chunk = &text[start..break_at];
            if !chunk.trim().is_empty() {
                chunks.push(chunk.to_string());
            }
            start = break_at;
        }

        chunks
    }

    /// Embed a long text by chunking it. Returns (chunk_index, vector) pairs.
    /// Each chunk gets its own vector for independent similarity matching.
    pub async fn embed_chunked(&self, text: &str) -> Result<Vec<(usize, Vec<f32>)>, String> {
        let chunks = Self::chunk_text(text, 2048);
        let mut results = Vec::new();

        for (i, chunk) in chunks.iter().enumerate() {
            let vec = self.embed(chunk).await?;
            results.push((i, vec));
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_env_missing() {
        // Without HIVE_EMBED_MODEL set, should return None
        unsafe { std::env::remove_var("HIVE_EMBED_MODEL"); }
        assert!(EmbedClient::from_env().is_none());
    }

    #[test]
    fn test_from_env_empty() {
        unsafe { std::env::set_var("HIVE_EMBED_MODEL", ""); }
        assert!(EmbedClient::from_env().is_none());
        unsafe { std::env::remove_var("HIVE_EMBED_MODEL"); }
    }
}
