/// Reasoning Router — Dynamic model selection based on message complexity.
///
/// Uses a lightweight model (e.g., qwen3.5:9b) to classify incoming messages
/// as low/medium/high complexity, then routes to the appropriate model:
///   low    → qwen3.5:9b   (fast, simple tasks)
///   medium → qwen3.5:35b  (default, moderate tasks)
///   high   → qwen3.5:122b (complex reasoning)
///
/// Toggle: HIVE_ROUTER_ENABLED (default: false — single-model users unaffected)
/// Autonomy: always uses HIGH model (no classification needed).
/// Glasses: skipped (has its own provider).
use std::sync::Arc;
use crate::providers::Provider;
use crate::providers::ollama::OllamaProvider;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ComplexityLevel {
    Low,
    Medium,
    High,
}

impl std::fmt::Display for ComplexityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComplexityLevel::Low => write!(f, "low"),
            ComplexityLevel::Medium => write!(f, "medium"),
            ComplexityLevel::High => write!(f, "high"),
        }
    }
}

pub struct ReasoningRouter {
    /// The classifier model — fast, lightweight (e.g., 9b)
    classifier: Arc<dyn Provider>,
    /// Provider for simple tasks
    pub low_provider: Arc<dyn Provider>,
    /// Provider for moderate tasks (default)
    pub medium_provider: Arc<dyn Provider>,
    /// Provider for complex reasoning
    pub high_provider: Arc<dyn Provider>,
    /// Model names for logging
    low_model: String,
    medium_model: String,
    high_model: String,
}

const ROUTER_PROMPT: &str = r#"Classify this user message's reasoning complexity. Output ONLY valid JSON.

LOW: Short, minimal engagement. Greetings (hi, hey, hello), acknowledgements (ok, sure, got it, thanks, bye), emoji reactions, yes/no answers, single-word replies, simple farewells. Messages that need no research, no tools, and no deep thought.

MEDIUM: Default tier. Any casual conversational reply of medium to long length. General questions, opinions, discussions, sharing activities, talking about interests, status updates, moderate requests. If unsure, classify as medium.

HIGH: Any message with file attachments ([USER_ATTACHMENT]). Any file creation, code writing, or coding task of any kind. Any message requiring investigative tool use (web_search, researcher, codebase_read, memory lookup, bash commands, image generation — NOT reply_to_request, which is just the final delivery mechanism). Any complex science, philosophy, mathematics, STEM, or technical question. Any emotionally charged or intense conversational exchange (anger, distress, deep personal topics, heated debate). Multi-entity research. System architecture. Anything that demands depth, accuracy, or emotional intelligence.

Default to "medium" if unsure.

{"level": "low" | "medium" | "high"}

USER MESSAGE:
"#;

impl ReasoningRouter {
    /// Create from environment variables.
    pub fn from_env() -> Option<Self> {
        let enabled = std::env::var("HIVE_ROUTER_ENABLED")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        if !enabled {
            return None;
        }

        let system_name = std::env::var("HIVE_SYSTEM_NAME").unwrap_or_else(|_| "Apis".into());

        let router_model = std::env::var("HIVE_ROUTER_MODEL")
            .unwrap_or_else(|_| "qwen3.5:9b".into());
        let low_model = std::env::var("HIVE_LOW_MODEL")
            .unwrap_or_else(|_| "qwen3.5:9b".into());
        let medium_model = std::env::var("HIVE_MEDIUM_MODEL")
            .unwrap_or_else(|_| {
                // Fall back to the main model if no medium model is set
                std::env::var("HIVE_MODEL")
                    .or_else(|_| std::env::var("OLLAMA_MODEL"))
                    .unwrap_or_else(|_| "qwen3.5:35b".into())
            });
        let high_model = std::env::var("HIVE_HIGH_MODEL")
            .unwrap_or_else(|_| {
                std::env::var("HIVE_MODEL")
                    .or_else(|_| std::env::var("OLLAMA_MODEL"))
                    .unwrap_or_else(|_| "qwen3.5:35b".into())
            });

        tracing::info!("[ROUTER] 📊 Reasoning Router enabled:");
        tracing::info!("[ROUTER]   classifier = {}", router_model);
        tracing::info!("[ROUTER]   low = {}", low_model);
        tracing::info!("[ROUTER]   medium = {}", medium_model);
        tracing::info!("[ROUTER]   high = {}", high_model);

        let classifier: Arc<dyn Provider> = Arc::new(OllamaProvider::with_model(&router_model, system_name.clone()));

        // Share provider instances when models overlap (saves memory)
        let low_provider: Arc<dyn Provider> = if low_model == router_model {
            classifier.clone()
        } else {
            Arc::new(OllamaProvider::with_model(&low_model, system_name.clone()))
        };

        let medium_provider: Arc<dyn Provider> = Arc::new(OllamaProvider::with_model(&medium_model, system_name.clone()));

        let high_provider: Arc<dyn Provider> = Arc::new(OllamaProvider::with_model(&high_model, system_name.clone()));

        Some(Self {
            classifier,
            low_provider,
            medium_provider,
            high_provider,
            low_model,
            medium_model,
            high_model,
        })
    }

