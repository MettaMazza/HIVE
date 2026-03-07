use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::models::message::{Event, Response};
use crate::models::capabilities::AgentCapabilities;
use crate::models::scope::Scope;

use crate::memory::MemoryStore;
use crate::platforms::Platform;
use crate::providers::Provider;
use crate::teacher::Teacher;

/// Format elapsed seconds as a human-readable string.
fn format_elapsed(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        format!("{}s", elapsed_secs)
    } else {
        format!("{:.1}m", elapsed_secs as f64 / 60.0)
    }
}

use crate::swarm::SwarmManager;
use crate::swarm::planner::PLANNER_SYSTEM_PROMPT;

pub struct EngineBuilder {
    platforms: HashMap<String, Box<dyn Platform>>,
    provider: Option<Arc<dyn Provider>>,
    capabilities: AgentCapabilities,
    memory: MemoryStore,
    swarm: Option<Arc<SwarmManager>>,
}

impl EngineBuilder {
    pub fn new() -> Self {
        Self {
            platforms: HashMap::new(),
            provider: None,
            capabilities: AgentCapabilities::default(),
            memory: MemoryStore::new(None),
            swarm: None,
        }
    }

    pub fn with_platform(mut self, platform: Box<dyn Platform>) -> Self {
        self.platforms.insert(platform.name().to_string(), platform);
        self
    }

    pub fn with_capabilities(mut self, capabilities: AgentCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn with_provider(mut self, provider: Arc<dyn Provider>) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Injects a custom testing MemoryStore instead of the default global `memory/` path
    #[cfg(test)]
    pub fn with_memory(mut self, mem: MemoryStore) -> Self {
        self.memory = mem;
        self
    }
    
    /// Injects a pre-configured SwarmManager (e.g., dynamically built native drones)
    pub fn with_swarm(mut self, swarm: Arc<SwarmManager>) -> Self {
        self.swarm = Some(swarm);
        self
    }
    
    pub fn build(self) -> Result<Engine, &'static str> {
        let provider = self.provider.ok_or("Engine requires a Provider to be set")?;
        let (tx, rx) = mpsc::channel(100);
        
        let memory = Arc::new(self.memory);
        
        let swarm = match self.swarm {
            Some(s) => s,
            None => Arc::new(SwarmManager::new(provider.clone(), memory.clone())),
        };

        Ok(Engine {
            platforms: Arc::new(self.platforms),
            provider: provider.clone(),
            capabilities: Arc::new(self.capabilities),
            memory: memory,
            swarm: swarm,
            teacher: Arc::new(Teacher::new(None)),
            event_sender: Some(tx),
            event_receiver: rx,
        })
    }
}

pub struct Engine {
    platforms: Arc<HashMap<String, Box<dyn Platform>>>,
    provider: Arc<dyn Provider>,
    capabilities: Arc<AgentCapabilities>,
    memory: Arc<MemoryStore>,
    swarm: Arc<SwarmManager>,
    teacher: Arc<Teacher>,
    
    // Channel for platforms to send events IN to the engine
    event_sender: Option<mpsc::Sender<Event>>,
    // The engine receives them here
    event_receiver: mpsc::Receiver<Event>,
}

