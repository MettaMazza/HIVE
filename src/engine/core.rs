#![allow(clippy::redundant_field_names, clippy::collapsible_if)]

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::engine::drives;
use crate::engine::inbox;
use crate::engine::outreach;
use crate::models::message::{Event, Response};
use crate::models::capabilities::AgentCapabilities;
use crate::models::scope::Scope;

use crate::memory::MemoryStore;
use crate::platforms::Platform;
use crate::providers::Provider;
use crate::teacher::Teacher;

/// Format elapsed seconds as a human-readable string.
pub fn format_elapsed(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        format!("{}s", elapsed_secs)
    } else {
        format!("{:.1}m", elapsed_secs as f64 / 60.0)
    }
}

use crate::agent::AgentManager;




pub struct Engine {
    pub platforms: Arc<HashMap<String, Box<dyn Platform>>>,
    pub provider: Arc<dyn Provider>,
    pub capabilities: Arc<AgentCapabilities>,
    pub memory: Arc<MemoryStore>,
    pub agent: Arc<AgentManager>,
    pub teacher: Arc<Teacher>,
    
    #[allow(dead_code)]
    pub drives: Arc<drives::DriveSystem>,
    #[allow(dead_code)]
    pub outreach_gate: Arc<outreach::OutreachGate>,
    #[allow(dead_code)]
    pub inbox: Arc<inbox::InboxManager>,
    
    pub event_sender: Option<mpsc::Sender<Event>>,
    pub event_receiver: mpsc::Receiver<Event>,
}

impl Engine {
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
        Self {
            platforms, provider, capabilities, memory, agent, teacher, drives, outreach_gate, inbox, event_sender, event_receiver
        }
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
        drop(sender);

        tracing::info!("HIVE is active. Apis is listening.");

        // Autonomy loop: self-event timer after 5 min idle
        let mut autonomy_handle: Option<tokio::task::JoinHandle<()>> = None;

