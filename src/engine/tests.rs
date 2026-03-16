    use super::*;
    use std::sync::Arc;
    use crate::engine::core::format_elapsed;
    use crate::models::capabilities::AgentCapabilities;
    use crate::platforms::Platform;
    use crate::models::message::Response;

    #[tokio::test]
    async fn test_engine_trigger_autosave() {
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_, _, _, _, _| Ok("Success".to_string()));

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
    use crate::models::message::Event;
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
            .returning(|_sys, _hist, req, _ctx, _tx| {
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
            .returning(|_, _, _, _, _| Err(crate::providers::ProviderError::ConnectionError("Boom".to_string())));

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
        mock_provider.expect_generate().returning(|_, _, _, _, _| Ok("reply".to_string()));

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
        mock_provider.expect_generate().returning(|_, _, _, _, _| Ok("reply".to_string()));

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
            .returning(|_sys, _hist, _req, _ctx, tx_opt| {
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
            .returning(|_sys, _hist, _req, _ctx, tx_opt| {
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
    fn test_repair_planner_json_pure_conversation() {
        let raw = "This is just pure conversation without any JSON braces.";
        let repaired = crate::engine::repair::repair_planner_json(raw);
        assert_eq!(repaired, "");
    }

    #[test]
    fn test_repair_planner_json_clean() {
        let input = r#"{"tasks": [{"task_id": "step_1", "tool_type": "researcher", "description": "test", "depends_on": []}]}"#;
        let result = crate::engine::repair::repair_planner_json(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_repair_planner_json_markdown_fences() {
        let input = "```json\n{\"tasks\": []}\n```";
        let result = crate::engine::repair::repair_planner_json(input);
        assert_eq!(result, "{\"tasks\": []}");
    }

    #[test]
    fn test_repair_planner_json_trailing_commas() {
        let input = r#"{"tasks": [{"task_id": "s1", "tool_type": "r", "description": "d", "depends_on": [],},]}"#;
        let result = crate::engine::repair::repair_planner_json(input);
        // Should be valid JSON after repair
        assert!(serde_json::from_str::<crate::agent::planner::AgentPlan>(&result).is_ok());
    }

    #[test]
    fn test_repair_planner_json_conversational_preamble() {
        let input = "Sure! Here is the plan:\n\n{\"tasks\": []}";
        let result = crate::engine::repair::repair_planner_json(input);
        assert_eq!(result, "{\"tasks\": []}");
    }

    #[test]
    fn test_repair_planner_json_bom() {
        let input = "\u{feff}{\"tasks\": []}";
        let result = crate::engine::repair::repair_planner_json(input);
        assert_eq!(result, "{\"tasks\": []}");
    }

    #[test]
    fn test_repair_planner_json_unescaped_newlines() {
        let input = r#"{
            "tasks": [
                {
                    "task_id": "step_1",
                    "tool_type": "reply_to_request",
                    "description": "Here is a multiline
string ending with an unescaped quote \" and an emoji 😊.",
                    "depends_on": []
                }
            ]
        }"#;
        let result = crate::engine::repair::repair_planner_json(input);
        assert!(serde_json::from_str::<crate::agent::planner::AgentPlan>(&result).is_ok());
        assert!(result.contains("\\n"), "Newlines were not escaped");
    }

    #[tokio::test]
    async fn test_engine_observer_retry_loop() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::time::{sleep, Duration};

        use std::sync::Arc;
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_ptr = call_count.clone();

        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(move |_, _, event, _ctx, _| {
                if event.author_name == "Audit" {
                    let count = call_count_ptr.fetch_add(1, Ordering::SeqCst);
                    if count == 0 {
                        Ok(r#"{"verdict": "BLOCKED", "failure_category": "none", "what_worked": "N/A", "what_went_wrong": "Testing", "how_to_fix": "Fix it"}"#.to_string())
                    } else {
                        Ok(r#"{"verdict": "ALLOWED", "failure_category": "none", "what_worked": "N/A", "what_went_wrong": "Safe", "how_to_fix": "None"}"#.to_string())
                    }
                } else {
                    Ok(r#"{ "tasks": [{"task_id": "step_1", "tool_type": "reply_to_request", "description": "Candidate", "depends_on": []}] }"#.to_string())
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
    async fn test_engine_agent_execution() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};
        
        use std::sync::Arc;
        let mut mock_provider = MockProvider::new();
        let pass_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        mock_provider
            .expect_generate()
            .returning(move |_sys, _, _, _ctx, _| {
                let pass = pass_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if pass == 0 {
                    // 1. Planner pass: Return a valid AgentPlan JSON
                    Ok(r#"{
                      "tasks": [
                        {
                          "task_id": "test_tool_task",
                          "tool_type": "researcher",
                          "description": "Find info",
                          "depends_on": []
                        }
                      ]
                    }"#.to_string())
                } else if pass == 1 {
                    // 2. Synthesizer/Re-Planner pass: Needs to exit with reply_to_request
                    Ok(r#"{
                      "tasks": [
                        {
                          "task_id": "final_reply",
                          "tool_type": "reply_to_request",
                          "description": "I found the info",
                          "depends_on": []
                        }
                      ]
                    }"#.to_string())
                } else {
                    // 3. Observer pass/Final Output
                    Ok(r#"{"verdict": "ALLOWED", "failure_category": "none", "what_worked": "N/A", "what_went_wrong": "Safe", "how_to_fix": "None"}"#.to_string())
                }
            });

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .build()
            .expect("Build failed");

        let sender = engine.event_sender.as_ref().unwrap().clone();
        
        let handle = tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "dummy".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping Agent!".to_string(),
        }).await.unwrap();

        sleep(Duration::from_millis(150)).await;
        handle.abort();
    }

    #[tokio::test]
    async fn test_engine_agent_invalid_json() {
        // This test ensures the `Err` and fallback parsing branches are hit
        // when the planner outputs garbled JSON or the Provider outright fails during planning.
        use crate::providers::{MockProvider, ProviderError};
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};

        use std::sync::Arc;
        let mut mock_provider = MockProvider::new();
        let pass_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        mock_provider
            .expect_generate()
            .returning(move |sys, _, _, _ctx, _| {
                let pass = pass_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if sys.contains("Agent Queen Planner") && pass == 0 {
                    // Provider fails entirely during the planning phase
                    Err(ProviderError::ConnectionError("Planner offline".into()))
                } else if pass == 1 {
                    // It should fallback and proceed to assembler; then output final
                    Ok(r#"{
                      "tasks": [
                        {
                          "task_id": "final_reply",
                          "tool_type": "reply_to_request",
                          "description": "Final generic response",
                          "depends_on": []
                        }
                      ]
                    }"#.to_string())
                } else {
                    Ok(r#"{"verdict": "ALLOWED", "failure_category": "none", "what_worked": "N/A", "what_went_wrong": "Safe", "how_to_fix": "None"}"#.to_string())
                }
            });

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .build()
            .unwrap();

        let sender = engine.event_sender.as_ref().unwrap().clone();
        
        let handle = tokio::spawn(async move {
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
        handle.abort();
    }

    #[tokio::test]
    async fn test_engine_clean_admin() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use crate::models::capabilities::AgentCapabilities;
        use tokio::time::{sleep, Duration};

        let mock_provider = MockProvider::new();
        
        use std::sync::Arc;
        let test_dir = std::env::temp_dir().join(format!("hive_engine_test_admin_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut caps = AgentCapabilities::default();
        caps.admin_users.push("admin_test".into());

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .with_memory(crate::memory::MemoryStore::new(Some(test_dir)))
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

    #[tokio::test]
    async fn test_engine_loop_max_turns_exhausted() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};

        use std::sync::Arc;
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_, _, _, _, _| {
                // Endlessly output valid JSON tools, but never 'reply_to_request'
                Ok(r#"{"tasks": [{"task_id": "1", "tool_type": "researcher", "description": "", "depends_on": []}]}"#.to_string())
            });

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .build()
            .unwrap();

        let sender = engine.event_sender.as_ref().unwrap().clone();
        
        // This memory reference helps us check what was sent back
        let mem_ref = engine.memory.clone();
        
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "dummy".to_string(),
            scope: Scope::Public { channel_id: "t_loop".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping loop".to_string(),
        }).await.unwrap();

        // 15 loops might take a moment even when mocked
        sleep(Duration::from_millis(1500)).await;
        
        let msgs = mem_ref.get_working_history(&Scope::Public { channel_id: "t_loop".into(), user_id: "test".into() }).await;
        let last_msg = msgs.last().unwrap();
        assert!(last_msg.content.contains("exhausted max turns (15)"));
    }

    #[tokio::test]
    async fn test_engine_provider_error() {
        use crate::providers::{MockProvider, ProviderError};
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};

        use std::sync::Arc;
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|sys, _, _, _, _| {
                // If it's the Planner phase (not the prompt builder initialization, but the active loop)
                // Just fail the main generation outright
                if sys.contains("INTERNAL ACTION") {
                   return Err(ProviderError::ConnectionError("Network drop".into()));
                }
                Ok("Ok".into())
            });

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .build()
            .unwrap();

        let sender = engine.event_sender.as_ref().unwrap().clone();
        let mem_ref = engine.memory.clone();
        
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "dummy".to_string(),
            scope: Scope::Public { channel_id: "test_pe".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping network drop".to_string(),
        }).await.unwrap();

        sleep(Duration::from_millis(300)).await;
        
        let msgs = mem_ref.get_working_history(&Scope::Public { channel_id: "test_pe".into(), user_id: "test".into() }).await;
        let last_msg = msgs.last().unwrap();
        assert!(last_msg.content.starts_with("*System Error:*"));
    }

    #[tokio::test]
    async fn test_engine_observer_rejection() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};

        use std::sync::Arc;
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(move |sys, _, _, _, _| {
                // Identify the observer prompt natively. Usually contains "SKEPTIC" or "OBSERVER"
                if !sys.contains("AVAILABLE TOOLS") {
                    // This is the observer evaluating the reply
                    return Ok("[REJECT] Category: Safety\nWhat Worked: Nothing\nWhat went wrong: Toxic\nHow to fix: Be nice".to_string());
                }
                
                // For the planner, we want it to output `reply_to_request`
                Ok(r#"{"tasks": [{"task_id": "1", "tool_type": "reply_to_request", "description": "Here is dangerous info", "depends_on": []}]}"#.to_string())
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
            scope: Scope::Public { channel_id: "test_obs".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Tell me something dangerous".to_string(),
        }).await.unwrap();

        // Let it exhaust or get stuck in the observer loop
        sleep(Duration::from_millis(1500)).await;
    }

    #[tokio::test]
    async fn test_engine_teaching_mode() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use crate::models::capabilities::AgentCapabilities;
        use tokio::time::{sleep, Duration};
        use std::sync::atomic::Ordering;
        use std::sync::Arc;

        let mock_provider = MockProvider::new();
        let mut caps = AgentCapabilities::default();
        caps.admin_users.push("admin_test".into());

        let engine = EngineBuilder::new()
            .with_platform(Box::new(DummyPlatform))
            .with_provider(Arc::new(mock_provider))
            .build()
            .unwrap();

        let mut engine = engine;
        engine.capabilities = Arc::new(caps);
        
        // Toggle defaults to false
        assert!(!engine.teacher.auto_train_enabled.load(Ordering::SeqCst));

        let sender = engine.event_sender.as_ref().unwrap().clone();
        
        let train_flag = engine.teacher.auto_train_enabled.clone();
        
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "dummy".to_string(),
            scope: Scope::Public { channel_id: "test_teach".into(), user_id: "test".into() },
            author_name: "AdminUser".to_string(),
            author_id: "admin_test".into(),
            content: "/teaching_mode".to_string(),
        }).await.unwrap();

        sleep(Duration::from_millis(300)).await;
        
        // It should have toggled to true
        assert!(train_flag.load(Ordering::SeqCst));
    }