impl Engine {
    #[cfg(not(tarpaulin_include))]
    pub async fn run(mut self) {
        println!("Starting HIVE Engine...");
        
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
        
        drop(sender);

        println!("HIVE is active. Apis is listening.");

        // Main Event Loop
        while let Some(event) = self.event_receiver.recv().await {
            
            // 0. Intercept System Commands (/clean)
            if event.content.trim() == "/clean" {
                if self.capabilities.admin_users.contains(&event.author_id) {
                    println!("[ADMIN COMMAND] Executing Factory Memory Wipe initiated by UID: {}", event.author_id);
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
                    println!("Memory wipe complete. HIVE Engine shutting down.");
                    std::process::exit(0);
                } else {
                    println!("[SECURITY INCIDENT] Unauthorized wipe attempt by UID: {}", event.author_id);
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

            // 1. Retrieve working history for this specific scope
            let mut history = self.memory.get_working_history(&event.scope).await;

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
                                let status = format!("🧠 Thinking... ({})\n\n{}", elapsed_str, buffered_thought);
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

            // 4. Swarm Planning Pass
            let drone_list = self.swarm.get_available_drones_text();
            let mut planner_prompt = crate::prompts::SystemPromptBuilder::assemble(&event.scope, self.memory.clone()).await;
            planner_prompt.push_str("\n\n");
            planner_prompt.push_str(&PLANNER_SYSTEM_PROMPT.replace("{available_drones}", &drone_list));
            let plan_json = match self.provider.generate(&planner_prompt, &history, &event, Some(telemetry_tx.clone())).await {
                Ok(text) => text,
                Err(e) => {
                    eprintln!("Planner Failed: {:?}", e);
                    "{}".to_string() // Fallback to empty plan
                }
            };
            
            // 4b. JSON Repair Catcher — catches malformed Planner output, retries up to 3 times
            let mut context_from_swarm = String::new();
            let mut parsed_plan: Option<crate::swarm::planner::SwarmPlan> = None;
            let max_planner_retries: usize = 3;

            // Attempt 1: repair and parse initial output
            let cleaned_json = Self::repair_planner_json(&plan_json);
            match serde_json::from_str::<crate::swarm::planner::SwarmPlan>(&cleaned_json) {
                Ok(plan) => { parsed_plan = Some(plan); }
                Err(first_err) => {
                    eprintln!("[PLANNER CATCHER] ⚠️ Malformed JSON from Planner (attempt 1)");
                    eprintln!("[PLANNER CATCHER] Error: {}", first_err);
                    eprintln!("[PLANNER CATCHER] Raw output: {}", plan_json.chars().take(500).collect::<String>());

                    // Retry loop — keep re-prompting until we get valid JSON
                    let mut last_bad_output = plan_json.clone();
                    let mut last_error = first_err.to_string();
                    
                    for attempt in 2..=max_planner_retries {
                        let retry_prompt = format!(
                            "{}\n\n[CRITICAL ERROR — ATTEMPT {} OF {} — YOUR PREVIOUS OUTPUT WAS NOT VALID JSON]\nParse error: {}\nYour raw output started with:\n{}\n\nYou MUST output ONLY a valid JSON object. No markdown fences. No conversational text. No explanation. JUST the JSON:\n{{\"tasks\": [...]}}",
                            planner_prompt, attempt, max_planner_retries, last_error, last_bad_output.chars().take(200).collect::<String>()
                        );

                        match self.provider.generate(&retry_prompt, &history, &event, Some(telemetry_tx.clone())).await {
                            Ok(retry_text) => {
                                let retry_cleaned = Self::repair_planner_json(&retry_text);
                                match serde_json::from_str::<crate::swarm::planner::SwarmPlan>(&retry_cleaned) {
                                    Ok(plan) => {
                                        println!("[PLANNER CATCHER] ✅ Recovered on attempt {} — plan parsed successfully", attempt);
                                        parsed_plan = Some(plan);
                                        break;
                                    }
                                    Err(e) => {
                                        eprintln!("[PLANNER CATCHER] ⚠️ Still malformed (attempt {}): {}", attempt, e);
                                        last_bad_output = retry_text;
                                        last_error = e.to_string();
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("[PLANNER CATCHER] ⚠️ Provider error on attempt {}: {:?}", attempt, e);
                                last_error = format!("{:?}", e);
                            }
                        }
                    }

                    if parsed_plan.is_none() {
                        eprintln!("[PLANNER CATCHER] ❌ All {} attempts failed. Falling back to empty plan.", max_planner_retries);
                        parsed_plan = Some(crate::swarm::planner::SwarmPlan { tasks: vec![] });
                    }
                }
            };

            if let Some(plan) = parsed_plan {
                if !plan.tasks.is_empty() {
                    // Execute parallel swarm
                    let tx_clone = telemetry_tx.clone();
                    let drone_results = self.swarm.execute_plan(plan, &event.content, Some(tx_clone)).await;
                    
                    // Aggregate results for the final assembler
                    context_from_swarm.push_str("\n\n[YOUR TOOLS HAVE ALREADY EXECUTED — USE THESE RESULTS TO ANSWER THE USER]\nThe following tool results were gathered by your Swarm drones BEFORE this response. DO NOT say 'let me read the files' or 'let me check' — the tools already ran. Use the output below to formulate your answer directly.\n");
                    let result_count = drone_results.len();
                    for res in &drone_results {
                        context_from_swarm.push_str(&format!("Task {}: {:?}\nOutput: {}\n\n", res.task_id, res.status, res.output));
                    }
                    println!("[SWARM] ✅ {} tool results aggregated for Observer context", result_count);
                } else {
                    println!("[PLANNER] No tasks planned (simple conversation)");
                }
            }

            // 5. Generate Final Apis Assembler Prompt & Call Provider
            let system_prompt_base = crate::prompts::SystemPromptBuilder::assemble(&event.scope, self.memory.clone()).await;
            let system_prompt = format!("{}{}", system_prompt_base, context_from_swarm);
            
            let final_response_text;
            let mut extra_guidance = String::new();
            let mut observer_attempts: usize = 0;
            let mut all_rejections: Vec<(String, String, String)> = vec![];

            loop {
                let active_prompt = format!("{}{}", extra_guidance, system_prompt);
                
                // DO NOT pass the telemetry_tx channel here.
                // If we stream this generation, the user sees the response before the Observer even audits it.
                // Telemetry is strictly for the Swarm Planner internal thoughts.
                let candidate_text = match self.provider.generate(&active_prompt, &history, &event, None).await {
                    Ok(text) => text,
                    Err(e) => {
                        eprintln!("Provider Error: {:?}", e);
                        format!("*System Error:* Something went wrong. ({})", e)
                    }
                };

                // Fast path for hard errors
                if candidate_text.starts_with("*System Error:*") {
                    final_response_text = candidate_text;
                    break;
                }

                observer_attempts += 1;

                // Run 1:1 Shadow Observer Audit
                let audit_result = crate::prompts::observer::run_skeptic_audit(
                    self.provider.clone(),
                    &self.capabilities,
                    &candidate_text,
                    &active_prompt,
                    &history,
                    &event,
                    &context_from_swarm
                ).await;

                if audit_result.is_allowed() {
                    // Teacher capture: only Public scope
                    if matches!(event.scope, Scope::Public { .. }) {
                        if observer_attempts == 1 {
                            // 🏆 GOLDEN: Perfect first-pass
                            self.teacher.capture_golden(
                                &system_prompt, &event, &context_from_swarm, &candidate_text
                            ).await;
                        } else {
                            // ⚖️ ORPO: Every rejection becomes a preference pair
                            for (idx, (rejected, category, reason)) in all_rejections.iter().enumerate() {
                                self.teacher.capture_preference_pair(
                                    &system_prompt, &event,
                                    rejected, &candidate_text,
                                    category, reason,
                                    idx + 1, observer_attempts,
                                ).await;
                            }
                        }
                    }
                    final_response_text = candidate_text;
                    break;
                } else {
                    // Store ALL rejections for multi-signal training
                    all_rejections.push((
                        candidate_text.clone(),
                        audit_result.failure_category.clone(),
                        audit_result.what_went_wrong.clone(),
                    ));
                    println!("[OBSERVER BLOCKED]\nCategory: {}\nWhat Worked: {}\nWhat Went Wrong: {}\nHow to Fix: {}", audit_result.failure_category, audit_result.what_worked, audit_result.what_went_wrong, audit_result.how_to_fix);
                    extra_guidance = format!("[OBSERVER GUIDANCE - CORRECTION REQUIRED]\nFAILURE CATEGORY: {}\nWHAT WORKED: {}\nWHAT WENT WRONG: {}\nHOW TO FIX: {}\n\n", audit_result.failure_category, audit_result.what_worked, audit_result.what_went_wrong, audit_result.how_to_fix);
                    
                    // Broadcast the Observer block to the frontend so the user knows why it's taking so long
                    let msg = format!("\n🛑 **[OBSERVER BLOCKED GENERATION]**\n**Category:** {}\n**Violation:** {}\n**Fixing...**", audit_result.failure_category, audit_result.what_went_wrong);
                    let _ = telemetry_tx.send(msg).await;
                    // Loops infinitely until the LLM complies with the Skeptic rules
                }
            }

            let response_text = final_response_text;

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
                    eprintln!("Error sending response to {}: {}", platform.name(), e);
                }
            } else {
                eprintln!("Received event from unknown platform: {}", response.platform);
            }

            // 8. Background Self-Supervised Training Trigger
            let (golden_count, pair_count) = self.teacher.get_counts();
            if golden_count >= crate::teacher::GOLDEN_THRESHOLD || pair_count >= crate::teacher::PAIR_THRESHOLD {
                let teacher_clone = self.teacher.clone();
                let tx_clone = telemetry_tx.clone();
                
                // Spawn the training process in a detached background task
                tokio::spawn(async move {
                    if teacher_clone.try_acquire_training_lock().await {
                        let _ = tx_clone.send(format!("\n⚙️ **[TEACHER MODULE]** Threshold reached (Golden: {}, Pairs: {}). Background MLX LoRA training initiated...", golden_count, pair_count)).await;
                        println!("[TEACHER] Threshold reached. Spawning Python MLX training pipeline...");
                        
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
                                eprintln!("[TEACHER] ❌ Failed to execute Python training script: {}", e);
                                let _ = tx_clone.send("\n❌ **[TEACHER MODULE]** Failed to execute Python script. Is python3 installed?".to_string()).await;
                            }
                        }

                        teacher_clone.release_training_lock().await;
                    }
                });
            }
        }
    }

