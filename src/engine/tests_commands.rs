    use super::*;
    use std::sync::Arc;
    use crate::models::scope::Scope;
    use crate::models::message::Event;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_engine_agent_execution() {
        use crate::providers::MockProvider;
        use crate::engine::tests::DummyPlatform;
        use tokio::time::{sleep, Duration};
        
        use std::sync::Arc;
        let mut mock_provider = MockProvider::new();
        let pass_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        mock_provider
            .expect_generate()
            .returning(move |_sys, _, _, _ctx, _, _| {
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
        
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "dummy".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping Agent!".to_string(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await.unwrap();

        sleep(Duration::from_millis(150)).await;
    }

    #[tokio::test(flavor = "multi_thread")]
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
            .returning(move |sys, _, _, _ctx, _, _| {
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
        
        tokio::spawn(async move {
            engine.run().await;
        });

        sender.send(Event {
            platform: "dummy".to_string(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping err".to_string(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await.unwrap();

        sleep(Duration::from_millis(150)).await;
    }

    #[tokio::test(flavor = "multi_thread")]
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
            .with_memory(Arc::new(crate::memory::MemoryStore::new(Some(test_dir))))
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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await.unwrap();

        sleep(Duration::from_millis(300)).await;
        
        assert_eq!(mem_ref.get_working_history(&pub_scope).await.len(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
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
            .with_memory(Arc::new(crate::memory::MemoryStore::new(Some(test_dir))))
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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
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
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await.unwrap();

        sleep(Duration::from_millis(300)).await;
        
        let pub_scope = Scope::Public { channel_id: "test".into(), user_id: "test".into() };
        assert_eq!(mem_ref.get_working_history(&pub_scope).await.len(), 1);
    }
