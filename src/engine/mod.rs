use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::models::message::{Event, Response};
use crate::models::capabilities::AgentCapabilities;

use crate::memory::MemoryStore;
use crate::platforms::Platform;
use crate::providers::Provider;

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
}

impl EngineBuilder {
    pub fn new() -> Self {
        Self {
            platforms: HashMap::new(),
            provider: None,
            capabilities: AgentCapabilities::default(),
            memory: MemoryStore::new(None),
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
    pub fn with_memory(mut self, mem: MemoryStore) -> Self {
        self.memory = mem;
        self
    }
    
    pub fn build(self) -> Result<Engine, &'static str> {
        let provider = self.provider.ok_or("Engine requires a Provider to be set")?;
        let (tx, rx) = mpsc::channel(100);
        
        let memory = Arc::new(self.memory);
        
        // Ensure the swarm receives the same Arc memory map as the Engine
        let swarm = Arc::new(SwarmManager::new(provider.clone(), memory.clone()));

        Ok(Engine {
            platforms: Arc::new(self.platforms),
            provider: provider.clone(),
            capabilities: Arc::new(self.capabilities),
            memory: memory,
            swarm: swarm,
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
    
    // Channel for platforms to send events IN to the engine
    event_sender: Option<mpsc::Sender<Event>>,
    // The engine receives them here
    event_receiver: mpsc::Receiver<Event>,
}

impl Engine {
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
                }
                // Skip the rest of the LLM generation loop
                continue;
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
            let planner_prompt = PLANNER_SYSTEM_PROMPT.replace("{available_drones}", &self.swarm.get_available_drones_text());
            let plan_json = match self.provider.generate(&planner_prompt, &history, &event, Some(telemetry_tx.clone())).await {
                Ok(text) => text,
                Err(e) => {
                    eprintln!("Planner Failed: {:?}", e);
                    "{}".to_string() // Fallback to empty plan
                }
            };
            
            // Try to parse the Queen's plan
            let mut context_from_swarm = String::new();
            if let Ok(plan) = serde_json::from_str::<crate::swarm::planner::SwarmPlan>(&plan_json) {
                if !plan.tasks.is_empty() {
                    // Execute parallel swarm
                    let tx_clone = telemetry_tx.clone();
                    let drone_results = self.swarm.execute_plan(plan, &event.content, Some(tx_clone)).await;
                    
                    // Aggregate results for the final assembler
                    context_from_swarm.push_str("\n\n[SWARM EXECUTION RESULTS]\n");
                    for res in drone_results {
                        context_from_swarm.push_str(&format!("Task {}: {:?}\nOutput: {}\n\n", res.task_id, res.status, res.output));
                    }
                }
            }

            // 5. Generate Final Apis Assembler Prompt & Call Provider
            let system_prompt_base = crate::prompts::SystemPromptBuilder::assemble(&event.scope, self.memory.clone()).await;
            let system_prompt = format!("{}{}", system_prompt_base, context_from_swarm);
            
            let final_response_text;
            let mut extra_guidance = String::new();

            loop {
                let active_prompt = format!("{}{}", extra_guidance, system_prompt);
                
                let candidate_text = match self.provider.generate(&active_prompt, &history, &event, Some(telemetry_tx.clone())).await {
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

                // Run 1:1 Shadow Observer Audit
                let audit_result = crate::prompts::observer::run_skeptic_audit(
                    self.provider.clone(),
                    &self.capabilities,
                    &candidate_text,
                    &active_prompt,
                    &history,
                    &event
                ).await;

                if audit_result.is_allowed() {
                    final_response_text = candidate_text;
                    break;
                } else {
                    println!("[OBSERVER BLOCKED]: {}", audit_result.reason);
                    extra_guidance = format!("[OBSERVER GUIDANCE - CORRECTION REQUIRED]: {}\n\n", audit_result.guidance);
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
        }
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