    /// Classify a user message and return the appropriate provider.
    /// Thinking is OFF, JSON mode, designed to be < 1 second on 9b.
    ///
    /// Slash command overrides (checked first, bypass classifier):
    ///   /fast → low (9b)
    ///   /med  → medium (35b)
    ///   /slow → high (122b)
    pub async fn classify(&self, user_message: &str) -> (ComplexityLevel, Arc<dyn Provider>) {
        let lower = user_message.trim().to_lowercase();

        // ── Slash command overrides — user directive always wins ──
        if lower.starts_with("/fast") {
            tracing::info!("[ROUTER] 📊 /fast override → {} (user directive)", self.low_model);
            return (ComplexityLevel::Low, self.low_provider.clone());
        }
        if lower.starts_with("/med") {
            tracing::info!("[ROUTER] 📊 /med override → {} (user directive)", self.medium_model);
            return (ComplexityLevel::Medium, self.medium_provider.clone());
        }
        if lower.starts_with("/slow") {
            tracing::info!("[ROUTER] 📊 /slow override → {} (user directive)", self.high_model);
            return (ComplexityLevel::High, self.high_provider.clone());
        }

        // ── No override — classify with the 9b ──
        // Truncate very long messages for the classifier — it only needs the gist
        let truncated: String = user_message.chars().take(500).collect();
        let prompt = format!("{}{}", ROUTER_PROMPT, truncated);

        // Use a minimal event for the classifier
        let classifier_event = crate::models::message::Event {
            platform: "system:router".into(),
            scope: crate::models::scope::Scope::Private { user_id: "router".into() },
            author_name: "Router".into(),
            author_id: "router".into(),
            content: prompt.clone(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        let result = self.classifier.generate(
            "You are a message complexity classifier. Output only JSON.",
            &[],
            &classifier_event,
            &prompt,
            None,
            None,
        ).await;

        let level = match result {
            Ok(text) => self.parse_level(&text),
            Err(e) => {
                tracing::warn!("[ROUTER] ⚠️ Classification failed: {} — defaulting to medium", e);
                ComplexityLevel::Medium
            }
        };

        let (provider, model_name) = match level {
            ComplexityLevel::Low => (self.low_provider.clone(), &self.low_model),
            ComplexityLevel::Medium => (self.medium_provider.clone(), &self.medium_model),
            ComplexityLevel::High => (self.high_provider.clone(), &self.high_model),
        };

        tracing::info!("[ROUTER] 📊 {} → {} (message: \"{}...\")",
            level, model_name, &truncated.chars().take(60).collect::<String>());

        (level, provider)
    }

    /// Force high-complexity provider (for autonomy mode).
    pub fn force_high(&self) -> Arc<dyn Provider> {
        tracing::info!("[ROUTER] 📊 Autonomy mode → {} (forced high)", self.high_model);
        self.high_provider.clone()
    }

    fn parse_level(&self, raw: &str) -> ComplexityLevel {
        let cleaned = raw.trim();

        // Try to find JSON object
        if let Some(start) = cleaned.find('{') {
            if let Some(end) = cleaned.rfind('}') {
                if end > start {
                    let json_str = &cleaned[start..=end];
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                        if let Some(level) = val.get("level").and_then(|l| l.as_str()) {
                            return match level.to_lowercase().as_str() {
                                "low" => ComplexityLevel::Low,
                                "high" => ComplexityLevel::High,
                                _ => ComplexityLevel::Medium,
                            };
                        }
                    }
                }
            }
        }

        // Fallback: scan for keywords
        let lower = cleaned.to_lowercase();
        if lower.contains("\"low\"") || lower.contains("'low'") {
            ComplexityLevel::Low
        } else if lower.contains("\"high\"") || lower.contains("'high'") {
            ComplexityLevel::High
        } else {
            ComplexityLevel::Medium
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_level_clean_json() {
        let router = make_test_router();
        assert_eq!(router.parse_level(r#"{"level": "low"}"#), ComplexityLevel::Low);
        assert_eq!(router.parse_level(r#"{"level": "medium"}"#), ComplexityLevel::Medium);
        assert_eq!(router.parse_level(r#"{"level": "high"}"#), ComplexityLevel::High);
    }

    #[test]
    fn test_parse_level_markdown_wrapped() {
        let router = make_test_router();
        assert_eq!(router.parse_level("```json\n{\"level\": \"high\"}\n```"), ComplexityLevel::High);
    }

    #[test]
    fn test_parse_level_garbage_defaults_medium() {
        let router = make_test_router();
        assert_eq!(router.parse_level("I cannot classify this"), ComplexityLevel::Medium);
    }

    #[test]
    fn test_parse_level_with_preamble() {
        let router = make_test_router();
        assert_eq!(router.parse_level("Sure! Here's the classification:\n{\"level\": \"low\"}"), ComplexityLevel::Low);
    }

    #[test]
    fn test_complexity_display() {
        assert_eq!(format!("{}", ComplexityLevel::Low), "low");
        assert_eq!(format!("{}", ComplexityLevel::Medium), "medium");
        assert_eq!(format!("{}", ComplexityLevel::High), "high");
    }

    fn make_test_router() -> ReasoningRouter {
        use crate::providers::MockProvider;
        let mock = Arc::new(MockProvider::new());
        ReasoningRouter {
            classifier: mock.clone(),
            low_provider: mock.clone(),
            medium_provider: mock.clone(),
            high_provider: mock.clone(),
            low_model: "test:small".into(),
            medium_model: "test:medium".into(),
            high_model: "test:large".into(),
        }
    }
}
