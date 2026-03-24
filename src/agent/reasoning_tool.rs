use crate::models::tool::{ToolResult, ToolStatus};
use crate::models::scope::Scope;
use crate::memory::MemoryStore;
use std::sync::Arc;
use tokio::sync::mpsc;
use crate::agent::preferences::extract_tag;
use tokio::io::{AsyncBufReadExt, BufReader};

pub async fn execute_review_reasoning(
    task_id: String,
    desc: String,
    _memory: Arc<MemoryStore>,
    scope: Scope,
    telemetry_tx: Option<mpsc::Sender<String>>,
    agent_mgr: Option<Arc<crate::agent::AgentManager>>,
    provider: Option<Arc<dyn crate::providers::Provider>>,
    capabilities: Option<Arc<crate::models::capabilities::AgentCapabilities>>,
    drives: Option<Arc<crate::engine::drives::DriveSystem>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("🧠 Native Reasoning Review Tool executing...\n")).await;
    }
    tracing::debug!("[AGENT:reasoning] ▶ task_id={}", task_id);
    
    // Settling uncertainty: Deep reasoning significantly reduces DriveState entropy
    if let Some(d) = drives {
        d.modify_drive("uncertainty", -30.0).await;
    }

    // Parse parameters
    let mut limit: usize = 5;
    if let Some(turns_str) = desc.split("turns_ago:[").nth(1)
        && let Some(num_str) = turns_str.split("]").next()
            && let Ok(num) = num_str.parse::<usize>() {
                limit = num;
            }
    if let Some(limit_str) = desc.split("limit:[").nth(1)
        && let Some(num_str) = limit_str.split("]").next()
            && let Ok(num) = num_str.parse::<usize>() {
                limit = num;
            }

    // Resolve the timeline file path from the current scope
    let timeline_path = match &scope {
        Scope::Public { channel_id, user_id } => {
            std::path::PathBuf::from(format!("memory/public_{}/{}/timeline.jsonl", channel_id, user_id))
        }
        Scope::Private { user_id } => {
            std::path::PathBuf::from(format!("memory/private_{}/timeline.jsonl", user_id))
        }
    };

    // Read the persistent timeline file and extract only internal reasoning traces
    let file = match tokio::fs::File::open(&timeline_path).await {
        Ok(f) => f,
        Err(_) => {
            return ToolResult {
                task_id,
                output: "No persistent timeline found — no reasoning traces available.".to_string(),
                tokens_used: 0,
                status: ToolStatus::Success,
            };
        }
    };

    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut all_traces = Vec::new();

    while let Ok(Some(line)) = lines.next_line().await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line)
            && json["author_id"].as_str() == Some("internal")
                && let Some(content) = json["content"].as_str() {
                    all_traces.push(content.to_string());
                }
    }

    if all_traces.is_empty() {
        return ToolResult {
            task_id,
            output: "No reasoning traces found in the persistent timeline.".to_string(),
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    // Return the most recent N traces (from the end of the file)
    let start = if all_traces.len() > limit { all_traces.len() - limit } else { 0 };
    let slice = &all_traces[start..];

    // Check if Swarm Validation is requested
    let validate = extract_tag(&desc, "validate:").unwrap_or_default() == "true";
    if validate {
        if let (Some(am), Some(prov), Some(caps)) = (agent_mgr, provider, capabilities) {
            if let Some(ref tx) = telemetry_tx {
                let _ = tx.send("🐝 Spawning isolated Competitive Swarm to validate reasoning traces...\n".to_string()).await;
            }
            let user_id = match &scope { Scope::Public { user_id, .. } => user_id.clone(), Scope::Private { user_id } => user_id.clone() };
            
            // Build Context
            let context = slice.join("\n\n");
            let base_task = format!("Review these recent reasoning traces:\n\n{}\n\n", context);
            
            let specs = vec![
                crate::agent::sub_agent::SubAgentSpec {
                    task: format!("{}Analyze the reasoning history. Are there any logical fallacies or critically missed context? Provide a strict Yes/No and why.", base_task),
                    max_turns: 4,
                    timeout_secs: 180,
                    scope: scope.clone(),
                    user_id: user_id.clone(),
                    spatial_offset: Some((800, 800, 800)),
                    swarm_depth: 0,
                },
                crate::agent::sub_agent::SubAgentSpec {
                    task: format!("{}Play devil's advocate to the recent reasoning. Find edge cases where the logic absolutely fails. Provide a strict Yes/No on validity.", base_task),
                    max_turns: 4,
                    timeout_secs: 180,
                    scope: scope.clone(),
                    user_id: user_id.clone(),
                    spatial_offset: Some((800, 900, 800)),
                    swarm_depth: 0,
                },
                crate::agent::sub_agent::SubAgentSpec {
                    task: format!("{}Synthesize the logic from an external perspective. Is it perfectly sound? Provide a strict Yes/No on validity.", base_task),
                    max_turns: 4,
                    timeout_secs: 180,
                    scope: scope.clone(),
                    user_id: user_id.clone(),
                    spatial_offset: Some((800, 1000, 800)),
                    swarm_depth: 0,
                }
            ];

            let tx_for_spawn = telemetry_tx.clone().unwrap_or_else(|| {
                let (tx, _) = tokio::sync::mpsc::channel(1);
                tx
            });

            let swarm_result = crate::agent::spawner::spawn_agents(
                specs,
                crate::agent::sub_agent::SpawnStrategy::Competitive,
                prov,
                _memory.clone(),
                tx_for_spawn,
                am,
                caps
            ).await;
            
            let mut out = format!("--- SWARM VALIDATION VERDICT ({:.1}s) ---\n\n", swarm_result.total_duration_ms as f64 / 1000.0);
            for r in swarm_result.results {
                if r.status == crate::agent::sub_agent::SubAgentStatus::Completed {
                    out.push_str(&format!("Result from {}:\n{}\n\n", r.agent_id, r.output));
                }
            }
            return ToolResult { task_id, output: out, tokens_used: 0, status: ToolStatus::Success };
        }
    }

    let mut out = String::new();
    for (i, trace) in slice.iter().enumerate() {
        out.push_str(&format!("--- REASONING TRACE {} of {} ---\n{}\n\n", start + i + 1, all_traces.len(), trace));
    }

    ToolResult {
        task_id,
        output: out,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}