        // Main Event Loop
        while let Some(event) = self.event_receiver.recv().await {

            // Cancel any pending autonomy timer when a real event arrives
            if let Some(handle) = autonomy_handle.take() {
                handle.abort();
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

            // 1. Retrieve working history for this specific scope
            let mut history = self.memory.get_working_history(&event.scope).await;
            
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

            if bg_synth_needed || bg_daily_needed {
                let prov_clone = self.provider.clone();
                let mem_clone = self.memory.clone();
                let scope_clone = event.scope.clone();
                tokio::spawn(async move {
                    if bg_synth_needed {
                        let _ = crate::agent::synthesis::synthesize_50_turn(prov_clone.clone(), mem_clone.clone(), scope_clone.clone()).await;
                    }
                    if bg_daily_needed {
                        let _ = crate::agent::synthesis::synthesize_24_hr(prov_clone.clone(), mem_clone.clone(), scope_clone.clone()).await;
                    }
                    if bg_lifetime_needed {
                        let _ = crate::agent::synthesis::synthesize_lifetime(prov_clone.clone(), mem_clone.clone(), scope_clone.clone()).await;
                    }
                });
            }

            // 2. Now store the incoming event in memory for future context
            self.memory.add_event(event.clone()).await;

            // 3. Check for Context Limit & Trigger Autosave
            if let Some(continuity_summary) = self.memory.check_and_trigger_autosave(&event.scope).await {
                // If an autosave happened, the history we retrieved in step 1 is stale and huge.
                // We must reset our history to JUST the continuity summary and the new event.
                history = vec![continuity_summary, event.clone()];
            }

            // 3. Setup Telemetry Channel for Live Updates (ErnOS CognitionTracker pattern)
            let (telemetry_tx, mut telemetry_rx) = mpsc::channel::<String>(50);
            
            let platforms_ref = self.platforms.clone();
            let platform_id_clone = event.platform.clone();
            let scope_clone = event.scope.clone();
            
            // Spawn debounced telemetry task (800ms interval, matching ErnOS)
            tokio::spawn(async move {
                let start_time = tokio::time::Instant::now();
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

                loop {
                    let recv_result = tokio::time::timeout(
                        tokio::time::Duration::from_millis(debounce_ms),
                        telemetry_rx.recv()
                    ).await;

                    match recv_result {
                        Ok(Some(chunk)) => {
                            // Accumulate actual thinking tokens
                            buffered_thought.push_str(&chunk);
                            has_update = true;
                        }
                        Ok(None) => {
                            // Channel closed — provider finished
                            break;
                        }
                        Err(_) => {
                            // Debounce timeout — flush update with accumulated thinking text
                            if has_update {
                                let elapsed_str = format_elapsed(start_time.elapsed().as_secs());
                                let current_quirk = quirks[start_time.elapsed().as_millis() as usize % quirks.len()];
                                let status = format!("{} ({})\n\n{}", current_quirk, elapsed_str, buffered_thought);
                                let update_res = Response {
                                    platform: platform_id_clone.clone(),
                                    target_scope: scope_clone.clone(),
                                    text: status,
                                    is_telemetry: true,
                                };
                                if let Some(platform) = platforms_ref.get(update_res.platform.split(':').next().unwrap_or("")) {
                                    let _ = platform.send(update_res).await;
                                }
                                has_update = false;
                            }
                        }
                    }
                }

                // Channel closed: send final "complete" telemetry with full reasoning
                let elapsed_str = format_elapsed(start_time.elapsed().as_secs());
                let status = if buffered_thought.is_empty() {
                    format!("✅ Complete ({})", elapsed_str)
                } else {
                    format!("✅ Complete ({})\n\n{}", elapsed_str, buffered_thought)
                };
                let update_res = Response {
                    platform: platform_id_clone.clone(),
                    target_scope: scope_clone.clone(),
                    text: status,
                    is_telemetry: true,
                };
                if let Some(platform) = platforms_ref.get(update_res.platform.split(':').next().unwrap_or("")) {
                    let _ = platform.send(update_res).await;
                }
            });

            // 4. Multi-Turn Agentic Action Loop
            let react_fut = crate::engine::react::execute_react_loop(
                &event,
                &history,
                telemetry_tx.clone(),
                &self.platforms,
                &self.agent,
                self.provider.clone(),
                self.memory.clone(),
                self.drives.clone(),
                self.capabilities.clone(),
                self.teacher.clone(),
            );

            let (response_text, current_turn, completed_tools) = if event.author_id == "apis_autonomy" {
                let interrupt_flag = self.memory.interrupt_autonomy.clone();
                let interrupt_flag_ret = self.memory.interrupt_autonomy.clone();
                tokio::select! {
                    res = react_fut => res,
                    _ = async move {
                        loop {
                            if interrupt_flag.load(std::sync::atomic::Ordering::Relaxed) {
                                break;
                            }
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        }
                    } => {
                        interrupt_flag_ret.store(false, std::sync::atomic::Ordering::Relaxed);
                        tracing::info!("🛑 Core Engine: Aborted active autonomy execution because a user message arrived.");
                        ("🐝 *Autonomy session interrupted by incoming user.*".to_string(), 1, vec![])
                    }
                }
            } else {
                react_fut.await
            };

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
            };
            self.memory.add_event(apis_event).await;

            // 7. Route final response back to the platform it came from
            if let Some(platform) = self.platforms.get(response.platform.split(':').next().unwrap_or("")) {
                if let Err(e) = platform.send(response).await {
                    tracing::error!("Error sending response to {}: {}", platform.name(), e);
                }
            } else {
                tracing::warn!("Received event from unknown platform: {}", response.platform);
            }

            // 7.1. Log autonomy sessions for later introspection
            if event.author_id == "apis_autonomy" {
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
            }

            // 7.5. Spawn Continuous Autonomy timer (5 min)
            // After Apis responds, wait 5 min then inject a self-event as if Apis is a user.
            // Skip autonomy for events that Apis herself generated (prevent infinite loops).
            if event.author_name != "Apis" {
                if let Some(ref sender) = autonomy_sender {
                    let sender_clone = sender.clone();
                    let memory_clone = self.memory.clone();
                    autonomy_handle = Some(tokio::spawn(async move {
                        tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
                        tracing::info!("[AUTONOMY] 🐝 5-minute idle timer fired. Entering Continuous Autonomy mode.");
                        
                        // Build context about public engagements for diversity
                        let public_narrative = memory_clone.get_public_narrative().await;
                        
                        let autonomy_event = Event {
                            platform: "discord:1480192647657427044".to_string(),
                            scope: Scope::Public {
                                channel_id: "1480192647657427044".to_string(),
                                user_id: "apis_autonomy".to_string(),
                            },
                            author_name: "Apis".to_string(),
                            author_id: "apis_autonomy".into(),
                            content: format!(
                                "You are now in Continuous Autonomy mode. You have 5 minutes of idle time.\n\n\
                                {}\n\n\
                                CRITICAL DIRECTIVE: This is an UNSUPERVISED, INTERNAL AUTONOMY SESSION. You are alone. \
                                You MUST NOT speak to users, you MUST NOT write conversational stories about them, and you MUST NOT address anyone directly. \
                                Do NOT respond to any previous conversations or messages in the history. \n\
                                This is your private internal monologue and system status update. \
                                \n\n\
                                Use this time productively. Consider the public engagement narrative above and diversify your autonomous work. \
                                Ideas: review your memory, consolidate lessons, practice skills, explore your Turing Grid, run routines, \
                                do research, or anything you find valuable for self-improvement. \
                                Report your internal monologue and actions in your final reply. \n\
                                You MUST start your reply exactly like this: 'Autonomy session complete. Here is what I accomplished:'",
                                public_narrative
                            ),
                        };
                        let _ = sender_clone.send(autonomy_event).await;
                    }));
                }
            }

            // 8. Background Self-Supervised Training Trigger
            let (golden_count, pair_count) = self.teacher.get_counts();
            if golden_count >= crate::teacher::GOLDEN_THRESHOLD || pair_count >= crate::teacher::PAIR_THRESHOLD {
                if self.teacher.auto_train_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                    let teacher_clone = self.teacher.clone();
                    let tx_clone = telemetry_tx.clone();
                    
                    // Spawn the training process in a detached background task
                    tokio::spawn(async move {
                        if teacher_clone.try_acquire_training_lock().await {
                            let _ = tx_clone.send(format!("\n⚙️ **[TEACHER MODULE]** Threshold reached (Golden: {}, Pairs: {}). Background MLX LoRA training initiated...", golden_count, pair_count)).await;
                            tracing::info!("[TEACHER] Threshold reached. Spawning Python MLX training pipeline...");
                            
                            // Reset counters immediately so we don't trigger again while training
                            teacher_clone.reset_counts();

                            // Execute python3 training/train_teacher.py
                            let output = std::process::Command::new("python3")
                                .arg("training/train_teacher.py")
                                .output();

                            match output {
                                Ok(res) => {
                                    let stdout = String::from_utf8_lossy(&res.stdout);
                                    let stderr = String::from_utf8_lossy(&res.stderr);
                                    if res.status.success() {
                                        println!("[TEACHER] ✅ Training complete:\n{}", stdout);
                                        let _ = tx_clone.send("\n✅ **[TEACHER MODULE]** Training complete. New weights registered and ready.".to_string()).await;
                                    } else {
                                        eprintln!("[TEACHER] ❌ Training failed:\nSTDOUT:\n{}\nSTDERR:\n{}", stdout, stderr);
                                        let _ = tx_clone.send("\n❌ **[TEACHER MODULE]** Training script failed. Check HIVE console logs.".to_string()).await;
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("[TEACHER] ❌ Failed to execute Python training script: {}", e);
                                    let _ = tx_clone.send("\n❌ **[TEACHER MODULE]** Failed to execute Python script. Is python3 installed?".to_string()).await;
                                }
                            }

                            teacher_clone.release_training_lock().await;
                        }
                    });
                } else {
                    tracing::debug!("[TEACHER] Training threshold reached (Golden: {}, Pairs: {}), but auto-tuning is toggled off.", golden_count, pair_count);
                }
            }
        }
    }

}