    /// Repair common LLM JSON malformations from the Planner output.
    /// Strips markdown fences, BOM, trailing commas, and conversational preamble.
    fn repair_planner_json(raw: &str) -> String {
        let mut s = raw.trim().to_string();

        // Strip BOM
        s = s.trim_start_matches('\u{feff}').to_string();

        // Strip markdown code fences (```json ... ``` or ``` ... ```)
        if s.starts_with("```json") {
            s = s.strip_prefix("```json").unwrap_or(&s).to_string();
        } else if s.starts_with("```") {
            s = s.strip_prefix("```").unwrap_or(&s).to_string();
        }
        if s.ends_with("```") {
            s = s.strip_suffix("```").unwrap_or(&s).to_string();
        }
        s = s.trim().to_string();

        // If the LLM wrote conversational text before the JSON, extract just the JSON
        if let Some(start) = s.find('{') {
            if let Some(end) = s.rfind('}') {
                if end > start {
                    s = s[start..=end].to_string();
                }
            }
        } else {
            // No JSON at all — Planner output pure conversation. Return empty plan.
            return r#"{"tasks": []}"#.to_string();
        }

        // Fix trailing commas before closing braces/brackets: ,} or ,]
        // This is the most common LLM JSON mistake
        // Simple approach without regex dependency: repeatedly replace ,} and ,]
        while s.contains(",}") {
            s = s.replace(",}", "}");
        }
        while s.contains(",]") {
            s = s.replace(",]", "]");
        }
        // Also handle whitespace between comma and closing: , } or , ]
        while s.contains(", }") {
            s = s.replace(", }", "}");
        }
        while s.contains(", ]") {
            s = s.replace(", ]", "]");
        }

        s.trim().to_string()
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_engine_trigger_autosave() {
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_, _, _, _| Ok("Success".to_string()));

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .with_capabilities(AgentCapabilities::default())
            .build()
            .unwrap();

        let giant_content = "A".repeat(1_025_000);
        let event = Event {
            platform: "test".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "Tester".to_string(),
            author_id: "test".into(),
            content: giant_content,
        };

        let tx = engine.event_sender.as_ref().unwrap().clone();
        
        tokio::spawn(async move {
            engine.run().await;
        });

        tx.send(event).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
    use crate::providers::MockProvider;
    use crate::models::scope::Scope;
    use tokio::sync::mpsc;
    use tokio::time::{sleep, Duration};

    pub(crate) struct DummyPlatform;

    #[async_trait::async_trait]
    impl Platform for DummyPlatform {
        fn name(&self) -> &str { "dummy" }
        async fn start(&self, _: mpsc::Sender<Event>) -> Result<(), crate::platforms::PlatformError> { Ok(()) }
        async fn send(&self, _: Response) -> Result<(), crate::platforms::PlatformError> { Ok(()) }
    }

    #[tokio::test]
    async fn test_engine_routing_with_mock_provider() {
        // Setup the mock provider
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_sys, _hist, req, _tx| {
                Ok(format!("Mock response to: {}", req.content))
            });

        // Initialize engine
        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .build()
            .expect("Build failed");

        let sender = engine.event_sender.as_ref().unwrap().clone();
        
        // Spawn engine in background
        tokio::spawn(async move {
            engine.run().await;
        });

        // Send a test event
        let test_event = Event {
            platform: "dummy".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping!".to_string(),
        };

        sender.send(test_event).await.unwrap();

        // Give it a tiny bit of time to process
        sleep(Duration::from_millis(50)).await;
        // The coverage run will pick up these lines being hit.
        // And mockall enforces our expectations automatically.
    }

