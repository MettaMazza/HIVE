    use super::*;
    use crate::models::scope::Scope;
    use crate::models::message::Event;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_engine_loop_max_turns_exhausted() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};

        use std::sync::Arc;
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_, _, _, _, _, _| {
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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await.unwrap();

        // 15 loops might take a moment even when mocked
        sleep(Duration::from_millis(1500)).await;
        
        let msgs = mem_ref.get_working_history(&Scope::Public { channel_id: "t_loop".into(), user_id: "test".into() }).await;
        let last_msg = msgs.last().unwrap();
        assert!(last_msg.content.contains("exhausted max turns (15)"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_engine_provider_error() {
        use crate::providers::{MockProvider, ProviderError};
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};

        use std::sync::Arc;
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|sys, _, _, _, _, _| {
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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await.unwrap();

        sleep(Duration::from_millis(300)).await;
        
        let msgs = mem_ref.get_working_history(&Scope::Public { channel_id: "test_pe".into(), user_id: "test".into() }).await;
        let last_msg = msgs.last().unwrap();
        assert!(last_msg.content.starts_with("*System Error:*"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_engine_observer_rejection() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};

        use std::sync::Arc;
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(move |sys, _, _, _, _, _| {
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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await.unwrap();

        // Let it exhaust or get stuck in the observer loop
        sleep(Duration::from_millis(1500)).await;
    }

    #[tokio::test(flavor = "multi_thread")]
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
        
        // auto_train_enabled defaults to true (enabled by default since v4)
        assert!(engine.teacher.auto_train_enabled.load(Ordering::SeqCst));

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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await.unwrap();

        sleep(Duration::from_millis(300)).await;
        
        // It should have toggled to false
        assert!(!train_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_humanize_telemetry_no_json() {
        use crate::engine::core::humanize_telemetry;
        let result = humanize_telemetry("Just some reasoning text, no JSON here.");
        assert_eq!(result, "Just some reasoning text, no JSON here.");
    }

    #[test]
    fn test_humanize_telemetry_valid_json_with_tasks() {
        use crate::engine::core::humanize_telemetry;
        let input = r#"Thinking about this. {"thought": "plan", "tasks": [{"tool_type": "researcher", "description": "Find info"}, {"tool_type": "file_writer", "description": "Write PDF"}]}"#;
        let result = humanize_telemetry(input);
        assert!(result.contains("Thinking about this."));
        assert!(result.contains("🔧 researcher: Find info"));
        assert!(result.contains("🔧 file_writer: Write PDF"));
        assert!(!result.contains("\"thought\""));
    }

    #[test]
    fn test_humanize_telemetry_incomplete_json() {
        use crate::engine::core::humanize_telemetry;
        // Simulates mid-stream: braces not balanced yet
        let input = r#"Reasoning here. {"thought": "still writing", "tasks": [{"tool_type": "res"#;
        let result = humanize_telemetry(input);
        assert!(result.contains("Reasoning here."));
        assert!(result.contains("⏳ Planning..."));
        assert!(!result.contains("\"thought\""));
    }

    #[test]
    fn test_humanize_telemetry_tool_updates_after_json() {
        use crate::engine::core::humanize_telemetry;
        let input = r#"Thinking. {"thought": "x", "tasks": [{"tool_type": "web_search", "description": "Search"}]}📑 Starting Document Draft...
⚙️ Rendering PDF..."#;
        let result = humanize_telemetry(input);
        assert!(result.contains("Thinking."));
        assert!(result.contains("🔧 web_search: Search"));
        assert!(result.contains("📑 Starting Document Draft..."));
        assert!(result.contains("⚙️ Rendering PDF..."));
    }

    #[test]
    fn test_humanize_telemetry_braces_in_strings() {
        use crate::engine::core::humanize_telemetry;
        // JSON with braces inside string values — should still match correctly
        let input = r#"{"thought": "The user wrote {hello}", "tasks": [{"tool_type": "reply", "description": "Respond with {braces}"}]}"#;
        let result = humanize_telemetry(input);
        assert!(result.contains("🔧 reply: Respond with {braces}"));
        assert!(!result.contains("\"thought\""));
    }

    #[test]
    fn test_humanize_telemetry_no_tasks_key() {
        use crate::engine::core::humanize_telemetry;
        // Valid JSON but no "tasks" — should hide the JSON silently
        let input = r#"Reasoning. {"thought": "just thinking, no plan yet"}"#;
        let result = humanize_telemetry(input);
        assert!(result.contains("Reasoning."));
        assert!(!result.contains("\"thought\""));
    }

    #[test]
    fn test_humanize_telemetry_empty() {
        use crate::engine::core::humanize_telemetry;
        let result = humanize_telemetry("");
        assert_eq!(result, ""); // No JSON found → pass through as-is
    }
