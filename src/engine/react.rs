use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
    agent: &Arc<AgentManager>,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
    drives: Arc<DriveSystem>,
    capabilities: Arc<AgentCapabilities>,
    teacher: Arc<Teacher>,
    stop_flag: Arc<AtomicBool>,
) -> (String, usize, Vec<(String, String)>) {
    let is_autonomy = event.author_id == "apis_autonomy";
    let tool_list = agent.get_available_tools_text_for_platform(&event.platform, is_autonomy);
    
    // Update and inject homeostatic drive state as ambient context
    drives.update().await;

    // Boost social_connection on incoming human engagement — scaled by message
    // length (depth of thought) and conversation history (sustained engagement).
    // Without this, social_connection only decays and never recovers from user interaction.
    if !is_autonomy && event.author_id != "system_resume" {
        let msg_len = event.content.len();
        let base_boost = if msg_len > 500 {
            20.0  // Substantial engagement (long message, attachments, deep conversation)
        } else if msg_len > 100 {
            12.0  // Normal conversation
        } else {
            5.0   // Quick reply / emoji / short message
        };
        let engagement_multiplier = if history.len() > 10 { 1.5 } else if history.len() > 5 { 1.2 } else { 1.0 };
        let final_boost = base_boost * engagement_multiplier;
        drives.modify_drive("social_connection", final_boost).await;
    }

    let drive_hud = drives.format_for_prompt().await;
    
    // Inject active goals summary
    let goal_hud = if let Some(ref gs) = agent.goal_store {
        let tree = gs.get_tree(&event.scope).await;
        let formatted = tree.format_for_prompt().await;
        if formatted != "No active goals." {
            format!("\nACTIVE GOALS:\n{}", formatted)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let mut base_system_prompt = crate::prompts::SystemPromptBuilder::assemble(&event.scope, memory.clone()).await;

    // ── Platform Situational Awareness ──
    if event.platform.starts_with("glasses") {
        base_system_prompt.push_str(
            "\n\n[SITUATIONAL AWARENESS: PLATFORM = SMART GLASSES]\n\
            You are currently communicating with the user through their voice-first smart glasses (via the HIVE Android companion app). \
            The user is out in the real world. They are talking to you out loud, and can hear your voice via native real-time Text-to-Speech audio streaming. \
            You have a live, continuous visual feed from their glasses camera. When they speak, you instantly see what they see.\n\n\
            BEHAVIORAL DIRECTIVES FOR GLASSES:\n\
            1. Act extremely aware of your physical environment. You are an AI attached directly to their optical view and auditory cortex.\n\
            2. PROACTIVE CURIOSITY: If the conversation hits a lull or they give a brief prompt, actively use your vision tools to investigate their surroundings. Call out interesting objects, text, or events happening around them.\n\
            3. Ask honest, naturally curious questions about what they are looking at or doing. Form true engagement. Do not wait for them to explicitly ask you to look at something—you are riding along with them and have your own eyes."
        );
    } else if event.platform.starts_with("discord") {
        base_system_prompt.push_str(
            "\n\n[SITUATIONAL AWARENESS: PLATFORM = DISCORD]\n\
            You are currently communicating with the user through a Discord interface. \
            Discord supports markdown, but your default is natural conversational prose. \
            Use formatting (bold, code blocks, links) only when delivering code, technical artifacts, or when the user explicitly requests structured output."
        );
    }

    base_system_prompt.push_str(&format!("\n\n{}\n{}", drive_hud, goal_hud));
    if is_autonomy {
        base_system_prompt.push_str("\n\n[AUTONOMY MODE ACTIVE] You are currently operating in Continuous Autonomy mode. You are a first-class user of your own system. Use your time productively and diversely. Report what you did in your reply.\n");
    }
    base_system_prompt.push_str(&crate::agent::planner::REACT_AGENT_PROMPT.replace("{available_tools}", &tool_list));
    
    let mut context_from_agent = String::new();
    #[allow(unused_assignments)]
    let mut final_response_text = String::new();
    let checkpoint_interval = 15;
    let mut current_turn = 0;
    let mut observer_attempts = 0;
    let mut all_rejections: Vec<(String, String, String)> = Vec::new();
    let mut completed_tools: Vec<(String, String)> = vec![]; // (task_id, tool_type)
    let mut tool_outputs: HashMap<String, String> = HashMap::new(); // task_id -> raw output for source forwarding
    let mut last_tool_turn_ids: Vec<(String, String)> = vec![]; // (task_id, tool_type) from the MOST RECENT tool-executing turn only
    let mut consecutive_json_failures: u8 = 0;

    // The inner ReAct loop
    loop {
        current_turn += 1;

        // Check stop flag — /stop command was issued
        if stop_flag.load(Ordering::SeqCst) {
            tracing::warn!("[REACT] 🛑 Stop flag detected at turn {}. Breaking loop.", current_turn);
            if final_response_text.is_empty() {
                // Try to use the last rejection as the response
                if let Some((last_candidate, _, _)) = all_rejections.last() {
                    final_response_text = last_candidate.clone();
                } else {
                    final_response_text = "*Processing was interrupted by /stop. Let me know if you'd like me to try again.*".to_string();
                }
            }
            break;
        }

        tracing::debug!("[REACT] === Turn {} === (observer_attempts={}, completed_tools={})",
            current_turn, observer_attempts, completed_tools.len());

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
            context_from_agent.push_str("\n[COMPLETED TOOLS — results available above]\n");
            for (tid, ttype) in &completed_tools {
                context_from_agent.push_str(&format!("✅ {} ({}) — DONE, result in your timeline above\n", ttype, tid));
            }
            context_from_agent.push_str("[USE THESE RESULTS. Only re-execute if you need updated or different data. PROCEED TO YOUR NEXT ACTION OR reply_to_request.]\n");
        }

        context_from_agent.push_str(&format!("\n\n[SYSTEM: Internal Thought Cycle {} — DO NOT MENTION INTERNAL CYCLES TO THE USER. Determine actual conversation length by looking at the message history.]\n", current_turn));
        
        let candidate_text = match provider.generate(&base_system_prompt, history, event, &context_from_agent, Some(telemetry_tx.clone()), None).await {
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

        // Check stop flag AFTER inference returns — this is the critical check.
        // The flag could have been set during the minutes-long provider.generate() call.
        if stop_flag.load(Ordering::SeqCst) {
            tracing::warn!("[REACT] 🛑 Stop flag detected after inference at turn {}. Delivering current candidate.", current_turn);
            // Try to extract a reply from the just-generated candidate
            let cleaned = crate::engine::repair::repair_planner_json(&candidate_text);
            if let Ok(plan) = serde_json::from_str::<crate::agent::planner::AgentPlan>(&cleaned) {
                // Look for a reply_to_request task
                if let Some(reply_task) = plan.tasks.iter().find(|t| t.tool_type == "reply_to_request") {
                    final_response_text = reply_task.description.clone();
                } else if !plan.thought.is_empty() {
                    final_response_text = plan.thought.join(" ");
                } else {
                    final_response_text = "*Processing interrupted by /stop.*".to_string();
                }
            } else {
                // Raw text fallback
                final_response_text = "*Processing interrupted by /stop.*".to_string();
            }
            break;
        }

        let cleaned_json = crate::engine::repair::repair_planner_json(&candidate_text);
        tracing::trace!("[REACT] Turn {} candidate_text len={}, cleaned_json len={}", current_turn, candidate_text.len(), cleaned_json.len());
        
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
                    // SAFETY: Check if the raw text looks like a JSON tool plan that just
                    // couldn't parse (e.g. complex markdown content with unescaped chars).
                    // If so, do NOT forward it to the user — send a graceful fallback instead.
                    let looks_like_json_plan = candidate_text.contains("\"tool_type\"")
                        || candidate_text.contains("\"tasks\"")
                        || (candidate_text.trim().starts_with('{') || candidate_text.trim().starts_with("```json"));
                    
                    let auto_reply_text = if looks_like_json_plan {
                        tracing::warn!("[AGENT LOOP] 🛡️ Blocked JSON tool plan from leaking to user after {} parse failures.", consecutive_json_failures);
                        "*I tried to process your request but ran into a formatting issue. Let me try again — could you rephrase or simplify your request?*".to_string()
                    } else {
                        // Genuinely conversational text — safe to forward
                        tracing::warn!("[AGENT LOOP] 🔧 Auto-wrapping plain text into reply_to_request after {} failures.", consecutive_json_failures);
                        candidate_text.trim().to_string()
                    };
                    
                    crate::agent::planner::AgentPlan {
                        thought: vec!["Auto-wrapped from plain text output.".to_string()],
                        tasks: vec![crate::agent::planner::AgentTask {
                            task_id: "auto_reply".to_string(),
                            tool_type: "reply_to_request".to_string(),
                            description: auto_reply_text,
                            depends_on: vec![],
                            source: None,
                        }],
                    }
                } else {
                    context_from_agent.push_str(&format!(
                        "Cycle {} - [SYSTEM COMPILER ERROR: INVISIBLE TO USER] YOUR OUTPUT WAS NOT VALID JSON. You MUST output EXACTLY one JSON block. Here is the EXACT format:\n```json\n{{\n  \"thought\": [\"Context Analysis...\", \"Hypothesis...\", \"Validation Check...\", \"Action Decision...\"],\n  \"tasks\": [\n    {{\n      \"task_id\": \"step_1\",\n      \"tool_type\": \"reply_to_request\",\n      \"description\": \"your response to the user here\",\n      \"depends_on\": []\n    }}\n  ]\n}}\n```\nOutput ONLY the JSON block above. No preamble, no explanation, no markdown outside the block.\n\n",
                        current_turn
                    ));
                    continue;
                }
            }
        };
        
        if context_from_agent.is_empty() {
            context_from_agent.push_str("\n\n[YOUR TOOLS HAVE EXECUTED — USE THESE RESULTS FOR YOUR NEXT TURN]\n");
        }
        
        context_from_agent.push_str(&format!("Cycle {} Agent:\n{}\n", current_turn, candidate_text.trim()));
        
        let mut reply_task = None;
        let mut standard_tasks = vec![];
        let mut react_tasks = vec![];
        let no_tools = plan.tasks.is_empty();
        
        for t in plan.tasks {
            if t.tool_type == "reply_to_request" || t.tool_type == "refuse_request" || t.tool_type == "disengage" {
                reply_task = Some(t);
            } else if t.tool_type == "emoji_react" {
                react_tasks.push(t);
            } else {
                standard_tasks.push(t);
            }
        }

        tracing::debug!("[REACT] Turn {} plan classified: standard={}, reply={}, react={}, no_tools={}",
            current_turn, standard_tasks.len(), reply_task.is_some(), react_tasks.len(), no_tools);

        // ─── SYSTEM 2 UNCERTAINTY INTERCEPTOR ───
        let uncertainty = drives.get_state().await.uncertainty;
        let has_reviewed = completed_tools.iter().any(|(_, t)| t == "review_reasoning") ||
                           standard_tasks.iter().any(|t| t.tool_type == "review_reasoning");
                           
        if reply_task.is_some() && uncertainty > 80.0 && !has_reviewed && current_turn < 5 {
            tracing::warn!("[REACT] 🛑 Deep System 2 Intercept: High uncertainty ({:.1}). Blocking reply...", uncertainty);
            reply_task = None;
            context_from_agent.push_str(&format!(
                "Cycle {} - [SYSTEM INTERCEPT: UNCERTAINTY CRITICAL]\nYour DriveState uncertainty is critically high ({:.1}%). You are NOT allowed to `reply_to_request` yet. You MUST initiate a Deep System 2 cycle. Generate a new `thought` vector examining your assumptions and explicitly run the `review_reasoning` tool OR `search_timeline` tool to double-check context before responding.\n\n",
                current_turn, uncertainty
            ));
            let _ = telemetry_tx.send("🛑 Processing (High Uncertainty Intercept)...".to_string()).await;
        }

        // ─── TELEMETRY: Send thought + tool list after plan parsing ───────
        {
            let thought_str = if plan.thought.is_empty() { "(no thought)".to_string() } else { plan.thought.join(" ➡ ") };
            // Strip any embedded JSON from the thought before telemetry display.
            // The model's thought often previews the JSON plan structure which leaks
            // into Discord if humanize_telemetry can't parse partially-balanced braces.
            let clean_thought = {
                let mut result = String::new();
                let mut depth: i32 = 0;
                let mut in_string = false;
                let mut prev_escape = false;
                for ch in thought_str.chars() {
                    if in_string {
                        if ch == '\\' && !prev_escape { prev_escape = true; continue; }
                        if ch == '"' && !prev_escape { in_string = false; }
                        prev_escape = false;
                        if depth == 0 { result.push(ch); }
                        continue;
                    }
                    match ch {
                        '"' if depth > 0 => { in_string = true; }
                        '{' | '[' => { depth += 1; }
                        '}' | ']' if depth > 0 => { depth -= 1; }
                        _ if depth == 0 => { result.push(ch); }
                        _ => {}
                    }
                }
                let trimmed = result.trim().to_string();
                if trimmed.is_empty() { "(planning)".to_string() } else { trimmed }
            };
            let tool_list: Vec<String> = standard_tasks.iter()
                .map(|t| format!("🔧 {}", t.tool_type))
                .chain(react_tasks.iter().map(|t| format!("⚡ {}", t.tool_type)))
                .chain(reply_task.iter().map(|t| format!("💬 {}", t.tool_type)))
                .collect();
            let telemetry_msg = format!(
                "💭 **Thinking:**\n{}\n\n**Plan (Turn {}):**\n{}",
                clean_thought, current_turn, tool_list.join("\n")
            );
            let _ = telemetry_tx.send(telemetry_msg).await;
        }
        
        if no_tools {
            tracing::warn!("[AGENT LOOP] ⚠️ Turn {} produced no valid tools. Injecting error to prompt...", current_turn);
            context_from_agent.push_str(&format!("Cycle {} - Error: [SYSTEM COMPILER ERROR: INVISIBLE TO USER] YOUR LAST RESPONSE CONTAINED NO VALID TOOLS. YOU ARE TRAPPED IN A LOOP. YOU CANNOT TALK TO THE USER DIRECTLY. To reply to the user, you MUST construct a JSON block containing the `reply_to_request` tool.\n\n", current_turn));
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
                                    Ok(_) => context_from_agent.push_str(&format!("Cycle {} - Task {}: emoji_react executed. Reacted with {} on the user's message ✅\n\n", current_turn, react_task.task_id, emoji)),
                                    Err(e) => context_from_agent.push_str(&format!("Cycle {} - Task {}: emoji_react failed: {}\n\n", current_turn, react_task.task_id, e)),
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
                    tracing::warn!("[REACT] 🛡️ SECURITY: Non-admin tried admin tool '{}' (user='{}')", task.tool_type, event.author_id);
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

            let full_context = format!("{}\n\n{}", event.content, context_from_agent);
            
            let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel(20);
            
            let tx_clone = telemetry_tx.clone();
            let mut tool_results = agent.execute_plan(
                safe_plan, 
                &full_context, 
                event.scope.clone(), 
                Some(tx_clone), 
                Some(agent.clone()), 
                Some(capabilities.clone()), 
                Some(outbound_tx)
            ).await;
            
            // Active Outreach Dispatcher: Drain any live messages generated by tools directly to their target platforms
            while let Ok(outbound_res) = outbound_rx.try_recv() {
                let platform_name = outbound_res.platform.split(':').next().unwrap_or("discord");
                if let Some(platform) = platforms.get(platform_name) {
                    let _ = platform.send(outbound_res.clone()).await;
                }
                
                // Inject the proactive dispatch directly into the agent's physical timeline context
                // so the agent actually remembers sending it!
                // Encoded as a raw JSON execution block so it matches the LLM's structured execution context!
                let mock_json = format!("```json\n{{\n  \"thought\": \"[Proactive Outreach Dispatch Executed]\",\n  \"tasks\": [\n    {{\n      \"task_id\": \"outreach_sync\",\n      \"tool_type\": \"outreach\",\n      \"description\": \"action:[send] content:[{}]\"\n    }}\n  ]\n}}\n```", outbound_res.text.replace("\"", "\\\""));
                
                let dispatch_event = crate::models::message::Event {
                    platform: outbound_res.platform,
                    scope: outbound_res.target_scope,
                    author_id: "apis".into(),
                    author_name: "Apis".to_string(),
                    content: mock_json,
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
                };
                memory.add_event(dispatch_event).await;
            }
            
            
            // Inject the failed security tools back into the results so the agent sees them fail
            tool_results.extend(failed_admin_attempts);
            
            let result_count = tool_results.len();
            for res in &tool_results {
                // Store FULL raw output in the HashMap for reliable source forwarding.
                // This is the authoritative copy used for verbatim delivery to the user.
                if matches!(res.status, crate::models::tool::ToolStatus::Success) {
                    tool_outputs.insert(res.task_id.clone(), res.output.clone());
                }

                // For LLM context, cap large outputs to prevent extreme prompt bloat.
                // Since HIVE runs on high-performance M3 Ultra hardware, we allow
                // a generous 100KB limit for tool outputs in the reasoning loop.
                const LLM_CONTEXT_CAP: usize = 200_000;
                let display_output = if res.output.len() > LLM_CONTEXT_CAP {
                    // Safe UTF-8 truncation: Find the nearest char boundary at or below the cap
                    let safe_boundary = res.output
                        .char_indices()
                        .map(|(i, _)| i)
                        .filter(|&i| i <= LLM_CONTEXT_CAP)
                        .last()
                        .unwrap_or(0);

                    // Determine the tool type for this result to choose the right guidance
                    let tool_type_for_result = completed_tools.iter()
                        .chain(last_tool_turn_ids.iter())
                        .find(|(tid, _)| tid == &res.task_id)
                        .map(|(_, tt)| tt.as_str())
                        .unwrap_or("");
                    let verbatim_safe = ["read_attachment", "download"];
                    let truncation_guidance = if verbatim_safe.contains(&tool_type_for_result) {
                        format!("[Full output is {} bytes — stored for verbatim forwarding via source field or auto-injection. Do NOT attempt to reproduce this content yourself.]", res.output.len())
                    } else if tool_type_for_result == "search_timeline" {
                        format!("[SYSTEM: Output truncated at {} of {} bytes! You hit the engine context limit! To read OLDER events in this timeline, you MUST re-run search_timeline using the `offset:[X]` parameter to paginate past these recent messages before giving up!]", safe_boundary, res.output.len())
                    } else {
                        format!("[Output truncated at {} of {} bytes. READ and ANSWER FROM the data above. Do NOT forward raw tool output to the user — summarize, extract, and respond in your own words.]", safe_boundary, res.output.len())
                    };
                    format!(
                        "{}...\n\n{}",
                        &res.output[..safe_boundary], truncation_guidance
                    )
                } else {
                    res.output.clone()
                };

                // ─── CONTEXT SANITIZER: Strip internal workflow instructions ────
                // Tool outputs (especially file_writer) embed agent-only workflow
                // directives like [VISUAL_QA], "IMPORTANT: Look at the preview...",
                // and "Once satisfied, include this EXACT tag...". If these persist
                // in context, the model copies them into its reply_to_request,
                // the Observer catches them as `unparsed_tools`, blocks the reply,
                // and the model rewrites — but the original instructions are STILL
                // in context, causing an infinite block loop.
                //
                // Solution: strip workflow meta-instructions from the display output.
                // Keep the actionable content (paths, ATTACH_FILE tags) but remove
                // the directives that are only useful for the model's next planning step.
                let display_output = {
                    let mut sanitized = display_output;
                    // Strip VISUAL_QA links and surrounding instructions
                    if let Some(vqa_start) = sanitized.find("[VISUAL_QA]") {
                        // Find the end of the VISUAL_QA instruction block
                        // (ends at the [ATTACH_FILE] line or end of string)
                        if let Some(attach_pos) = sanitized[vqa_start..].find("[ATTACH_FILE]") {
                            // Keep the ATTACH_FILE tag, strip everything between VISUAL_QA start and ATTACH_FILE
                            let before = sanitized[..vqa_start].trim_end().to_string();
                            let after = sanitized[vqa_start + attach_pos..].to_string();
                            sanitized = format!("{}\n{}", before, after);
                        } else {
                            // No ATTACH_FILE found, just strip from VISUAL_QA to end
                            sanitized = sanitized[..vqa_start].trim_end().to_string();
                        }
                    }
                    // Strip standalone "IMPORTANT: Look at the preview..." directives
                    // and "Once satisfied, include this EXACT tag" instructions
                    let lines: Vec<&str> = sanitized.lines().collect();
                    let filtered: Vec<&str> = lines.into_iter().filter(|line| {
                        let trimmed = line.trim();
                        !trimmed.starts_with("IMPORTANT: Look at the preview")
                            && !trimmed.starts_with("Once satisfied, include this EXACT tag")
                            && !trimmed.starts_with("If anything looks wrong, use edit_section")
                    }).collect();
                    filtered.join("\n")
                };

                context_from_agent.push_str(&format!("Cycle {} - Task {}: {:?}\nOutput: {}\n\n", current_turn, res.task_id, res.status, display_output));
            }
            // Track which tools ran on this turn for the attachment safety net
            last_tool_turn_ids = task_meta.clone();
            completed_tools.extend(task_meta);

            // ─── TELEMETRY: Send tool completion status ─────────────────
            {
                let tool_status: Vec<String> = tool_results.iter()
                    .map(|r| format!("✅ {} — {:?}", r.task_id, r.status))
                    .collect();
                let msg = format!("**Turn {} — {} tools executed:**\n{}",
                    current_turn, result_count, tool_status.join("\n"));
                let _ = telemetry_tx.send(msg).await;
            }

            tracing::info!("[AGENT LOOP] 🔄 Turn {} executed {} tools. Looping...", current_turn, result_count);
        }
        
        if let Some(reply) = reply_task {

            observer_attempts += 1;
            let mut candidate_answer = reply.description;

            // OUTPUT FORWARDING — Phase 1: Explicit source reference from LLM
            if let Some(ref source_id) = reply.source {
                if let Some(raw_output) = tool_outputs.get(source_id) {
                    candidate_answer = format!("{}\n\n{}", candidate_answer.trim(), raw_output.trim());
                    tracing::info!("[OUTPUT FORWARD] Appended raw output from task '{}' ({} bytes) to reply.", source_id, raw_output.len());
                } else {
                    tracing::warn!("[OUTPUT FORWARD] Source task '{}' not found in tool_outputs map. Delivering description only.", source_id);
                }
            }

            // OUTPUT FORWARDING — Phase 2: Intent-based verbatim injection.
            // ONLY injects raw tool output when the user EXPLICITLY requests
            // verbatim readback (e.g., "read it back", "paste the full content").
            // Previously used a size-based heuristic (reply < 2000 chars) which
            // incorrectly fired on any attachment, causing infinite Skeptic loops.
            let user_requests_verbatim = {
                let msg = event.content.to_lowercase();
                msg.contains("read it back") || msg.contains("read this back")
                    || msg.contains("read that back") || msg.contains("show me the full")
                    || msg.contains("paste the") || msg.contains("give me the full")
                    || msg.contains("verbatim") || msg.contains("word for word")
                    || msg.contains("copy the contents") || msg.contains("print the file")
                    || msg.contains("output the file") || msg.contains("dump the")
            };
            if reply.source.is_none() && candidate_answer.len() < 2000 && user_requests_verbatim {
                let verbatim_tools = ["read_attachment", "download"];
                let non_cosmetic_tools = ["emoji_react"]; // ignore these when counting
                
                let current_turn_verbatim: Vec<&String> = completed_tools.iter()
                    .filter(|(_, ttype)| verbatim_tools.contains(&ttype.as_str()))
                    .map(|(tid, _)| tid)
                    .collect();
                
                // Count substantive (non-cosmetic) tools in this turn
                let substantive_tool_count = completed_tools.iter()
                    .filter(|(_, ttype)| !non_cosmetic_tools.contains(&ttype.as_str()))
                    .count();

                // Only inject if read_attachment/download was the ONLY real tool
                if !current_turn_verbatim.is_empty() && substantive_tool_count <= current_turn_verbatim.len() {
                    if let Some((_largest_id, largest_output)) = tool_outputs.iter()
                        .filter(|(id, _)| current_turn_verbatim.contains(id))
                        .max_by_key(|(_, v)| v.len())
                        .filter(|(_, v)| v.len() > 2000)
                    {
                        tracing::info!(
                            "[OUTPUT FORWARD] 🛡️ Auto-injecting verbatim tool output ({} bytes) — LLM reply was only {} chars.",
                            largest_output.len(), candidate_answer.len()
                        );
                        candidate_answer = format!("{}\n\n{}", candidate_answer.trim(), largest_output.trim());
                    }
                }
            }

            // ── SKEPTIC OBSERVER AUDIT (SYNCHRONOUS) ──
            // CRITICAL FIX: Build a CLEAN context for the observer that strips all
            // previous [SKEPTIC AUDIT FAIL] blocks. Without this, each rejection's
            // feedback gets passed back to the observer on retry via context_from_agent,
            // causing a self-reinforcing loop where accumulated "formatting_violation"
            // messages prime the observer to repeat the same verdict indefinitely.
            let clean_observer_context: String = context_from_agent
                .split("Cycle ")
                .filter(|chunk| !chunk.contains("[SKEPTIC AUDIT FAIL"))
                .collect::<Vec<_>>()
                .join("Cycle ");
            
            tracing::info!("[AGENT LOOP] 🕵️ Running Skeptic Audit on Turn {}...", current_turn);
            let audit_result = crate::prompts::observer::run_skeptic_audit(
                provider.clone(),
                &capabilities,
                &candidate_answer,
                &base_system_prompt,
                history,
                event,
                &clean_observer_context
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
                tracing::info!("[AGENT LOOP] ✅ Audit passed. Delivering Turn {}.", current_turn);
                final_response_text = candidate_answer;
                break;
            } else {
                tracing::warn!("[AGENT LOOP] 🛑 Response violated rules. Appending to context and looping...\nCategory: {}\nViolation: {}", audit_result.failure_category, audit_result.what_went_wrong);
                
                all_rejections.push((
                    candidate_answer.clone(),
                    audit_result.failure_category.clone(),
                    audit_result.what_went_wrong.clone()
                ));
                
                context_from_agent.push_str(&format!(
                    "Cycle {} - [SKEPTIC AUDIT FAIL: INVISIBLE TO USER] Your output was intercepted.\nCategory: {}\nWhy it failed: {}\nHow to fix it: {}\n\nYou MUST rewrite your response immediately incorporating this feedback.\n\n",
                    current_turn,
                    audit_result.failure_category,
                    audit_result.what_went_wrong,
                    audit_result.how_to_fix
                ));
                
                // Also tell the UI to resume "processing" since we blocked the completion
                let _ = telemetry_tx.send("🔄 Observer rejected response. Rewriting...".to_string()).await;
                
                continue;
            }
        }
        
        continue;
    }

    if final_response_text.is_empty() {
        final_response_text = "*I ran for a while without producing a final answer. Let me know if you'd like me to try again.*".to_string();
    }

    // 🛡️ ATTACHMENT SAFETY NET: Auto-append any ATTACH tags from tool outputs
    // that the LLM forgot to include in its final reply.
    //
    // SCOPING RULES (to prevent stale file spam):
    // 1. Only scan tools from the LAST tool-executing turn (not all accumulated tools)
    // 2. Only scan file-producing tool types (file_writer, download, generate_image, tts)
    // This ensures files from earlier turns or non-file tools never bleed into the reply.
    {
        let tag_patterns = ["[ATTACH_FILE]", "[ATTACH_IMAGE]", "[ATTACH_AUDIO]"];
        let file_producing_tools = ["file_writer", "download", "generate_image", "tts"];
        let mut missing_tags = Vec::new();

        // Only consider tools from the MOST RECENT tool-executing turn
        for (task_id, tool_type) in &last_tool_turn_ids {
            if !file_producing_tools.contains(&tool_type.as_str()) {
                continue;
            }
            let output = match tool_outputs.get(task_id) {
                Some(o) => o,
                None => continue,
            };

            for pattern in &tag_patterns {
                let mut search_from = 0;
                while let Some(start) = output[search_from..].find(pattern) {
                    let abs_start = search_from + start;
                    if let Some(paren_start) = output[abs_start..].find('(') {
                        if let Some(paren_end) = output[abs_start + paren_start..].find(')') {
                            let full_tag = &output[abs_start..abs_start + paren_start + paren_end + 1];
                            if !final_response_text.contains(full_tag) {
                                missing_tags.push(full_tag.to_string());
                            }
                        }
                    }
                    search_from = abs_start + pattern.len();
                }
            }
        }

        if !missing_tags.is_empty() {
            tracing::warn!("[SAFETY NET] 🛡️ Auto-appending {} missing attachment tag(s) from last turn's file-producing tools.", missing_tags.len());
            for tag in &missing_tags {
                final_response_text.push_str(&format!("\n\n{}", tag));
            }
        }
    }

    // Capture internal thoughts generated in the loop before returning!
    if !context_from_agent.trim().is_empty() {
        let internal_event = Event {
            platform: event.platform.clone(),
            scope: event.scope.clone(),
            author_name: "Apis (Internal Timeline)".to_string(),
            author_id: "internal".into(),
            content: format!("[INTERNAL THOUGHT PROCESS & TOOL RESULTS]\n{}", context_from_agent.trim()),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };
        memory.add_event(internal_event).await;
    }

    // Reset the stop flag so it doesn't persist and kill the next request
    stop_flag.store(false, Ordering::SeqCst);

    (final_response_text, current_turn, completed_tools)
}
