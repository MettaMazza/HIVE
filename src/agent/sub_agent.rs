#![allow(clippy::should_implement_trait, clippy::too_many_arguments)]
//! Sub-Agent — An independent reasoning entity within the HIVE swarm.
//!
//! Each sub-agent runs a mini ReAct loop with full tool access,
//! its own provider calls, and scoped context. Security follows
//! the same admin/non-admin RBAC as the main Queen ReAct loop.

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use crate::models::message::Event;
use crate::models::scope::Scope;
use crate::providers::Provider;
use crate::memory::MemoryStore;

/// Configuration for spawning a single sub-agent.
#[derive(Debug, Clone)]
pub struct SubAgentSpec {
    /// The task/goal this agent must accomplish
    pub task: String,
    /// Maximum ReAct turns before forced termination (default: 8)
    pub max_turns: u8,
    /// Per-agent timeout in seconds (default: 300)
    pub timeout_secs: u64,
    /// Security scope — inherits caller's scope for RBAC
    pub scope: Scope,
    /// The requesting user's ID — used for admin checks
    pub user_id: String,
    /// 3D Coordinate mapping for Turing Grid segregation
    pub spatial_offset: Option<(i32, i32, i32)>,
    /// How many layers deep this agent is currently running
    pub swarm_depth: u8,
}

impl Default for SubAgentSpec {
    fn default() -> Self {
        Self {
            task: String::new(),
            max_turns: 8,
            timeout_secs: 300,
            scope: Scope::Private { user_id: "agent".into() },
            user_id: "agent".into(),
            spatial_offset: None,
            swarm_depth: 0,
        }
    }
}

/// Execution strategy for spawning multiple sub-agents.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SpawnStrategy {
    /// All agents execute concurrently
    Parallel,
    /// Sequential chain — each agent's output feeds into the next
    Pipeline,
    /// Race — first successful result wins, others are cancelled
    Competitive,
    /// Parallel execution + LLM synthesis of all results
    FanOutFanIn,
}

impl SpawnStrategy {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "pipeline" => Self::Pipeline,
            "competitive" => Self::Competitive,
            "fan_out_fan_in" | "fanoutfanin" => Self::FanOutFanIn,
            _ => Self::Parallel,
        }
    }
}

/// Status of a completed sub-agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubAgentStatus {
    Completed,
    Failed(String),
    TimedOut,
    Cancelled,
}

/// Result returned by a single sub-agent after execution.
#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub agent_id: String,
    pub output: String,
    pub status: SubAgentStatus,
    pub tools_called: Vec<String>,
    pub duration_ms: u64,
    pub turns_used: u8,
}

/// Aggregated result from a spawn operation.
#[derive(Debug, Clone)]
pub struct SpawnResult {
    pub results: Vec<SubAgentResult>,
    pub synthesis: Option<String>,
    pub total_duration_ms: u64,
    pub successful: usize,
    pub total_agents: usize,
}