    #[tokio::test]
    async fn test_engine_handles_provider_error() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};
        
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_, _, _, _| Err(crate::providers::ProviderError::ConnectionError("Boom".to_string())));

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .build()
            .expect("Build failed");

        let sender = engine.event_sender.as_ref().unwrap().clone();
        
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "dummy".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping!".to_string(),
        }).await.unwrap();

        sleep(Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn test_engine_platform_start_and_send_failure() {
        use crate::providers::MockProvider;
        use tokio::time::{sleep, Duration};
        
        pub(crate) struct FailingPlatform;
        #[async_trait::async_trait]
        impl Platform for FailingPlatform {
            fn name(&self) -> &str { "failing" }
            async fn start(&self, _: mpsc::Sender<Event>) -> Result<(), crate::platforms::PlatformError> { 
                Err(crate::platforms::PlatformError::Other("start fail".into()))
            }
            async fn send(&self, _: Response) -> Result<(), crate::platforms::PlatformError> { 
                Err(crate::platforms::PlatformError::Other("send fail".into()))
            }
        }

        let mut mock_provider = MockProvider::new();
        mock_provider.expect_generate().returning(|_, _, _, _| Ok("reply".to_string()));

        let engine = EngineBuilder::new()
            .with_platform(Box::new(FailingPlatform))
            .with_provider(Arc::new(mock_provider))
            .build().unwrap();

        let sender = engine.event_sender.as_ref().unwrap().clone();
        tokio::spawn(async move {
            engine.run().await; // hits start error covering line 68
        });

        sender.send(Event {
            platform: "failing".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "Test".to_string(),
            author_id: "test".into(),
            content: "Ping".to_string(),
        }).await.unwrap();
        sleep(Duration::from_millis(50)).await; // hits send error covering line 111
    }

    #[tokio::test]
    async fn test_engine_unknown_platform() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};
        
        let mut mock_provider = MockProvider::new();
        mock_provider.expect_generate().returning(|_, _, _, _| Ok("reply".to_string()));

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .build().unwrap();

        let sender = engine.event_sender.as_ref().unwrap().clone();
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "nonexistent".to_string(), // hit line 114
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "Test".to_string(),
            author_id: "test".into(),
            content: "Ping".to_string(),
        }).await.unwrap();
        sleep(Duration::from_millis(50)).await;
    }

    mockall::mock! {
        pub TelemetryPlatform {}
        #[async_trait::async_trait]
        impl Platform for TelemetryPlatform {
            fn name(&self) -> &str;
            async fn start(&self, sender: mpsc::Sender<Event>) -> Result<(), crate::platforms::PlatformError>;
            async fn send(&self, response: Response) -> Result<(), crate::platforms::PlatformError>;
        }
    }

    #[tokio::test]
    async fn test_engine_telemetry_streaming() {
        use crate::providers::MockProvider;
        use tokio::time::{sleep, Duration};
        
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_sys, _hist, _req, tx_opt| {
                if let Some(tx) = tx_opt {
                    let tx_clone = tx.clone();
                    tokio::spawn(async move {
                        let _ = tx_clone.send("think ".to_string()).await;
                        let _ = tx_clone.send("hard".to_string()).await;
                    });
                }
                Ok("Final".to_string())
            });

        let mut mock_platform = MockTelemetryPlatform::new();
        mock_platform.expect_name().return_const("telemetry_plat".to_string());
        mock_platform.expect_start().returning(|_| Ok(()));
        // Complete telemetry (1) + final response (1) = at least 2
        mock_platform.expect_send().times(2..).returning(|_| Ok(()));

        let engine = EngineBuilder::new()
            .with_platform(Box::new(mock_platform))
            .with_provider(Arc::new(mock_provider))
            .build().unwrap();

        let sender = engine.event_sender.as_ref().unwrap().clone();
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "telemetry_plat:123".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping".to_string(),
        }).await.unwrap();

        // Wait for debounce (800ms) + processing
        sleep(Duration::from_millis(2000)).await;
    }

    #[tokio::test]
    async fn test_engine_telemetry_debounce_fires() {
        // Test that the debounce timeout actually flushes thinking text
        use crate::providers::MockProvider;
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::time::{sleep, Duration};
        
        // Use a flag to track if a telemetry send was received
        let got_thinking = Arc::new(AtomicBool::new(false));
        let got_thinking_clone = got_thinking.clone();

        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_sys, _hist, _req, tx_opt| {
                // Send a token, then keep the channel open long enough for debounce to fire
                if let Some(tx) = tx_opt {
                    let tx_clone = tx.clone();
                    tokio::spawn(async move {
                        let _ = tx_clone.send("reasoning token".to_string()).await;
                        // Hold the channel open past the 800ms debounce
                        sleep(Duration::from_millis(1500)).await;
                        // Channel drops here, triggering the "Complete" path
                    });
                }
                // Provider returns after the spawned task completes
                Ok("Answer".to_string())
            });

        let mut mock_platform = MockTelemetryPlatform::new();
        mock_platform.expect_name().return_const("telemetry_plat".to_string());
        mock_platform.expect_start().returning(|_| Ok(()));
        mock_platform.expect_send().times(1..).returning(move |r| {
            if r.is_telemetry && r.text.contains("Thinking") {
                got_thinking_clone.store(true, Ordering::SeqCst);
            }
            Ok(())
        });

        let engine = EngineBuilder::new()
            .with_platform(Box::new(mock_platform))
            .with_provider(Arc::new(mock_provider))
            .build().unwrap();

        let sender = engine.event_sender.as_ref().unwrap().clone();
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "telemetry_plat:456".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Trigger debounce".to_string(),
        }).await.unwrap();

        // Wait past debounce (800ms) + processing time
        sleep(Duration::from_millis(2500)).await;
        assert!(got_thinking.load(Ordering::SeqCst), "Debounce should have flushed a thinking update");
    }

    #[test]
    fn test_format_elapsed_seconds() {
        assert_eq!(format_elapsed(0), "0s");
        assert_eq!(format_elapsed(5), "5s");
        assert_eq!(format_elapsed(59), "59s");
    }

    #[test]
    fn test_format_elapsed_minutes() {
        assert_eq!(format_elapsed(60), "1.0m");
        assert_eq!(format_elapsed(90), "1.5m");
        assert_eq!(format_elapsed(120), "2.0m");
    }

    #[test]
    fn test_repair_planner_json_clean() {
        let input = r#"{"tasks": [{"task_id": "step_1", "drone_type": "researcher", "description": "test", "depends_on": []}]}"#;
        let result = Engine::repair_planner_json(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_repair_planner_json_markdown_fences() {
        let input = "```json\n{\"tasks\": []}\n```";
        let result = Engine::repair_planner_json(input);
        assert_eq!(result, "{\"tasks\": []}");
    }

    #[test]
    fn test_repair_planner_json_trailing_commas() {
        let input = r#"{"tasks": [{"task_id": "s1", "drone_type": "r", "description": "d", "depends_on": [],},]}"#;
        let result = Engine::repair_planner_json(input);
        // Should be valid JSON after repair
        assert!(serde_json::from_str::<crate::swarm::planner::SwarmPlan>(&result).is_ok());
    }

    #[test]
    fn test_repair_planner_json_conversational_preamble() {
        let input = "Sure! Here is the plan:\n\n{\"tasks\": []}";
        let result = Engine::repair_planner_json(input);
        assert_eq!(result, "{\"tasks\": []}");
    }

    #[test]
    fn test_repair_planner_json_bom() {
        let input = "\u{feff}{\"tasks\": []}";
        let result = Engine::repair_planner_json(input);
        assert_eq!(result, "{\"tasks\": []}");
    }

    #[tokio::test]
    async fn test_engine_observer_retry_loop() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::time::{sleep, Duration};

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_ptr = call_count.clone();

        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(move |_, _, event, _| {
                if event.author_name == "Audit" {
                    let count = call_count_ptr.fetch_add(1, Ordering::SeqCst);
                    if count == 0 {
                        Ok(r#"{"verdict": "BLOCKED", "reason": "Testing", "guidance": "Fix it"}"#.to_string())
                    } else {
                        Ok(r#"{"verdict": "ALLOWED", "reason": "Safe", "guidance": "None"}"#.to_string())
                    }
                } else {
                    Ok("Candidate".to_string())
                }
            });

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .build().unwrap();

        let sender = engine.event_sender.as_ref().unwrap().clone();
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "dummy".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping".to_string(),
        }).await.unwrap();

        sleep(Duration::from_millis(150)).await;
        // Verify observer ran exactly twice (blocked once, allowed once)
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_engine_swarm_execution() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};
        
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|sys, _, _, _| {
                if sys.contains("Swarm Queen Planner") {
                    // 1. Planner pass: Return a valid SwarmPlan JSON
                    Ok(r#"{
                      "tasks": [
                        {
                          "task_id": "test_drone_task",
                          "drone_type": "researcher",
                          "description": "Find info",
                          "depends_on": []
                        }
                      ]
                    }"#.to_string())
                } else if sys.contains("Researcher Drone") {
                    // 2. Drone execution pass
                    Ok("Drone internal thought process complete".to_string())
                } else {
                    // 3. Final Assembler pass
                    Ok("Final output from Queen based on drone output".to_string())
                }
            });

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .build()
            .expect("Build failed");

        let sender = engine.event_sender.as_ref().unwrap().clone();
        
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "dummy".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping Swarm!".to_string(),
        }).await.unwrap();

        sleep(Duration::from_millis(150)).await;
    }

    #[tokio::test]
    async fn test_engine_swarm_invalid_json() {
        // This test ensures the `Err` and fallback parsing branches are hit
        // when the planner outputs garbled JSON or the Provider outright fails during planning.
        use crate::providers::{MockProvider, ProviderError};
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};

        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|sys, _, _, _| {
                if sys.contains("Swarm Queen Planner") {
                    // Provider fails entirely during the planning phase
                    Err(ProviderError::ConnectionError("Planner offline".into()))
                } else {
                    // It should fallback to empty plan and proceed to assembler
                    Ok("Final generic response".to_string())
                }
            });

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .build()
            .unwrap();

        let sender = engine.event_sender.as_ref().unwrap().clone();
        
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "dummy".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping err".to_string(),
        }).await.unwrap();

        sleep(Duration::from_millis(150)).await;
    }

    #[tokio::test]
    async fn test_engine_clean_admin() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use crate::models::capabilities::AgentCapabilities;
        use tokio::time::{sleep, Duration};

        let mock_provider = MockProvider::new();
        
        let mut caps = AgentCapabilities::default();
        caps.admin_users.push("admin_test".into());

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .build()
            .unwrap();
            
        // Because fields are mostly public or immutable, we build a fresh engine and override caps
        let mut engine = engine;
        engine.capabilities = Arc::new(caps);

        let pub_scope = Scope::Public { channel_id: "test".into(), user_id: "test".into() };
        engine.memory.add_event(Event {
            platform: "dummy".to_string(),
            scope: pub_scope.clone(),
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping".to_string(),
        }).await;
        
        assert_eq!(engine.memory.get_working_history(&pub_scope).await.len(), 1);

        let sender = engine.event_sender.as_ref().unwrap().clone();
        
        let mem_ref = engine.memory.clone();
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "dummy".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "AdminUser".to_string(),
            author_id: "admin_test".into(),
            content: "/clean".to_string(),
        }).await.unwrap();

        sleep(Duration::from_millis(300)).await;
        
        assert_eq!(mem_ref.get_working_history(&pub_scope).await.len(), 0);
    }

    #[tokio::test]
    async fn test_engine_clean_non_admin() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use crate::models::capabilities::AgentCapabilities;
        use tokio::time::{sleep, Duration};

        let mock_provider = MockProvider::new();
        
        let test_dir = std::env::temp_dir().join(format!("hive_engine_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut caps = AgentCapabilities::default();
        caps.admin_users.push("admin_test".into());

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .with_memory(crate::memory::MemoryStore::new(Some(test_dir)))
            .build()
            .unwrap();

        let mut engine = engine;
        engine.capabilities = Arc::new(caps);

        
        let pub_scope = Scope::Public { channel_id: "test".into(), user_id: "test".into() };
        engine.memory.add_event(Event {
            platform: "dummy".to_string(),
            scope: pub_scope.clone(),
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping".to_string(),
        }).await;
        
        assert_eq!(engine.memory.get_working_history(&pub_scope).await.len(), 1);

        let sender = engine.event_sender.as_ref().unwrap().clone();
        
        let mem_ref = engine.memory.clone();
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "discord_interaction:999".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "random_123".into() },
            author_name: "RandomUser".to_string(),
            author_id: "random_123".into(),
            content: "/clean".to_string(),
        }).await.unwrap();

        sleep(Duration::from_millis(300)).await;
        
        let pub_scope = Scope::Public { channel_id: "test".into(), user_id: "test".into() };
        assert_eq!(mem_ref.get_working_history(&pub_scope).await.len(), 1);
    }
}
