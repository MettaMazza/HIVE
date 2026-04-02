use crate::models::tool::{ToolResult, ToolStatus};
use crate::agent::preferences::extract_tag;
use crate::providers::Provider;
use crate::models::message::Event;
use crate::models::scope::Scope;
use tokio::sync::mpsc;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use chrono::Utc;
use crate::teacher::GoldenExample;

/// Distillation Mode Tool
/// Autonomously generates high-quality synthetic Q&A pairs on a requested domain
/// and appends them to the Teacher's golden_buffer.jsonl for future SFT/LoRA training.
pub async fn execute_distiller(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
    _provider: Arc<dyn Provider>,
) -> ToolResult {
    let domain = extract_tag(&description, "domain:").unwrap_or_default();
    let num_examples_str = extract_tag(&description, "num_examples:").unwrap_or_else(|| "3".to_string());
    
    let training_enabled = std::env::var("HIVE_TRAINING_ENABLED")
        .unwrap_or_else(|_| "true".to_string());
    if training_enabled.to_lowercase() != "true" {
        let msg = "Error: HIVE_TRAINING_ENABLED is globally set to false. Distillation mode and all synthetic training generation is currently locked.";
        if let Some(ref tx) = telemetry_tx {
            let _ = tx.send(format!("🚪 {}\n", msg)).await;
        }
        return ToolResult {
            task_id,
            output: msg.into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Training Globally Disabled".into()),
        };
    }
    
    if domain.is_empty() {
        return ToolResult {
            task_id,
            output: "Error: Missing domain:[<topic>] tag.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing domain".into()),
        };
    }

    let mut num_examples: usize = num_examples_str.parse().unwrap_or(3);
    // Cap to prevent infinite loops / token exhaustion
    if num_examples > 10 {
        num_examples = 10;
    }
    if num_examples == 0 {
        num_examples = 1;
    }

    // Dynamic Expert Provider Selection:
    // Priority: 1) HIVE_DISTILL_EXPERT env override  2) API keys (Anthropic/OpenAI)
    //           3) Autodetect largest local Ollama model  4) Fall back to HIVE_MODEL
    let explicit_expert = std::env::var("HIVE_DISTILL_EXPERT").ok().filter(|s| !s.is_empty());

    let expert_provider: Box<dyn Provider> = if let Some(ref model) = explicit_expert {
        // Explicit override — use exactly what the user specified
        Box::new(crate::providers::ollama::OllamaProvider::with_model(model, "Distiller".to_string()))
    } else if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        if let Ok(p) = crate::providers::anthropic::AnthropicProvider::new(60, "Distiller".to_string()) {
            Box::new(p)
        } else {
            Box::new(crate::providers::ollama::OllamaProvider::with_model(
                &autodetect_largest_model().await, "Distiller".to_string(),
            ))
        }
    } else if std::env::var("OPENAI_API_KEY").is_ok() {
        if let Ok(p) = crate::providers::openai::OpenAiProvider::new(60, "Distiller".to_string()) {
            Box::new(p)
        } else {
            Box::new(crate::providers::ollama::OllamaProvider::with_model(
                &autodetect_largest_model().await, "Distiller".to_string(),
            ))
        }
    } else {
        Box::new(crate::providers::ollama::OllamaProvider::with_model(
            &autodetect_largest_model().await, "Distiller".to_string(),
        ))
    };

    let expert_model_name = if let Some(ref model) = explicit_expert {
        model.clone()
    } else if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        "Anthropic API".to_string()
    } else if std::env::var("OPENAI_API_KEY").is_ok() {
        "OpenAI API".to_string()
    } else {
        format!("{} (autodetected)", autodetect_largest_model().await)
    };

    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("🧪 Entering Distillation Mode. \n  Expert generating queries: {}\n  Assistant responding: default\nGenerating {} synthetic examples for domain: '{}'...\n", expert_model_name, num_examples, domain)).await;
    }

    let mut success_count = 0;
    let identity_system = crate::prompts::identity::get_persona();
    let golden_path = "memory/teacher/golden_buffer.jsonl";

    for i in 1..=num_examples {
        if let Some(ref tx) = telemetry_tx {
            let _ = tx.send(format!("  ⚙️ Generating example {}/{}...\n", i, num_examples)).await;
        }

        // 1. Generate a complex, rigorous user query
        let query_prompt = format!(
            "You are a Senior Lead Engineer. Write exactly ONE highly technical, complex question or scenario \
             asking an AI assistant for help regarding the domain: '{}'. \
             The question should be detailed, realistic, and demand a structured, expert-level response. \
             Do not include any pleasantries or preamble, just write the raw query.", 
            domain
        );

        let query_event = Event {
            platform: "distiller".into(),
            scope: Scope::Private { user_id: "distiller".into() },
            author_name: "Senior Engineer".into(),
            author_id: "distiller".into(),
            content: query_prompt,
            timestamp: Some(Utc::now().to_rfc3339()),
            message_index: None,
        };

        let synthetic_query = match expert_provider.generate(
            "You are a Senior Engineer generating synthetic data.",
            &[],
            &query_event,
            "",
            None,
            Some(1000), // Max tokens for query
        ).await {
            Ok(q) => q.trim().trim_matches('"').to_string(),
            Err(e) => {
                tracing::warn!("Distiller's expert model failed to generate query: {}", e);
                continue;
            }
        };

        // 2. Generate the ideal Apis assistant response to that query (EXPERT KNOWLEDGE DISTILLATION)
        let response_event = Event {
            platform: "distiller".into(),
            scope: Scope::Private { user_id: "distiller".into() },
            author_name: "User".into(),
            author_id: "user".into(),
            content: format!("Please provide a highly detailed, expert-level answer to the following query:\n\n{}", synthetic_query),
            timestamp: Some(Utc::now().to_rfc3339()),
            message_index: None,
        };

        // Note: Using expert_provider here injects strictly superior, external model knowledge
        // directly into Apis's training loop, resolving the 'cannot learn new data' criticism.
        let synthetic_response = match expert_provider.generate(
            &identity_system,
            &[],
            &response_event,
            "", // No context needed for pure knowledge synthesis
            None,
            Some(3000), // Responses should be comprehensive
        ).await {
            Ok(r) => r.trim().to_string(),
            Err(e) => {
                tracing::warn!("Distiller's expert model failed to generate response: {}", e);
                continue;
            }
        };

        // 3. Construct GoldenExample and append to buffer
        let golden = GoldenExample {
            ts: Utc::now().to_rfc3339(),
            system_prompt: identity_system.clone(),
            user_msg: synthetic_query,
            agent_ctx: String::new(),
            response: synthetic_response,
            tools: vec![], // Pure abstract distillation usually has no tools
            attempts: 1,   // Dictates highest baseline quality score
        };

        if let Ok(json) = serde_json::to_string(&golden) {
            match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(golden_path)
                .await
            {
                Ok(mut file) => {
                    if file.write_all(format!("{}\n", json).as_bytes()).await.is_ok() {
                        success_count += 1;
                    }
                }
                Err(e) => {
                    tracing::error!("Distiller failed to write target buffer: {}", e);
                }
            }
        }
    }

    let result_msg = format!("✅ Distillation complete. {}/{} highly technical examples regarding '{}' were successfully generated and appended to the Teacher golden buffer for the next Sleep Cycle.", success_count, num_examples, domain);
    
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("{}\n", result_msg)).await;
    }

    ToolResult {
        task_id,
        output: result_msg,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

/// Query the local Ollama instance for all available models and return the
/// largest one by file size. This ensures the distiller always uses the most
/// capable local model without requiring hardcoded model names.
/// Falls back to HIVE_MODEL (the main inference model) if Ollama is unreachable.
async fn autodetect_largest_model() -> String {
    let base_url = std::env::var("HIVE_OLLAMA_URL")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());
    let url = format!("{}/api/tags", base_url);

    let fallback = std::env::var("HIVE_MODEL")
        .unwrap_or_else(|_| "qwen3.5:35b".to_string());

    let resp = match reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("[DISTILL] Could not reach Ollama at {} for model autodetection: {}. Falling back to {}", url, e, fallback);
            return fallback;
        }
    };

    let body: serde_json::Value = match resp.json().await {
        Ok(b) => b,
        Err(_) => return fallback,
    };

    // Parse models array, pick the one with the largest size
    let best = body["models"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let name = m["name"].as_str()?;
                    let size = m["size"].as_u64().unwrap_or(0);
                    Some((name.to_string(), size))
                })
                .max_by_key(|(_, size)| *size)
        });

    match best {
        Some((name, size)) => {
            let gb = size as f64 / 1_073_741_824.0;
            tracing::info!("[DISTILL] 🔍 Autodetected largest local model: {} ({:.1} GB)", name, gb);
            name
        }
        None => {
            tracing::warn!("[DISTILL] No models found on Ollama. Falling back to {}", fallback);
            fallback
        }
    }
}