/// Execute a single sub-agent's mini ReAct loop.
///
/// Each sub-agent gets its own reasoning context and can call any tool
/// available through the standard dispatch pipeline. The agent reasons
/// about its task, calls tools, observes results, and produces a final
/// output — all independently from the Queen.
#[cfg(not(tarpaulin_include))]
pub async fn execute_sub_agent(
    agent_id: String,
    spec: SubAgentSpec,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
    telemetry_tx: mpsc::Sender<String>,
    agent_manager: Arc<crate::agent::AgentManager>,
    capabilities: Arc<crate::models::capabilities::AgentCapabilities>,
    pipeline_context: Option<String>,
) -> SubAgentResult {
    let start = std::time::Instant::now();
    let mut tools_called: Vec<String> = vec![];
    let mut current_turn: u8 = 0;

    let _ = telemetry_tx.send(format!(
        "🐝 **[{}]** Spawned — Task: {}",
        agent_id,
        if spec.task.len() > 100 { &spec.task[..100] } else { &spec.task }
    )).await;

    // Build system prompt for this sub-agent
    let offset_msg = if let Some((x, y, z)) = spec.spatial_offset {
        format!("\n[SPATIAL SWARM ASSIGNMENT]\nYour physical workspace is anchored at Turing Grid coordinates [{}, {}, {}]. Navigate to these exact coordinates using `turing_move` before writing any pipeline scripts. Do not overwrite memory outside your sector.\n", x, y, z)
    } else {
        String::new()
    };
    
    let system_prompt = format!(
        "You are a Sub-Agent within the HIVE swarm. Your ID is '{agent_id}'.\n\
         You have been delegated a specific task by the Queen (main agent).\n\
         You have full access to all tools. Execute your task efficiently.\n\n\
         RULES:\n\
         1. Focus ONLY on your assigned task. Do not deviate.\n\
         2. Use tools as needed — you have the same access as the main agent.\n\
         3. When your task is complete, use `reply_to_request` with your findings.\n\
         4. Be concise but thorough in your final output.\n\
         5. If a tool fails, note it and work around it.\n{offset}\n\
         YOUR TASK:\n{task}\n",
        agent_id = agent_id,
        offset = offset_msg,
        task = spec.task,
    );

    // Inject pipeline context from previous agent if in pipeline mode
    let is_autonomy = spec.user_id == "apis_autonomy";
    let tool_list = agent_manager.get_available_tools_text_for_platform("agent", is_autonomy);
    let full_system = format!(
        "{}\n\n{}",
        system_prompt,
        crate::agent::planner::REACT_AGENT_PROMPT.replace("{available_tools}", &tool_list)
    );

    let dummy_event = Event {
        platform: "agent".into(),
        scope: spec.scope.clone(),
        author_name: format!("SubAgent:{}", agent_id),
        author_id: spec.user_id.clone(),
        content: spec.task.clone(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
    };

    let mut context = String::new();
    if let Some(ref pc) = pipeline_context {
        context.push_str(&format!("[CONTEXT FROM PREVIOUS AGENT]\n{}\n\n", pc));
    }

    let timeout = tokio::time::Duration::from_secs(spec.timeout_secs);
    let tools_called_fallback = tools_called.clone();
    let result = tokio::time::timeout(timeout, async {
        loop {
            if current_turn >= spec.max_turns {
                tracing::warn!("[SUB-AGENT:{}] Hit max turns ({}), forcing completion", agent_id, spec.max_turns);
                break;
            }
            current_turn += 1;

            context.push_str(&format!("\n\nSub-Agent ReAct Turn {}\n", current_turn));

            let _ = telemetry_tx.send(format!(
                "🔬 **[{}]** Turn {}/{}",
                agent_id, current_turn, spec.max_turns
            )).await;

            // Generate plan from provider
            let candidate = match provider.generate(
                &full_system, &[], &dummy_event, &context,
                Some(telemetry_tx.clone()), None,
            ).await {
                Ok(text) => text,
                Err(e) => {
                    tracing::error!("[SUB-AGENT:{}] Provider error: {:?}", agent_id, e);
                    context.push_str(&format!("Turn {} - Provider Error: {:?}\n", current_turn, e));
                    continue;
                }
            };

            // Parse the plan
            let cleaned = crate::engine::repair::repair_planner_json(&candidate);
            let plan = match serde_json::from_str::<crate::agent::planner::AgentPlan>(&cleaned) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("[SUB-AGENT:{}] JSON parse failed turn {}: {}", agent_id, current_turn, e);
                    context.push_str(&format!(
                        "Turn {} - [SYSTEM ERROR] Your output was not valid JSON. Output EXACTLY one JSON block.\n\n",
                        current_turn
                    ));
                    continue;
                }
            };

            context.push_str(&format!("Turn {} Agent:\n{}\n", current_turn, candidate.trim()));

            // Check for reply_to_request — that means the sub-agent is done
            let mut reply_task = None;
            let mut standard_tasks = vec![];

            for t in plan.tasks {
                if t.tool_type == "reply_to_request" || t.tool_type == "refuse_request" {
                    reply_task = Some(t);
                } else if t.tool_type != "emoji_react" {
                    standard_tasks.push(t);
                }
            }

            // Execute standard tools
            if !standard_tasks.is_empty() {
                // SECURITY GATE: same admin/non-admin check as the main ReAct loop
                let mut safe_tasks = vec![];
                let is_admin = capabilities.admin_users.contains(&spec.user_id);

                for task in &standard_tasks {
                    if capabilities.admin_tools.contains(&task.tool_type) && !is_admin {
                        tracing::warn!("[SUB-AGENT:{}] 🛡️ SECURITY: Non-admin blocked from '{}'", agent_id, task.tool_type);
                        context.push_str(&format!(
                            "Turn {} - Task {}: SECURITY VIOLATION — {} requires admin privileges.\n\n",
                            current_turn, task.task_id, task.tool_type
                        ));
                    } else {
                        safe_tasks.push(task.clone());
                    }
                }

                let safe_plan = crate::agent::planner::AgentPlan {
                    thought: plan.thought.clone(),
                    tasks: safe_tasks,
                };
                
                let pass_context = format!("[SWARM_DEPTH:{}] {}", spec.swarm_depth, spec.task);

                let tool_results = agent_manager.execute_plan(
                    safe_plan,
                    &pass_context,
                    spec.scope.clone(),
                    Some(telemetry_tx.clone()),
                    Some(agent_manager.clone()),
                    Some(capabilities.clone()),
                    None,
                ).await;

                for res in &tool_results {
                    tools_called.push(res.task_id.clone());
                    let display = if res.output.len() > 16000 {
                        format!("{}...[truncated, {} bytes total]", &res.output[..16000], res.output.len())
                    } else {
                        res.output.clone()
                    };
                    context.push_str(&format!(
                        "Turn {} - Task {}: {:?}\nOutput: {}\n\n",
                        current_turn, res.task_id, res.status, display
                    ));
                }

                let _ = telemetry_tx.send(format!(
                    "✅ **[{}]** Turn {} — {} tools executed",
                    agent_id, current_turn, tool_results.len()
                )).await;

                // If reply was also in this turn, defer it (same logic as Queen)
                if reply_task.is_some() {
                    context.push_str(&format!(
                        "Turn {} - [SYSTEM: Reply deferred — tools also ran. Write a new reply with results.]\n\n",
                        current_turn
                    ));
                    continue;
                }
            }

            // Handle reply — sub-agent is done
            if let Some(reply) = reply_task {
                let _ = telemetry_tx.send(format!(
                    "✅ **[{}]** Complete — {} turns, {} tools",
                    agent_id, current_turn, tools_called.len()
                )).await;

                return SubAgentResult {
                    agent_id: agent_id.clone(),
                    output: reply.description,
                    status: SubAgentStatus::Completed,
                    tools_called,
                    duration_ms: start.elapsed().as_millis() as u64,
                    turns_used: current_turn,
                };
            }
        }

        // Max turns exceeded — return whatever context we have
        let fatal_event = Event {
            platform: "internal".into(),
            scope: spec.scope.clone(),
            author_name: "Swarm Watchdog".into(),
            author_id: "system".into(),
            content: format!("[SWARM FATAL] Agent '{}' hit max turns ({}).", agent_id, spec.max_turns),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };
        memory.add_event(fatal_event).await;

        SubAgentResult {
            agent_id: agent_id.clone(),
            output: format!("[Sub-agent hit max turns ({}). Partial context available.]", spec.max_turns),
            status: SubAgentStatus::Failed("Max turns exceeded".into()),
            tools_called,
            duration_ms: start.elapsed().as_millis() as u64,
            turns_used: current_turn,
        }
    }).await;

    match result {
        Ok(r) => r,
        Err(_) => {
            let _ = telemetry_tx.send(format!(
                "⏱️ **[{}]** Timed out after {}s",
                agent_id, spec.timeout_secs
            )).await;
            
            let fatal_event = Event {
                platform: "internal".into(),
                scope: spec.scope.clone(),
                author_name: "Swarm Watchdog".into(),
                author_id: "system".into(),
                content: format!("[SWARM FATAL] Agent '{}' Timed Out after {}s.", agent_id, spec.timeout_secs),
                timestamp: Some(chrono::Utc::now().to_rfc3339()),
                message_index: None,
            };
            memory.add_event(fatal_event).await;

            SubAgentResult {
                agent_id,
                output: format!("Sub-agent timed out after {} seconds.", spec.timeout_secs),
                status: SubAgentStatus::TimedOut,
                tools_called: tools_called_fallback,
                duration_ms: start.elapsed().as_millis() as u64,
                turns_used: current_turn,
            }
        }
    }
}


#[cfg(test)]
#[path = "sub_agent_tests.rs"]
mod tests;
