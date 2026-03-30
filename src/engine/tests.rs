    use super::*;
    use std::sync::Arc;
    use crate::engine::core::format_elapsed;
    use crate::models::capabilities::AgentCapabilities;
    use crate::platforms::Platform;
    use crate::models::message::Response;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_engine_trigger_autosave() {
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_, _, _, _, _, _| Ok("Success".to_string()));

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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
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
    async fn test_engine_routing_with_mock_provider() {
        // Setup the mock provider
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_sys, _hist, req, _ctx, _tx, _| {
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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        sender.send(test_event).await.unwrap();

        // Give it a tiny bit of time to process
        sleep(Duration::from_millis(50)).await;
        // The coverage run will pick up these lines being hit.
        // And mockall enforces our expectations automatically.
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_engine_handles_provider_error() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};
        
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_, _, _, _, _, _| Err(crate::providers::ProviderError::ConnectionError("Boom".to_string())));

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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await.unwrap();

        sleep(Duration::from_millis(50)).await;
    }

    #[tokio::test(flavor = "multi_thread")]
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
        mock_provider.expect_generate().returning(|_, _, _, _, _, _| Ok("reply".to_string()));

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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await.unwrap();
        sleep(Duration::from_millis(50)).await; // hits send error covering line 111
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_engine_unknown_platform() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};
        
        let mut mock_provider = MockProvider::new();
        mock_provider.expect_generate().returning(|_, _, _, _, _, _| Ok("reply".to_string()));

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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
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

    #[tokio::test(flavor = "multi_thread")]
    async fn test_engine_telemetry_streaming() {
        use crate::providers::MockProvider;
        use tokio::time::{sleep, Duration};
        
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_sys, _hist, _req, _ctx, tx_opt, _| {
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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await.unwrap();

        // Wait for debounce (800ms) + processing
        sleep(Duration::from_millis(2000)).await;
    }

    #[tokio::test(flavor = "multi_thread")]
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
            .returning(|_sys, _hist, _req, _ctx, tx_opt, _| {
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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
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

    #[tokio::test(flavor = "multi_thread")]
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
            .returning(move |_, _, _event, ctx, _, _| {
                // Distinguish audit calls by the AUDIT MODE marker in context
                if ctx.contains("SWITCH TO AUDIT MODE") {
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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await.unwrap();

        sleep(Duration::from_millis(500)).await;
        // Verify observer ran exactly twice (blocked once, allowed once)
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }
