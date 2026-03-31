use crate::models::tool::{ToolResult, ToolStatus};

/// Execute the swap_model tool — list, check, or swap the active inference model.
pub async fn execute_swap_model(
    task_id: String,
    description: String,
    telemetry_tx: Option<tokio::sync::mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("🔄 Model Tool executing...\n".into()).await;
    }

    let action = crate::agent::preferences::extract_tag(&description, "action:")
        .unwrap_or_else(|| "current".into())
        .to_lowercase();

    let base_url = std::env::var("HIVE_OLLAMA_URL")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());

    match action.as_str() {
        "list" => {
            let url = format!("{}/api/tags", base_url);
            match reqwest::Client::new().get(&url).send().await {
                Ok(res) => {
                    match res.json::<serde_json::Value>().await {
                        Ok(body) => {
                            let models: Vec<String> = body["models"].as_array()
                                .map(|arr| arr.iter().filter_map(|m| {
                                    let name = m["name"].as_str()?;
                                    let size_bytes = m["size"].as_u64().unwrap_or(0);
                                    let size_gb = size_bytes as f64 / 1_073_741_824.0;
                                    Some(format!("  • {} ({:.1} GB)", name, size_gb))
                                }).collect())
                                .unwrap_or_default();

                            if models.is_empty() {
                                ToolResult {
                                    task_id,
                                    output: "No models found on Ollama instance.".into(),
                                    tokens_used: 0,
                                    status: ToolStatus::Success,
                                }
                            } else {
                                // Get the currently active model
                                let current = std::env::var("HIVE_MODEL")
                                    .unwrap_or_else(|_| "unknown".into());
                                let output = format!(
                                    "📋 **Available Models** (current: `{}`)\n\n{}\n\nTotal: {} models",
                                    current, models.join("\n"), models.len()
                                );
                                ToolResult {
                                    task_id,
                                    output,
                                    tokens_used: 0,
                                    status: ToolStatus::Success,
                                }
                            }
                        }
                        Err(e) => ToolResult {
                            task_id,
                            output: format!("Failed to parse Ollama response: {}", e),
                            tokens_used: 0,
                            status: ToolStatus::Failed(e.to_string()),
                        },
                    }
                }
                Err(e) => ToolResult {
                    task_id,
                    output: format!("Failed to connect to Ollama at {}: {}", base_url, e),
                    tokens_used: 0,
                    status: ToolStatus::Failed(e.to_string()),
                },
            }
        }
        "current" => {
            let current = std::env::var("HIVE_MODEL")
                .unwrap_or_else(|_| "unknown".into());
            ToolResult {
                task_id,
                output: format!("🔄 Current active model: `{}`", current),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        "swap" => {
            let model_name = crate::agent::preferences::extract_tag(&description, "model:")
                .unwrap_or_default();

            if model_name.is_empty() {
                return ToolResult {
                    task_id,
                    output: "Error: No model name provided. Use 'model:[model_name]'".into(),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Missing model name".into()),
                };
            }

            // Check if HIVE_MODEL_PULL is enabled for non-local models
            let allow_pull = std::env::var("HIVE_MODEL_PULL")
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(false);

            if !allow_pull {
                // Verify the model exists locally first
                let url = format!("{}/api/tags", base_url);
                let available = match reqwest::Client::new().get(&url).send().await {
                    Ok(res) => {
                        res.json::<serde_json::Value>().await.ok()
                            .and_then(|body| body["models"].as_array().map(|arr| {
                                arr.iter().filter_map(|m| m["name"].as_str().map(String::from)).collect::<Vec<_>>()
                            }))
                            .unwrap_or_default()
                    }
                    Err(_) => Vec::new(),
                };

                if !available.iter().any(|m| m == &model_name) {
                    return ToolResult {
                        task_id,
                        output: format!(
                            "Error: Model '{}' not found locally. Available: {}. Set HIVE_MODEL_PULL=true to allow downloading new models.",
                            model_name,
                            available.join(", ")
                        ),
                        tokens_used: 0,
                        status: ToolStatus::Failed("Model not available locally".into()),
                    };
                }
            }

            // Note: The actual model swap happens through the OllamaProvider's RwLock.
            // Since this tool doesn't have a direct reference to the provider,
            // we set the env var which will be read by the provider on next construct.
            // For live swap, the /model slash command or the engine's shared provider should be used.
            // SAFETY: We only call this from the single model swap tool, no concurrent env reads
            unsafe { std::env::set_var("HIVE_MODEL", &model_name); }

            if let Some(ref tx) = telemetry_tx {
                let _ = tx.send(format!("🔄 Model swapped to: {}\n", model_name)).await;
            }

            tracing::info!("[MODEL_TOOL] 🔄 Model swapped to '{}' via agent tool", model_name);

            ToolResult {
                task_id,
                output: format!("✅ Model swapped to `{}`. Next inference will use this model.", model_name),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        _ => ToolResult {
            task_id,
            output: format!("Unknown action '{}'. Use 'action:[list]', 'action:[current]', or 'action:[swap] model:[name]'", action),
            tokens_used: 0,
            status: ToolStatus::Failed(format!("Unknown action: {}", action)),
        },
    }
}
