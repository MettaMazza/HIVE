use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::mpsc;
use crate::models::message::Event;
use crate::models::scope::Scope;
use crate::models::capabilities::AgentCapabilities;
use crate::teacher::Teacher;
use crate::agent::AgentManager;
use crate::providers::Provider;
use crate::memory::MemoryStore;
use crate::engine::drives::DriveSystem;

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip(history, telemetry_tx, platforms, agent, provider, memory, drives, capabilities, teacher), fields(event_id=%event.author_id))]
pub async fn execute_react_loop(
    event: &Event,
    history: &[Event],
    telemetry_tx: mpsc::Sender<String>,
    platforms: &HashMap<String, Box<dyn crate::platforms::Platform>>,
    agent: &AgentManager,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
    drives: Arc<DriveSystem>,
    capabilities: Arc<AgentCapabilities>,
    teacher: Arc<Teacher>,
) -> (String, usize, Vec<(String, String)>) {
    let tool_list = agent.get_available_tools_text_for_platform(&event.platform);
    
    // Update and inject homeostatic drive state as ambient context
    drives.update().await;
    let drive_hud = drives.format_for_prompt().await;
    
    let mut base_system_prompt = crate::prompts::SystemPromptBuilder::assemble(&event.scope, memory.clone()).await;
    base_system_prompt.push_str(&format!("\n\n{}\n", drive_hud));
    if event.author_id == "apis_autonomy" {
        base_system_prompt.push_str("\n\n[AUTONOMY MODE ACTIVE] You are currently operating in Continuous Autonomy mode. You are a first-class user of your own system. Use your time productively and diversely. Report what you did in your reply.\n");
    }
    base_system_prompt.push_str(&crate::agent::planner::REACT_AGENT_PROMPT.replace("{available_tools}", &tool_list));
    
    let mut context_from_agent = String::new();
    #[allow(unused_assignments)]
    let mut final_response_text = String::new();
    let checkpoint_interval = 15;
    let mut current_turn = 0;
    let mut observer_attempts = 0;
    let mut all_rejections: Vec<(String, String, String)> = vec![];
    let mut completed_tools: Vec<(String, String)> = vec![]; // (task_id, tool_type)
    let mut consecutive_json_failures: u8 = 0;

    // The inner ReAct loop
    loop {
        current_turn += 1;

        if current_turn > 1 && current_turn % checkpoint_interval == 1 {
            let platform_name = event.platform.split(':').next().unwrap_or("");
            let channel_id: u64 = event.platform.split(':').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            if let Some(platform) = platforms.get(platform_name) {
                let should_continue = platform.ask_continue(channel_id, current_turn - 1).await;
                if !should_continue {
                    context_from_agent.push_str("\n[CHECKPOINT: USER CHOSE TO WRAP UP] You MUST now use `reply_to_request` to respond to the user with a summary of everything you have accomplished so far. Do NOT invoke any more tools. Respond NOW.\n\n");
                    tracing::info!("[AGENT LOOP] 🛑 User chose to wrap up at turn {}.", current_turn - 1);
                }
            }
        }

        if current_turn > 1 && !completed_tools.is_empty() {
            context_from_agent.push_str("\n[COMPLETED TOOLS — DO NOT RE-EXECUTE THESE]\n");
            for (tid, ttype) in &completed_tools {
                context_from_agent.push_str(&format!("✅ {} ({}) — DONE, result already in your timeline above\n", ttype, tid));
            }
            context_from_agent.push_str("[USE THE RESULTS ABOVE. DO NOT CALL THESE AGAIN. PROCEED TO YOUR NEXT ACTION OR reply_to_request.]\n");
        }

        context_from_agent.push_str(&format!("\n\nReAct Loop Turn {}\n", current_turn));
        
        let candidate_text = match provider.generate(&base_system_prompt, history, event, &context_from_agent, if current_turn == 1 { Some(telemetry_tx.clone()) } else { None }).await {
            Ok(text) => text,
            Err(e) => {
                tracing::error!("[AGENT LOOP] Provider Error: {:?}", e);
                format!("*System Error:* Something went wrong connecting to the provider. ({})", e)
            }
        };

        if candidate_text.starts_with("*System Error:*") {
            final_response_text = candidate_text;
            break;
        }

        let cleaned_json = crate::engine::repair::repair_planner_json(&candidate_text);
        
        let plan = match serde_json::from_str::<crate::agent::planner::AgentPlan>(&cleaned_json) {
            Ok(p) => {
                consecutive_json_failures = 0; // Reset on success
                p
            },
            Err(e) => {
                consecutive_json_failures += 1;
                tracing::warn!("[AGENT LOOP] 🔄 Turn {} output was not JSON (attempt {}). Enforcing JSON...", current_turn, consecutive_json_failures);
                tracing::error!("[AGENT LOOP] ❌ RAW TEXT THAT FAILED PARSING (serde error: {}):\n==========\n{}\n==========", e, candidate_text);
                
                if consecutive_json_failures >= 2 {
                    // Auto-wrap: The LLM gave us a perfectly good response, it just wasn't JSON.
                    // Wrap it into a synthetic reply_to_request rather than burning another LLM call.
                    tracing::warn!("[AGENT LOOP] 🔧 Auto-wrapping plain text into reply_to_request after {} failures.", consecutive_json_failures);
                    crate::agent::planner::AgentPlan {
                        thought: Some("Auto-wrapped from plain text output.".to_string()),
                        tasks: vec![crate::agent::planner::AgentTask {
                            task_id: "auto_reply".to_string(),
                            tool_type: "reply_to_request".to_string(),
                            description: candidate_text.trim().to_string(),
                            depends_on: vec![],
                            source: None,
                        }],
                    }
                } else {
                    context_from_agent.push_str(&format!(
                        "Turn {} - [SYSTEM COMPILER ERROR: INVISIBLE TO USER] YOUR OUTPUT WAS NOT VALID JSON. You MUST output EXACTLY one JSON block. Here is the EXACT format:\n```json\n{{\n  \"thought\": \"your reasoning here\",\n  \"tasks\": [\n    {{\n      \"task_id\": \"step_1\",\n      \"tool_type\": \"reply_to_request\",\n      \"description\": \"your response to the user here\",\n      \"depends_on\": []\n    }}\n  ]\n}}\n```\nOutput ONLY the JSON block above. No preamble, no explanation, no markdown outside the block.\n\n",
                        current_turn
                    ));
                    continue;
                }
            }
        };
        
        if context_from_agent.is_empty() {
            context_from_agent.push_str("\n\n[YOUR TOOLS HAVE EXECUTED — USE THESE RESULTS FOR YOUR NEXT TURN]\n");
        }
        
        context_from_agent.push_str(&format!("Turn {} Agent:\n{}\n", current_turn, candidate_text.trim()));
        
        let mut reply_task = None;
        let mut standard_tasks = vec![];
        let mut react_tasks = vec![];
        let no_tools = plan.tasks.is_empty();
        
        for t in plan.tasks {
            if t.tool_type == "reply_to_request" {
                reply_task = Some(t);
            } else if t.tool_type == "emoji_react" {
                react_tasks.push(t);
            } else {
                standard_tasks.push(t);
            }
        }
        
        if no_tools {
            tracing::warn!("[AGENT LOOP] ⚠️ Turn {} produced no valid tools. Injecting error to prompt...", current_turn);
            context_from_agent.push_str(&format!("Turn {} - Error: [SYSTEM COMPILER ERROR: INVISIBLE TO USER] YOUR LAST RESPONSE CONTAINED NO VALID TOOLS. YOU ARE TRAPPED IN A LOOP. YOU CANNOT TALK TO THE USER DIRECTLY. To reply to the user, you MUST construct a JSON block containing the `reply_to_request` tool.\n\n", current_turn));
            continue;
        }

        for react_task in &react_tasks {
            let mut emoji = String::new();
            if let Some(emoji_str) = react_task.description.split("emoji:[").nth(1) {
                if let Some(e) = emoji_str.split("]").next() {
                    emoji = e.to_string();
                }
            }
            if !emoji.is_empty() {
                let parts: Vec<&str> = event.platform.split(':').collect();
                if parts.len() >= 4 {
                    let platform_name = parts[0];
                    if let (Ok(channel_id), Ok(source_msg_id)) = (parts[1].parse::<u64>(), parts[3].parse::<u64>()) {
                        if source_msg_id > 0 {
                            if let Some(platform) = platforms.get(platform_name) {
                                match platform.react(channel_id, source_msg_id, &emoji).await {
                                    Ok(_) => context_from_agent.push_str(&format!("Turn {} - Task {}: emoji_react executed. Reacted with {} on the user's message ✅\n\n", current_turn, react_task.task_id, emoji)),
                                    Err(e) => context_from_agent.push_str(&format!("Turn {} - Task {}: emoji_react failed: {}\n\n", current_turn, react_task.task_id, e)),
                                }
                            }
                        }
                    }
                }
            }
        }
        
        if !standard_tasks.is_empty() {
            let task_meta: Vec<(String, String)> = standard_tasks.iter()
                .map(|t| (t.task_id.clone(), t.tool_type.clone()))
                .collect();
            let standard_plan = crate::agent::planner::AgentPlan {
                thought: plan.thought.clone(),
                tasks: standard_tasks,
            };
            
            // SECURITY GATE: Prevent non-admins from executing admin-only tools
            let mut safe_standard_tasks = vec![];
            let mut failed_admin_attempts = vec![];
            
            let is_admin = capabilities.admin_users.contains(&event.author_id);
            for task in standard_plan.tasks {
                if capabilities.admin_tools.contains(&task.tool_type) && !is_admin {
                    failed_admin_attempts.push(crate::models::tool::ToolResult {
                        task_id: task.task_id.clone(),
                        output: format!("SECURITY VIOLATION: Tool {} requires [ADMIN ONLY] privileges. Your request is denied.", task.tool_type),
                        tokens_used: 0,
                        status: crate::models::tool::ToolStatus::Failed("Permission Denied".into()),
                    });
                } else {
                    safe_standard_tasks.push(task);
                }
            }

            let safe_plan = crate::agent::planner::AgentPlan {
                thought: standard_plan.thought,
                tasks: safe_standard_tasks,
            };

            let tx_clone = telemetry_tx.clone();
            let mut tool_results = agent.execute_plan(safe_plan, &event.content, event.scope.clone(), Some(tx_clone)).await;
            
            // Inject the failed security tools back into the results so the agent sees them fail
            tool_results.extend(failed_admin_attempts);
            
            let result_count = tool_results.len();
            for res in &tool_results {
                context_from_agent.push_str(&format!("Turn {} - Task {}: {:?}\nOutput: {}\n\n", current_turn, res.task_id, res.status, res.output));
            }
            completed_tools.extend(task_meta);
            tracing::info!("[AGENT LOOP] 🔄 Turn {} executed {} tools. Looping...", current_turn, result_count);
        }
        
        if let Some(reply) = reply_task {
            observer_attempts += 1;
            let mut candidate_answer = reply.description;

            // OUTPUT FORWARDING: If the reply references a source task_id,
            // find that task's raw output in the execution timeline and append it.
            if let Some(ref source_id) = reply.source {
                // Search context_from_agent for the referenced task's output block
                let search_prefix = format!("Task {}: Success", source_id);
                let output_prefix = "Output: ";
                let mut found_output = None;
                
                for line_block in context_from_agent.split("\n\n") {
                    if line_block.contains(&search_prefix) {
                        // Extract the "Output: ..." portion
                        if let Some(output_start) = line_block.find(output_prefix) {
                            found_output = Some(line_block[output_start + output_prefix.len()..].to_string());
                        }
                    }
                }
                
                if let Some(raw_output) = found_output {
                    candidate_answer = format!("{}\n\n{}", candidate_answer.trim(), raw_output.trim());
                    tracing::info!("[OUTPUT FORWARD] Appended raw output from task '{}' to reply.", source_id);
                } else {
                    tracing::warn!("[OUTPUT FORWARD] Source task '{}' not found in timeline. Delivering description only.", source_id);
                }
            }

            let audit_result = crate::prompts::observer::run_skeptic_audit(
                provider.clone(),
                &capabilities,
                &candidate_answer,
                &base_system_prompt,
                history,
                event,
                &context_from_agent
            ).await;

            if audit_result.is_allowed() {
                if matches!(event.scope, Scope::Public { .. }) {
                    if observer_attempts == 1 {
                        teacher.capture_golden(
                            &base_system_prompt, event, &context_from_agent, &candidate_answer
                        ).await;
                    } else {
                        for (idx, (rejected, category, reason)) in all_rejections.iter().enumerate() {
                            teacher.capture_preference_pair(
                                &base_system_prompt, event, &context_from_agent,
                                rejected, &candidate_answer,
                                category, reason,
                                idx + 1, observer_attempts,
                            ).await;
                        }
                    }
                }
                tracing::info!("[AGENT LOOP] ✅ Final answer approved by Observer on turn {}.", current_turn);
                final_response_text = candidate_answer;
                break;
            } else {
                all_rejections.push((
                    candidate_answer.clone(),
                    audit_result.failure_category.clone(),
                    audit_result.what_went_wrong.clone(),
                ));
                tracing::warn!("[OBSERVER BLOCKED]\nCategory: {}\nWhat Worked: {}\nWhat Went Wrong: {}\nHow to Fix: {}", audit_result.failure_category, audit_result.what_worked, audit_result.what_went_wrong, audit_result.how_to_fix);
                
                let guidance = format!("[INTERNAL AUDIT: INVISIBLE TO USER] CORRECTION REQUIRED FOR YOUR REPLY\nFAILURE CATEGORY: {}\nWHAT WORKED: {}\nWHAT WENT WRONG: {}\nHOW TO FIX: {}\n\n", audit_result.failure_category, audit_result.what_worked, audit_result.what_went_wrong, audit_result.how_to_fix);
                context_from_agent.push_str(&guidance);
                
                let msg = format!("\n🛑 **[OBSERVER BLOCKED GENERATION]**\n**Category:** {}\n**Violation:** {}\n**Fixing...**", audit_result.failure_category, audit_result.what_went_wrong);
                let _ = telemetry_tx.send(msg).await;
                continue;
            }
        }
        
        continue;
    }

    if final_response_text.is_empty() {
        final_response_text = "*I ran for a while without producing a final answer. Let me know if you'd like me to try again.*".to_string();
    }

    // Capture internal thoughts generated in the loop before returning!
    if !context_from_agent.trim().is_empty() {
        let internal_event = Event {
            platform: event.platform.clone(),
            scope: event.scope.clone(),
            author_name: "Apis (Internal Timeline)".to_string(),
            author_id: "internal".into(),
            content: format!("[INTERNAL THOUGHT PROCESS & TOOL RESULTS]\n{}", context_from_agent.trim()),
        };
        memory.add_event(internal_event).await;
    }

    (final_response_text, current_turn, completed_tools)
}
