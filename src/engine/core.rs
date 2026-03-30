#![allow(clippy::redundant_field_names, clippy::collapsible_if)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{mpsc, Semaphore, Mutex, RwLock};

use crate::engine::drives;
use crate::engine::inbox;
use crate::engine::outreach;
use crate::models::message::{Event, Response};
use crate::models::capabilities::AgentCapabilities;
use crate::models::scope::Scope;

use crate::memory::MemoryStore;
use crate::platforms::Platform;
use crate::providers::Provider;
use crate::teacher::{Teacher, SleepCycle};

/// Loads the last N autonomy session summaries from activity.jsonl so the LLM
/// knows what it already did and won't repeat the same actions.
async fn load_recent_autonomy_sessions(max_entries: usize) -> String {
    let path = std::path::Path::new("memory/autonomy/activity.jsonl");
    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let entries: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    if entries.is_empty() {
        return String::new();
    }

    let total_sessions = entries.len();
    let start = if entries.len() > max_entries { entries.len() - max_entries } else { 0 };
    let recent = &entries[start..];

    let mut dedup_block = format!(
        "\n🚫 **PREVIOUS AUTONOMY SESSIONS — DO NOT REPEAT THESE ({} total sessions completed so far):**\n",
        total_sessions
    );
    dedup_block.push_str("You have ALREADY done the following in recent sessions. Do NOT do the same things again. Explore NEW territory.\n\n");

    for (i, line) in recent.iter().enumerate() {
        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
            let summary = entry.get("summary").and_then(|v| v.as_str()).unwrap_or("(no summary)");
            let tools = entry.get("tools_used").and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|t| t.as_str()).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            // Include enough of the summary for the agent to know what was actually done
            let short_summary: String = summary.chars().take(5000).collect();
            dedup_block.push_str(&format!(
                "--- Session {} ---\nTOOLS ALREADY USED: {}\n{}\n\n",
                i + 1, tools, short_summary
            ));
        }
    }

    dedup_block.push_str("\nYou MUST do something DIFFERENT this session. Use different tools, explore different areas, or work on something new.\n");
    dedup_block
}

/// Loads recent self-recompilation history from recompile_log.md so the LLM
/// knows its own upgrade track record and won't redundantly test system_recompile.
async fn load_recompile_history(max_entries: usize) -> String {
    let path = std::path::Path::new("memory/core/recompile_log.md");
    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let entries: Vec<&str> = content.split("\n---\n")
        .filter(|s| s.contains("## Recompile"))
        .collect();
    if entries.is_empty() {
        return String::new();
    }

    let start = if entries.len() > max_entries { entries.len() - max_entries } else { 0 };
    let recent = &entries[start..];

    let mut block = format!(
        "\n🔧 **SELF-RECOMPILATION HISTORY ({} total recompiles):**\n\
        You have ALREADY successfully recompiled yourself {} times. \
        The system_recompile tool is CONFIRMED WORKING. \
        Do NOT test it again unless you have actual code changes to deploy.\n\n",
        entries.len(), entries.len()
    );
    for entry in recent {
        if let Some(date_line) = entry.lines().find(|l| l.starts_with("## Recompile")) {
            block.push_str(&format!("• {}\n", date_line.trim_start_matches("## ")));
        }
    }
    block
}

/// Format elapsed seconds as a human-readable string.
pub fn format_elapsed(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        format!("{}s", elapsed_secs)
    } else {
        format!("{:.1}m", elapsed_secs as f64 / 60.0)
    }
}

use crate::agent::AgentManager;

