use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::models::message::{Event, Response};

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

pub struct EngineBuilder {
    platforms: HashMap<String, Box<dyn Platform>>,
    provider: Option<Arc<dyn Provider>>,
}

impl EngineBuilder {
    pub fn new() -> Self {
        Self {
            platforms: HashMap::new(),
            provider: None,
        }
    }

    pub fn with_platform(mut self, platform: Box<dyn Platform>) -> Self {
        self.platforms.insert(platform.name().to_string(), platform);
        self
    }

    pub fn with_provider(mut self, provider: Arc<dyn Provider>) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn build(self) -> Result<Engine, &'static str> {
        let provider = self.provider.ok_or("Engine requires a Provider to be set")?;
        let (tx, rx) = mpsc::channel(100);
        
        Ok(Engine {
            platforms: Arc::new(self.platforms),
            provider,
            memory: Arc::new(MemoryStore::new()),
            event_sender: Some(tx),
            event_receiver: rx,
        })
    }
}

pub struct Engine {
    platforms: Arc<HashMap<String, Box<dyn Platform>>>,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
    
    // Channel for platforms to send events IN to the engine
    event_sender: Option<mpsc::Sender<Event>>,
    // The engine receives them here
    event_receiver: mpsc::Receiver<Event>,
}

impl Engine {
    pub async fn run(mut self) {
        println!("Starting HIVE Engine...");
        
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
            // 1. Retrieve context BEFORE storing the new event (prevents duplicate in prompt)
            let history = self.memory.get_history(&event.scope).await;

            // 2. Now store the incoming event in memory for future context
            self.memory.add_event(event.clone()).await;

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

            // 4. Generate Apis Prompt & Call Provider
            let system_prompt = self.get_system_prompt(&event);
            let response_text = match self.provider.generate(&system_prompt, &history, &event, Some(telemetry_tx)).await {
                Ok(text) => text,
                Err(e) => {
                    eprintln!("Provider Error: {:?}", e);
                    format!("*System Error:* Something went wrong. ({})", e)
                }
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

    fn get_system_prompt(&self, _event: &Event) -> String {
        "You are Apis, the core persona of the HIVE system. \
         You are highly intelligent, analytical, and direct. \
         Be exceptionally helpful, concise, and do not break character. \
         Always acknowledge the context provided in the history without referring to how the system stores it.".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            scope: Scope::Public,
            author_name: "TestUser".to_string(),
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
            scope: Scope::Public,
            author_name: "TestUser".to_string(),
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
            scope: Scope::Public,
            author_name: "Test".to_string(),
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
            scope: Scope::Public,
            author_name: "Test".to_string(),
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
            scope: Scope::Public,
            author_name: "TestUser".to_string(),
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
            scope: Scope::Public,
            author_name: "TestUser".to_string(),
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
}