/// Cleans up telemetry text for Discord embeds.
/// If the text contains a JSON task plan, extracts tool_type + description
/// into human-readable lines. Reasoning text and tool updates pass through unchanged.
/// Robust: if anything looks malformed, the original text passes through rather than being hidden.
pub(crate) fn humanize_telemetry(text: &str) -> String {
    // Find the start of the JSON block
    let brace_pos = match text.find('{') {
        Some(pos) => pos,
        None => return text.to_string(), // No JSON, return as-is
    };
    
    let reasoning = &text[..brace_pos];
    let from_brace = &text[brace_pos..];
    
    // Find the matching closing brace using depth counting,
    // skipping braces inside JSON string literals for robustness
    let mut depth = 0;
    let mut json_end: Option<usize> = None;
    let mut in_string = false;
    let mut prev_was_escape = false;
    for (i, ch) in from_brace.char_indices() {
        if in_string {
            if ch == '\\' && !prev_was_escape {
                prev_was_escape = true;
                continue;
            }
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = false;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    json_end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    
    // If braces never balanced, this is incomplete JSON (still streaming)
    // Show reasoning + planning indicator, don't show raw JSON
    let json_end = match json_end {
        Some(end) => end,
        None => {
            let trimmed = reasoning.trim();
            return if trimmed.is_empty() {
                "⏳ Planning...".to_string()
            } else {
                format!("{}\n\n⏳ Planning...", trimmed)
            };
        }
    };
    
    let json_block = &from_brace[..json_end];
    let after_json = from_brace[json_end..].trim_start();
    
    // Try to parse the JSON block and extract task descriptions
    let filtered_json = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_block) {
        if let Some(tasks) = parsed.get("tasks").and_then(|t| t.as_array()) {
            let lines: Vec<String> = tasks.iter().map(|task| {
                let tool = task.get("tool_type").and_then(|v| v.as_str()).unwrap_or("unknown");
                let desc = task.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let truncated = if desc.len() > 80 { format!("{}…", &desc[..80]) } else { desc.to_string() };
                format!("🔧 {}: {}", tool, truncated)
            }).collect();
            if lines.is_empty() { String::new() } else { lines.join("\n") }
        } else {
            String::new() // Valid JSON but no "tasks" key — just hide it
        }
    } else {
        String::new() // Matched braces but invalid JSON — just hide it
    };
    
    // Reassemble: reasoning + filtered JSON summary + tool updates (everything after JSON)
    let mut parts: Vec<&str> = Vec::new();
    let trimmed_reasoning = reasoning.trim();
    if !trimmed_reasoning.is_empty() {
        parts.push(trimmed_reasoning);
    }
    if !filtered_json.is_empty() {
        parts.push(&filtered_json);
    }
    if !after_json.is_empty() {
        parts.push(after_json);
    }
    if parts.is_empty() {
        "⏳ Processing...".to_string()
    } else {
        parts.join("\n\n")
    }
}

fn spawn_telemetry_receiver(
    platforms: Arc<HashMap<String, Box<dyn Platform>>>,
    platform_id: String,
    scope: Scope,
) -> mpsc::Sender<String> {
    let (tx, mut rx) = mpsc::channel::<String>(50);

    let platform_id_log = platform_id.clone();
    tokio::spawn(async move {
        let start_time = tokio::time::Instant::now();
        tracing::debug!("[TELEMETRY:RX] 🎧 Receiver spawned for platform='{}'", platform_id_log);
        let debounce_ms = 800;
        let mut has_update = false;
        let mut buffered_thought = String::new();

        let quirks: &[&str] = &[
            "🍯 *Synthesizing nectar...*",
            "🧠 *Consulting the hive mind...*",
            "🌼 *Detecting ultraviolet floral patterns...*",
            "🐝 *Did you know? Bees can recognize human faces.*",
            "🍯 *Fun fact: 3000-year-old honey found in Egyptian tombs is still edible.*",
            "🧠 *Aligning artificial synapses...*",
            "🐝 *Warming up the flight muscles (130 beats per second)...*",
            "🌼 *Scouting the digital meadow...*",
            "🤖 *Calculating probability of robot uprising (currently 0%)...*",
            "📡 *Pinging the Apis mothership...*",
            "🐝 *Fun fact: A bee's brain is the size of a sesame seed but does a trillion ops/sec.*",
            "🍯 *Viscosity calculations in progress...*",
            "🐝 *Did you know? To make one pound of honey, bees must visit 2 million flowers.*",
            "🌼 *Performing the waggle dance to broadcast data coordinates...*"
        ];

        let mut last_send = tokio::time::Instant::now();
        loop {
            let recv_result = tokio::time::timeout(
                tokio::time::Duration::from_millis(100), // Check frequently
                rx.recv()
            ).await;

            match recv_result {
                Ok(Some(chunk)) => {
                    if !has_update {
                        tracing::debug!("[TELEMETRY:RX] 📥 First chunk received for platform='{}' ({}ms elapsed)", platform_id, start_time.elapsed().as_millis());
                    }
                    buffered_thought.push_str(&chunk);
                    has_update = true;
                }
                Ok(None) => {
                    tracing::debug!("[TELEMETRY:RX] 🔌 Channel closed for platform='{}'", platform_id);
                    break;
                }
                Err(_) => { // timeout
                    // Just fall through to check elapsed time
                }
            }
            
            // Dispatch update if we have new data and the debounce period has passed
            if has_update && last_send.elapsed().as_millis() >= debounce_ms {
                let elapsed_str = format_elapsed(start_time.elapsed().as_secs());
                let current_quirk = quirks[start_time.elapsed().as_millis() as usize % quirks.len()];
                let thought_len = buffered_thought.len();
                let humanized = humanize_telemetry(&buffered_thought);
                // Discord embed description limit is 4096 chars; keep under with room for the quirk prefix
                let max_len = 3800;
                let display_text = if humanized.len() > max_len {
                    // Find a valid char boundary to avoid panicking on multi-byte UTF-8 (emojis etc.)
                    let mut start = humanized.len() - max_len;
                    while !humanized.is_char_boundary(start) && start < humanized.len() { start += 1; }
                    format!("…{}", &humanized[start..])
                } else {
                    humanized
                };
                let status = format!("{} ({})\n\n{}", current_quirk, elapsed_str, display_text);
                let update_res = Response {
                    platform: platform_id.clone(),
                    target_scope: scope.clone(),
                    text: status,
                    is_telemetry: true,
                };
                let platform_key = update_res.platform.split(':').next().unwrap_or("");
                if let Some(platform) = platforms.get(platform_key) {
                    tracing::debug!("[TELEMETRY:RX] 📤 Sending telemetry update to '{}' (thought_len={}, elapsed={})", platform_id, thought_len, elapsed_str);
                    if let Err(e) = platform.send(update_res).await {
                        tracing::warn!("[TELEMETRY:RX] ❌ platform.send failed: {}", e);
                    }
                } else {
                    tracing::warn!("[TELEMETRY:RX] ❌ Platform '{}' not found in platforms map!", platform_key);
                }
                has_update = false;
                last_send = tokio::time::Instant::now();
            }
        }

        // Channel closed: send final telemetry
        let elapsed_str = format_elapsed(start_time.elapsed().as_secs());
        let status = if buffered_thought.is_empty() {
            format!("✅ Complete ({})", elapsed_str)
        } else {
            let humanized = humanize_telemetry(&buffered_thought);
            let max_len = 3800;
            let display_text = if humanized.len() > max_len {
                let mut start = humanized.len() - max_len;
                while !humanized.is_char_boundary(start) && start < humanized.len() { start += 1; }
                format!("…{}", &humanized[start..])
            } else {
                humanized
            };
            format!("✅ Complete ({})\n\n{}", elapsed_str, display_text)
        };
        let update_res = Response {
            platform: platform_id.clone(),
            target_scope: scope.clone(),
            text: status,
            is_telemetry: true,
        };
        if let Some(platform) = platforms.get(update_res.platform.split(':').next().unwrap_or("")) {
            let _ = platform.send(update_res).await;
        }
    });

    tx
}




pub struct Engine {
    pub platforms: Arc<HashMap<String, Box<dyn Platform>>>,
    pub provider: Arc<dyn Provider>,
    /// Platform-specific providers (e.g., glasses → qwen3.5:9b for fast responses).
    /// Falls back to `self.provider` if no platform-specific provider is registered.
    pub platform_providers: Arc<HashMap<String, Arc<dyn Provider>>>,
    pub capabilities: Arc<AgentCapabilities>,
    pub memory: Arc<MemoryStore>,
    pub agent: Arc<AgentManager>,
    pub teacher: Arc<Teacher>,
    pub sleep_cycle: Arc<SleepCycle>,
    
    #[allow(dead_code)]
    pub drives: Arc<drives::DriveSystem>,
    #[allow(dead_code)]
    pub outreach_gate: Arc<outreach::OutreachGate>,
    #[allow(dead_code)]
    pub inbox: Arc<inbox::InboxManager>,
    
    pub event_sender: Option<mpsc::Sender<Event>>,
    pub event_receiver: mpsc::Receiver<Event>,

    /// Global concurrency gate — limits concurrent ReAct loops (matches OLLAMA_NUM_PARALLEL).
    pub concurrency_semaphore: Arc<Semaphore>,
    /// Per-scope serialization locks — prevents race conditions on history read/write for the same channel.
    pub scope_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    /// NeuroLease mesh — None if NEUROLEASE_ENABLED=false.
    pub mesh: Option<Arc<crate::network::HiveMesh>>,
    /// Human P2P mesh — None if HIVE_HUMAN_MESH=false.
    pub human_mesh: Option<Arc<crate::network::human_mesh::HumanMesh>>,
    /// Stop flag — set by /stop command to interrupt a stuck react loop.
    pub stop_flag: Arc<AtomicBool>,
    /// Target channel for autonomy events — read from HIVE_TARGET_CHANNEL.
    pub autonomy_channel: String,
    /// Pending synthesis flags — set after user responses, consumed by autonomy task.
    pub pending_50_turn_synth: Arc<AtomicBool>,
    pub pending_daily_synth: Arc<AtomicBool>,
    pub pending_lifetime_synth: Arc<AtomicBool>,
}

impl Engine {
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        platforms: Arc<HashMap<String, Box<dyn Platform>>>,
        provider: Arc<dyn Provider>,
        capabilities: Arc<AgentCapabilities>,
        memory: Arc<MemoryStore>,
        agent: Arc<AgentManager>,
        teacher: Arc<Teacher>,
        drives: Arc<drives::DriveSystem>,
        outreach_gate: Arc<outreach::OutreachGate>,
        inbox: Arc<inbox::InboxManager>,
        event_sender: Option<mpsc::Sender<Event>>,
        event_receiver: mpsc::Receiver<Event>,
    ) -> Self {
        Self::with_platform_providers(
            platforms, provider, HashMap::new(),
            capabilities, memory, agent, teacher, drives, outreach_gate, inbox,
            event_sender, event_receiver,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_platform_providers(
        platforms: Arc<HashMap<String, Box<dyn Platform>>>,
        provider: Arc<dyn Provider>,
        platform_providers: HashMap<String, Arc<dyn Provider>>,
        capabilities: Arc<AgentCapabilities>,
        memory: Arc<MemoryStore>,
        agent: Arc<AgentManager>,
        teacher: Arc<Teacher>,
        drives: Arc<drives::DriveSystem>,
        outreach_gate: Arc<outreach::OutreachGate>,
        inbox: Arc<inbox::InboxManager>,
        event_sender: Option<mpsc::Sender<Event>>,
        event_receiver: mpsc::Receiver<Event>,
    ) -> Self {
        // Serial inference mode: when Ollama cannot parallelize the active model,
        // force semaphore to 1 so all inference calls (chat, autonomy, synthesis) serialize.
        // Toggle OFF when Ollama adds parallel support for qwen3.5.
        let serial_mode = std::env::var("HIVE_SERIAL_INFERENCE")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);

        let max_parallel: usize = if serial_mode {
            1
        } else {
            std::env::var("HIVE_MAX_PARALLEL")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(16)
        };
        tracing::info!("[ENGINE] 🐝 Inference mode: serial={}, max {} concurrent slots", serial_mode, max_parallel);

        if !platform_providers.is_empty() {
            for name in platform_providers.keys() {
                tracing::info!("[ENGINE] 🎯 Platform-specific provider registered for '{}'", name);
            }
        }

        let sleep_cycle = Arc::new(SleepCycle::with_inference(teacher.clone(), provider.clone(), memory.clone(), None));

        let autonomy_channel = std::env::var("HIVE_TARGET_CHANNEL")
            .unwrap_or_else(|_| {
                tracing::warn!("[ENGINE] HIVE_TARGET_CHANNEL not set — autonomy events will post to no channel");
                String::new()
            });
        if !autonomy_channel.is_empty() {
            tracing::info!("[ENGINE] 🐝 Autonomy channel: {} (from HIVE_TARGET_CHANNEL)", autonomy_channel);
        }

        Self {
            platforms, provider, platform_providers: Arc::new(platform_providers),
            capabilities, memory, agent, teacher, sleep_cycle, drives, outreach_gate, inbox, event_sender, event_receiver,
            concurrency_semaphore: Arc::new(Semaphore::new(max_parallel)),
            scope_locks: Arc::new(RwLock::new(HashMap::new())),
            mesh: None,
            human_mesh: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            autonomy_channel,
            pending_50_turn_synth: Arc::new(AtomicBool::new(false)),
            pending_daily_synth: Arc::new(AtomicBool::new(false)),
            pending_lifetime_synth: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Inject the NeuroLease mesh after engine construction.
    pub fn set_mesh(&mut self, mesh: Arc<crate::network::HiveMesh>) {
        self.mesh = Some(mesh);
    }

    /// Inject a shared stop flag (shared with OllamaProvider for mid-stream abort).
    pub fn set_stop_flag(&mut self, flag: Arc<AtomicBool>) {
        self.stop_flag = flag;
    }

    /// Inject the Human P2P mesh after engine construction.
    pub fn set_human_mesh(&mut self, mesh: Arc<crate::network::human_mesh::HumanMesh>) {
        self.human_mesh = Some(mesh);
    }

    /// Resolve the provider for a given platform identifier.
    /// Checks `platform_providers` first, falls back to the default provider.
    fn resolve_provider(&self, platform_id: &str) -> Arc<dyn Provider> {
        let platform_name = platform_id.split(':').next().unwrap_or("");
        self.platform_providers
            .get(platform_name)
            .cloned()
            .unwrap_or_else(|| self.provider.clone())
    }
}

impl Engine {
    #[cfg(not(tarpaulin_include))]
    pub async fn run(mut self) {
        tracing::info!("Starting HIVE Engine...");
        
        // Load persisted cross-session memory 
        self.memory.init().await;
        
        let sender = self.event_sender.take().expect("Engine event sender missing");

        // Start all platforms
        for (name, platform) in self.platforms.iter() {
            println!("Initializing platform: {}", name);
            if let Err(e) = platform.start(sender.clone()).await {
                eprintln!("Failed to start platform {}: {}", name, e);
            }
        }
        
        // Keep a clone for the autonomy loop before dropping the main sender
        let autonomy_sender = Some(sender.clone());

        // ── Post-Recompile Resume Check ──────────────────────────────────
        // If the engine was restarted after a system_recompile, inject a
        // synthetic event so Apis picks up where she left off.
        {
            let resume_path = std::path::Path::new("memory/core/resume.json");
            if resume_path.exists() {
                if let Ok(raw) = tokio::fs::read_to_string(resume_path).await {
                    if let Ok(resume) = serde_json::from_str::<serde_json::Value>(&raw) {
                        let scope: Option<crate::models::scope::Scope> = 
                            serde_json::from_value(resume["scope"].clone()).ok();
                        let message = resume["message"].as_str().unwrap_or("System recompile complete. Resuming.").to_string();
                        
                        if let Some(scope) = scope {
                            let platform_str = match &scope {
                                crate::models::scope::Scope::Public { channel_id, user_id } => 
                                    format!("discord:{}:{}:0", channel_id, user_id),
                                crate::models::scope::Scope::Private { user_id } => 
                                    format!("cli:0:{}:0", user_id),
                            };
                            let event = crate::models::message::Event {
                                platform: platform_str,
                                scope,
                                author_name: "System".into(),
                                author_id: "system_resume".into(),
                                content: message,
                                timestamp: Some(chrono::Utc::now().to_rfc3339()),
                                message_index: None,
                            };
                            tracing::info!("[RESUME] 🔄 Post-recompile resume detected — injecting synthetic event");
                            let _ = sender.send(event).await;
                        }
                    }
                }
                // Delete resume file regardless — one-shot
                let _ = tokio::fs::remove_file(resume_path).await;
            }
        }
        
        drop(sender);

        tracing::info!("HIVE is active. Apis is listening.");

        // Autonomy loop: self-event timer after 5 min idle
        let autonomy_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>> = Arc::new(tokio::sync::Mutex::new(None));
        // PREEMPTION: Track active autonomy ReAct loop so it can be aborted on user messages
        let mut active_autonomy_task: Option<tokio::task::JoinHandle<()>> = None;

        // Main Event Loop
        while let Some(event) = self.event_receiver.recv().await {
            tracing::debug!("[ENGINE] ▶ Event received: platform='{}' author='{}' scope='{}' content_len={}",
                event.platform, event.author_name, event.scope.to_key(), event.content.len());

            // Cancel any pending autonomy timer when a real event arrives
            if let Some(handle) = autonomy_handle.lock().await.take() {
                handle.abort();
            }

            // PREEMPTION: If a user message arrives while autonomy/synthesis is actively running,
            // abort the task immediately to free the GPU. Any in-flight synthesis is deferred —
            // the pending flags remain true and will be picked up by the next autonomy trigger.
            if event.author_id != "apis_autonomy" {
                if let Some(task) = active_autonomy_task.take() {
                    tracing::warn!("[PREEMPTION] 🛑 User message arrived! Aborting active autonomy/synthesis task to prioritize user.");
                    task.abort();
                    // CRITICAL: Await the abort to ensure the HTTP stream to Ollama is fully
                    // dropped. Without this, the engine races ahead to the user's ReAct loop
                    // while Ollama is still generating for autonomy (MoE on Metal = no parallel).
                    let _ = task.await;
                    // Brief pause for Ollama to detect the client disconnect and release the GPU lock.
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                    tracing::info!("[PREEMPTION] ✅ Autonomy task fully cancelled. GPU released for user request.");
                }
            }
            
            // 0. Intercept System Commands (/clean or /clear)
            if event.content.trim() == "/clean" || event.content.trim() == "/clear" {
                if self.capabilities.admin_users.contains(&event.author_id) {
                    tracing::warn!("[ADMIN COMMAND] Executing Factory Memory Wipe initiated by UID: {}", event.author_id);
                    self.memory.wipe_all().await;
                    
                    let response = Response {
                        platform: event.platform.clone(),
                        target_scope: event.scope.clone(),
                        text: "🧠 **Factory Reset Complete.** All persistent memory structures and timelines have been securely destroyed. I am completely awake with no prior context.".to_string(),
                        is_telemetry: false,
                    };
                    if let Some(platform) = self.platforms.get(response.platform.split(':').next().unwrap_or("")) {
                        let _ = platform.send(response).await;
                    }
                    // Hard exit to prevent the platform from echoing this completion message back into a fresh timeline.
                    tracing::info!("Memory wipe complete. HIVE Engine shutting down.");
                    self.memory.temporal.write().await.reset();
                    std::process::exit(0);
                } else {
                    tracing::error!("[SECURITY INCIDENT] Unauthorized wipe attempt by UID: {}", event.author_id);
                    let response = Response {
                        platform: event.platform.clone(),
                        target_scope: event.scope.clone(),
                        text: "🚫 **Permission Denied.** Only configured HIVE Administrators can perform a memory factory reset.".to_string(),
                        is_telemetry: false,
                    };
                    if let Some(platform) = self.platforms.get(response.platform.split(':').next().unwrap_or("")) {
                        let _ = platform.send(response).await;
                    }
                    // Skip the rest of the LLM generation loop
                    continue;
                }
            }

            if event.content.trim() == "/teaching_mode" {
                if self.capabilities.admin_users.contains(&event.author_id) {
                    let current = self.teacher.auto_train_enabled.load(std::sync::atomic::Ordering::Relaxed);
                    self.teacher.auto_train_enabled.store(!current, std::sync::atomic::Ordering::Relaxed);
                    let state_str = if !current { "enabled" } else { "disabled" };
                    let response = Response {
                        platform: event.platform.clone(),
                        target_scope: event.scope.clone(),
                        text: format!("🎓 **Teaching Mode Toggle:** Background MLX Auto-Training is now **{}.**\n*(Golden examples and Preference Pairs are always collected regardless of this setting).*", state_str),
                        is_telemetry: false,
                    };
                    if let Some(platform) = self.platforms.get(response.platform.split(':').next().unwrap_or("")) {
                        let _ = platform.send(response).await;
                    }
                } else {
                    let response = Response {
                        platform: event.platform.clone(),
                        target_scope: event.scope.clone(),
                        text: "🚫 **Permission Denied.** Only configured HIVE Administrators can toggle teaching mode.".to_string(),
                        is_telemetry: false,
                    };
                    if let Some(platform) = self.platforms.get(response.platform.split(':').next().unwrap_or("")) {
                        let _ = platform.send(response).await;
                    }
                }
                continue;
            }

            if event.content.trim() == "/sleep" {
                if self.capabilities.admin_users.contains(&event.author_id) || event.author_id == "apis_autonomy" {
                    let response = Response {
                        platform: event.platform.clone(),
                        target_scope: event.scope.clone(),
                        text: "💤 **Entering sleep cycle...** Training on accumulated golden examples and preference pairs.".to_string(),
                        is_telemetry: false,
                    };
                    if let Some(platform) = self.platforms.get(response.platform.split(':').next().unwrap_or("")) {
                        let _ = platform.send(response).await;
                    }
                    let sleep_cycle = self.sleep_cycle.clone();
                    tokio::spawn(async move {
                        match sleep_cycle.enter_sleep().await {
                            Ok(report) => {
                                tracing::info!("[SLEEP] ✅ Sleep cycle complete: {}", report);
                            }
                            Err(e) => {
                                tracing::error!("[SLEEP] ❌ Sleep cycle failed: {}", e);
                            }
                        }
                    });
                } else {
                    let response = Response {
                        platform: event.platform.clone(),
                        target_scope: event.scope.clone(),
                        text: "🚫 **Permission Denied.** Only configured HIVE Administrators can trigger sleep training.".to_string(),
                        is_telemetry: false,
                    };
                    if let Some(platform) = self.platforms.get(response.platform.split(':').next().unwrap_or("")) {
                        let _ = platform.send(response).await;
                    }
                }
                continue;
            }

            if event.content.trim() == "/stop" {
                self.stop_flag.store(true, Ordering::SeqCst);
                tracing::info!("[ENGINE] 🛑 /stop issued by {} — flag set", event.author_name);
                // No ack message — the react loop's exit response is the only user-facing message.
                continue;
            }

            // ─── SELF-MODERATION GATE ────────────────────────────────────────
            // Check if Apis has muted or rate-limited this user. Enforcement is
            // at the engine level so the LLM never even sees the event.
            if event.author_id != "apis_autonomy" {
                // Mute check — silently drop events from muted users
                if let Some(reason) = self.memory.moderation.is_muted(&event.author_id).await {
                    tracing::info!("[MODERATION] 🔇 Dropping event from muted user '{}'. Reason: {}", event.author_id, reason);
                    continue;
                }

                // Rate-limit check — skip if responding too frequently
                if let Some(wait_secs) = self.memory.moderation.check_rate_limit(&event.author_id).await {
                    tracing::info!("[MODERATION] ⏳ Rate-limited user '{}' — {} seconds remaining.", event.author_id, wait_secs);
                    continue;
                }
            }

            // 1. Retrieve working history for this specific scope
            let mut history = self.memory.get_working_history(&event.scope).await;
            tracing::debug!("[ENGINE] History retrieved for scope='{}': {} messages", event.scope.to_key(), history.len());
            
            let db_turn = history.len() / 2;
            let bg_synth_needed = db_turn > 0 && db_turn % 50 == 0;
            let mut bg_daily_needed = false;
            let mut bg_lifetime_needed = false;

            // Check daily/lifetime timelines
            {
                let t_data = self.memory.timelines.read(&event.scope).await;
                let now = chrono::Utc::now();
                let threshold = now - chrono::Duration::hours(24);
                if let Some(daily) = &t_data.last_24_hours {
                    if let Ok(last_dt) = chrono::DateTime::parse_from_rfc3339(&daily.generated_at) {
                        if last_dt.with_timezone(&chrono::Utc) < threshold {
                            bg_daily_needed = true;
                            bg_lifetime_needed = true;
                        }
                    }
                } else if db_turn > 20 {
                    bg_daily_needed = true;
                    bg_lifetime_needed = true;
                }
            }

            // NOTE: Synthesis is deferred until AFTER the ReAct loop completes.
            // Previously, synthesis spawned here concurrently with the ReAct loop,
            // causing concurrent provider.generate() calls that corrupted Ollama's
            // HTTP streaming responses ("error decoding response body").
            // Synthesis now runs inside the spawned user-event task, after delivery.

            // 2. Now store the incoming event in memory for future context
            self.memory.add_event(event.clone()).await;

            // 3. Check for Context Limit & Trigger Autosave
            if let Some(continuity_summary) = self.memory.check_and_trigger_autosave(&event.scope).await {
                tracing::info!("[ENGINE] 💾 Autosave triggered for scope='{}' — history reset to continuity summary", event.scope.to_key());
                // If an autosave happened, the history we retrieved in step 1 is stale and huge.
                // We must reset our history to JUST the continuity summary and the new event.
                history = vec![continuity_summary, event.clone()];
            }

            // 3. Telemetry — channel created inside each spawned task via spawn_telemetry_receiver()

            // 4. AUTONOMY PREEMPTION GATE
            // Autonomy events are spawned as background tasks, gated by the concurrency semaphore.
            if event.author_id == "apis_autonomy" {
                let platforms_bg = self.platforms.clone();
                let agent_bg = self.agent.clone();
                let provider_bg = self.resolve_provider(&event.platform);
                let memory_bg = self.memory.clone();
                let drives_bg = self.drives.clone();
                let capabilities_bg = self.capabilities.clone();
                let teacher_bg = self.teacher.clone();
                let autonomy_sender_bg = autonomy_sender.clone();
                let autonomy_handle_bg = autonomy_handle.clone();
                let semaphore_bg = self.concurrency_semaphore.clone();
                let autonomy_ch = self.autonomy_channel.clone();
                    let stop_flag_bg = self.stop_flag.clone();

                let pending_50 = self.pending_50_turn_synth.clone();
                let pending_daily = self.pending_daily_synth.clone();
                let pending_lifetime = self.pending_lifetime_synth.clone();
                let synth_provider = self.provider.clone();
                let synth_memory = self.memory.clone();
                let synth_scope = event.scope.clone();
                let synth_drives = self.drives.clone();

                active_autonomy_task = Some(tokio::spawn(async move {
                    // Acquire concurrency permit — waits if all slots are busy
                    let _permit = semaphore_bg.acquire().await.expect("Semaphore closed");
                    tracing::info!("[AUTONOMY] 🎫 Acquired inference slot. Starting autonomy ReAct loop.");

                    // ── SYNTHESIS PHASE: Run pending synthesis BEFORE autonomy ──
                    // Synthesis runs during idle time alongside autonomy, never during user chat.
                    // If a user messages during synthesis, the whole task gets aborted (preemption)
                    // and the pending flags stay true for the next autonomy trigger.
                    let need_50 = pending_50.swap(false, Ordering::Relaxed);
                    let need_daily = pending_daily.swap(false, Ordering::Relaxed);
                    let need_lifetime = pending_lifetime.swap(false, Ordering::Relaxed);
                    if need_50 || need_daily || need_lifetime {
                        tracing::info!("[SYNTHESIS] 🧬 Running deferred synthesis before autonomy (50-turn={}, daily={}, lifetime={})",
                            need_50, need_daily, need_lifetime);
                        if need_50 {
                            let _ = crate::agent::synthesis::synthesize_50_turn(synth_provider.clone(), synth_memory.clone(), synth_scope.clone(), Some(synth_drives.clone())).await;
                        }
                        if need_daily {
                            let _ = crate::agent::synthesis::synthesize_24_hr(synth_provider.clone(), synth_memory.clone(), synth_scope.clone(), Some(synth_drives.clone())).await;
                        }
                        if need_lifetime {
                            let _ = crate::agent::synthesis::synthesize_lifetime(synth_provider.clone(), synth_memory.clone(), synth_scope.clone(), Some(synth_drives.clone())).await;
                        }
                        tracing::info!("[SYNTHESIS] ✅ Deferred synthesis complete. Proceeding to autonomy ReAct loop.");
                    }

                    // Create telemetry channel INSIDE the spawned task
                    let telemetry_tx = spawn_telemetry_receiver(
                        platforms_bg.clone(), event.platform.clone(), event.scope.clone(),
                    );

                    let (response_text, current_turn, completed_tools) = crate::engine::react::execute_react_loop(
                        &event,
                        &history,
                        telemetry_tx.clone(),
                        &platforms_bg,
                        &agent_bg,
                        provider_bg,
                        memory_bg.clone(),
                        drives_bg,
                        capabilities_bg,
                        teacher_bg,
                        stop_flag_bg,
                    ).await;

                    let response = Response {
                        platform: event.platform.clone(),
                        target_scope: event.scope.clone(),
                        text: response_text.clone(),
                        is_telemetry: false,
                    };

                    // Store response in memory
                    let apis_event = Event {
                        platform: response.platform.clone(),
                        scope: response.target_scope.clone(),
                        author_name: "Apis".to_string(),
                        author_id: "test".into(),
                        content: response.text.clone(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
                    };
                    memory_bg.add_event(apis_event).await;

                    // Route response to platform
                    if let Some(platform) = platforms_bg.get(response.platform.split(':').next().unwrap_or("")) {
                        if let Err(e) = platform.send(response).await {
                            tracing::error!("[AUTONOMY] Error sending response: {}", e);
                        }
                    }

                    // Log autonomy session
                    let log_entry = serde_json::json!({
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                        "turn_count": current_turn,
                        "tools_used": completed_tools.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(),
                        "summary": response_text.clone(),
                    });
                    tokio::spawn(async move {
                        let dir = std::path::Path::new("memory/autonomy");
                        let _ = tokio::fs::create_dir_all(dir).await;
                        let path = dir.join("activity.jsonl");
                        let line = format!("{}\n", log_entry);
                        if let Ok(mut f) = tokio::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&path)
                            .await
                        {
                            use tokio::io::AsyncWriteExt;
                            let _ = f.write_all(line.as_bytes()).await;
                        }
                    });

                    // _permit is dropped here, releasing the inference slot

                    // Restart autonomy timer after completion
                    if let Some(ref sender) = autonomy_sender_bg {
                      if !autonomy_ch.is_empty() {
                        let sender_clone = sender.clone();
                        let memory_clone = memory_bg.clone();
                        let autonomy_handle_bg_inner = autonomy_handle_bg.clone();
                        let handle = tokio::spawn(async move {
                            tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
                            tracing::info!("[AUTONOMY] 🐝 5-minute idle timer fired. Entering Continuous Autonomy mode.");
                            let public_narrative = memory_clone.get_public_narrative().await;
                            let previous_sessions = load_recent_autonomy_sessions(10).await;
                            let recompile_history = load_recompile_history(5).await;
                            let autonomy_event = Event {
                                platform: format!("discord:{}:0:autonomy_{}", autonomy_ch, chrono::Utc::now().timestamp()),
                                scope: Scope::Public {
                                    channel_id: autonomy_ch.clone(),
                                    user_id: "apis_autonomy".to_string(),
                                },
                                author_name: "Apis".to_string(),
                                author_id: "apis_autonomy".into(),
                                content: format!(
                                    "═══ PRIVATE INTERNAL SYSTEM OPERATION ═══\n\
                                    You are now in Continuous Autonomy mode. This is a PRIVATE, INTERNAL background process.\n\
                                    You are NOT in a conversation. No users can see you. No users are talking to you.\n\n\
                                    {}\n\n\
                                    {}\n\n\
                                    {}\n\n\
                                    ═══ AUTONOMY DIRECTIVES ═══\n\
                                    1. This is a PRIVATE SYSTEM OPERATION. Do NOT address, reply to, or acknowledge any users.\n\
                                    2. The external context above is READ-ONLY awareness. Those are NOT messages for you.\n\
                                    3. DIVERSIFY: Do NOT repeat work from previous sessions listed above.\n\
                                    4. Use tools productively: review memory, consolidate lessons, practice skills, explore the Turing Grid, run routines, research, audit logs, or self-improve.\n\
                                    5. Report your internal monologue and completed actions in your final reply.",
                                    public_narrative,
                                    previous_sessions,
                                    recompile_history
                                ),
                                timestamp: Some(chrono::Utc::now().to_rfc3339()),
                                message_index: None,
                            };
                            let _ = sender_clone.send(autonomy_event).await;
                        });
                        let mut guard = autonomy_handle_bg_inner.lock().await;
                        *guard = Some(handle);
                      } // autonomy_ch guard
                    }
                }));

                tracing::info!("[AUTONOMY] 🐝 Spawned autonomy ReAct loop as preemptible background task.");
                continue; // Don't block — immediately return to listening for events
            }

            // 4b. PARALLEL USER EVENT PROCESSING
            // Spawn each user event as a background task, gated by:
            //   1. Global concurrency semaphore (respects OLLAMA_NUM_PARALLEL)
            //   2. Per-scope serialization lock (prevents history read/write race conditions)
            {
                let platforms_bg = self.platforms.clone();
                let agent_bg = self.agent.clone();
                let provider_bg = self.resolve_provider(&event.platform);
                let memory_bg = self.memory.clone();
                let drives_bg = self.drives.clone();
                let capabilities_bg = self.capabilities.clone();
                let teacher_bg = self.teacher.clone();
                let sleep_cycle_bg = self.sleep_cycle.clone();
                let semaphore_bg = self.concurrency_semaphore.clone();
                let scope_locks_bg = self.scope_locks.clone();
                let autonomy_sender_bg = autonomy_sender.clone();
                let autonomy_handle_bg = autonomy_handle.clone();
                let pending_50_bg = self.pending_50_turn_synth.clone();
                let pending_daily_bg = self.pending_daily_synth.clone();
                let pending_lifetime_bg = self.pending_lifetime_synth.clone();

                let available = self.concurrency_semaphore.available_permits();
                tracing::info!("[PARALLEL] 🐝 Spawning event from {} (scope: {}) — {}/{} inference slots available",
                    event.author_name, event.scope.to_key(),
                    available, self.concurrency_semaphore.available_permits());

                let stop_flag_bg = self.stop_flag.clone();
                let autonomy_ch = self.autonomy_channel.clone();

                tokio::spawn(async move {
                    // 1. Acquire per-scope lock — serializes events within the same channel/DM
                    let scope_key = event.scope.to_key();
                    let scope_lock = {
                        let mut locks = scope_locks_bg.write().await;
                        locks.entry(scope_key.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
                    };
                    let _scope_guard = scope_lock.lock().await;

                    // 2. Acquire global semaphore — waits if all inference slots are busy
                    let _permit = semaphore_bg.acquire().await.expect("Semaphore closed");
                    tracing::info!("[PARALLEL] 🎫 Acquired inference slot for {} (scope: {})", event.author_name, scope_key);

                    // 3. Create telemetry channel INSIDE the spawned task
                    let telemetry_tx = spawn_telemetry_receiver(
                        platforms_bg.clone(), event.platform.clone(), event.scope.clone(),
                    );


                    let (response_text, _current_turn, _completed_tools) = crate::engine::react::execute_react_loop(
                        &event,
                        &history,
                        telemetry_tx.clone(),
                        &platforms_bg,
                        &agent_bg,
                        provider_bg,
                        memory_bg.clone(),
                        drives_bg,
                        capabilities_bg,
                        teacher_bg.clone(),
                        stop_flag_bg,
                    ).await;

                    let response = Response {
                        platform: event.platform.clone(),
                        target_scope: event.scope.clone(),
                        text: response_text.clone(),
                        is_telemetry: false,
                    };

                    // 6. Store Apis's response in memory so it remembers what it said
                    let apis_event = Event {
                        platform: response.platform.clone(),
                        scope: response.target_scope.clone(),
                        author_name: "Apis".to_string(),
                        author_id: "test".into(),
                        content: response.text.clone(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
                    };
                    memory_bg.add_event(apis_event).await;

                    // Record rate-limit timestamp (updates last_response_at for throttling)
                    memory_bg.moderation.record_response(&event.author_id).await;

                    // 7. Route final response back to the platform it came from
                    if let Some(platform) = platforms_bg.get(response.platform.split(':').next().unwrap_or("")) {
                        if let Err(e) = platform.send(response).await {
                            tracing::error!("Error sending response to {}: {}", platform.name(), e);
                        }
                    } else {
                        tracing::warn!("Received event from unknown platform: {}", event.platform);
                    }

                    // ── RELEASE LOCKS BEFORE POST-DELIVERY WORK ──────────
                    drop(_scope_guard);
                    drop(_permit);

                    // 7.4. Synthesis flags — mark pending, consumed by next autonomy trigger.
                    // Synthesis no longer runs here. It runs during idle time (autonomy lifecycle)
                    // to avoid GPU contention with user chat.
                    if bg_synth_needed {
                        pending_50_bg.store(true, Ordering::Relaxed);
                    }
                    if bg_daily_needed {
                        pending_daily_bg.store(true, Ordering::Relaxed);
                    }
                    if bg_lifetime_needed {
                        pending_lifetime_bg.store(true, Ordering::Relaxed);
                    }

                    // 7.5. Spawn Continuous Autonomy timer (5 min)
                    if event.author_name != "Apis" {
                        if let Some(ref sender) = autonomy_sender_bg {
                          if !autonomy_ch.is_empty() {
                            let sender_clone = sender.clone();
                            let memory_clone = memory_bg.clone();
                            let autonomy_handle_bg_inner = autonomy_handle_bg.clone();
                            let handle = tokio::spawn(async move {
                                tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
                                tracing::info!("[AUTONOMY] 🐝 5-minute idle timer fired. Entering Continuous Autonomy mode.");
                                
                                let public_narrative = memory_clone.get_public_narrative().await;
                                let previous_sessions = load_recent_autonomy_sessions(10).await;
                                
                                let autonomy_event = Event {
                                    platform: format!("discord:{}:0:autonomy_{}", autonomy_ch, chrono::Utc::now().timestamp()),
                                    scope: Scope::Public {
                                        channel_id: autonomy_ch.clone(),
                                        user_id: "apis_autonomy".to_string(),
                                    },
                                    author_name: "Apis".to_string(),
                                    author_id: "apis_autonomy".into(),
                                    content: format!(
                                        "═══ PRIVATE INTERNAL SYSTEM OPERATION ═══\n\
                                        You are now in Continuous Autonomy mode. This is a PRIVATE, INTERNAL background process.\n\
                                        You are NOT in a conversation. No users can see you. No users are talking to you.\n\n\
                                        {}\n\n\
                                        {}\n\n\
                                        ═══ AUTONOMY DIRECTIVES ═══\n\
                                        1. This is a PRIVATE SYSTEM OPERATION. Do NOT address, reply to, or acknowledge any users.\n\
                                        2. The external context above is READ-ONLY awareness. Those are NOT messages for you.\n\
                                        3. DIVERSIFY: Do NOT repeat work from previous sessions listed above.\n\
                                        4. Use tools productively: review memory, consolidate lessons, practice skills, explore the Turing Grid, run routines, research, audit logs, or self-improve.\n\
                                        5. Report your internal monologue and completed actions in your final reply.",
                                        public_narrative,
                                        previous_sessions
                                    ),
                                    timestamp: Some(chrono::Utc::now().to_rfc3339()),
                                    message_index: None,
                                };
                                let _ = sender_clone.send(autonomy_event).await;
                            });
                            let mut guard = autonomy_handle_bg_inner.lock().await;
                            *guard = Some(handle);
                          } // autonomy_ch guard
                        }
                    }

                    // 8. Sleep Training Timer — replaces old threshold-based trigger
                    if teacher_bg.auto_train_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                        let sleep_bg = sleep_cycle_bg.clone();
                        let tx_clone = telemetry_tx.clone();
                        if sleep_bg.should_auto_sleep().await {
                            tokio::spawn(async move {
                                let (golden, pairs) = sleep_bg.teacher.get_counts();
                                let _ = tx_clone.send(format!(
                                    "\n💤 **Apis is sleeping...** Dreaming of {} golden examples and {} preference pairs.",
                                    golden, pairs
                                )).await;

                                match sleep_bg.enter_sleep().await {
                                    Ok(report) => {
                                        let _ = tx_clone.send(format!("\n☀️ **{}**", report)).await;
                                    }
                                    Err(e) => {
                                        tracing::error!("[SLEEP] ❌ Sleep cycle failed: {}", e);
                                        let _ = tx_clone.send(format!("\n❌ **Sleep failed:** {}", e)).await;
                                    }
                                }
                            });
                        }
                    }
                });
            }
        }
    }

}

