    use super::*;
    use crate::providers::MockProvider;
    use crate::models::capabilities::AgentCapabilities;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_sub_agent_spec_defaults() {
        let spec = SubAgentSpec::default();
        assert_eq!(spec.max_turns, 8);
        assert_eq!(spec.timeout_secs, 300);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_spawn_strategy_from_str() {
        assert_eq!(SpawnStrategy::from_str("parallel"), SpawnStrategy::Parallel);
        assert_eq!(SpawnStrategy::from_str("pipeline"), SpawnStrategy::Pipeline);
        assert_eq!(SpawnStrategy::from_str("competitive"), SpawnStrategy::Competitive);
        assert_eq!(SpawnStrategy::from_str("fan_out_fan_in"), SpawnStrategy::FanOutFanIn);
        assert_eq!(SpawnStrategy::from_str("unknown"), SpawnStrategy::Parallel);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_sub_agent_completes_on_reply() {
        let mut mock = MockProvider::new();
        mock.expect_generate()
            .returning(|_, _, _, _, _, _| {
                Ok(r#"{"thought":["A","B","C","D"],"tasks":[{"task_id":"reply","tool_type":"reply_to_request","description":"Here is my finding: test result","depends_on":[]}]}"#.to_string())
            });

        let provider: Arc<dyn Provider> = Arc::new(mock);
        let memory = Arc::new(MemoryStore::default());
        let (tx, mut _rx) = mpsc::channel(100);

        let agent_mgr = Arc::new(crate::agent::AgentManager::new(provider.clone(), memory.clone(), Arc::new(crate::config::AppConfig::default())));
        let capabilities = Arc::new(AgentCapabilities::default());

        let spec = SubAgentSpec {
            task: "Find the answer to life".into(),
            max_turns: 5,
            timeout_secs: 30,
            scope: Scope::Private { user_id: "test".into() },
            user_id: "test".into(),
            spatial_offset: None,
            swarm_depth: 0,
        };

        let result = execute_sub_agent(
            "test-agent-1".into(),
            spec,
            provider,
            memory,
            tx,
            agent_mgr,
            capabilities,
            None,
        ).await;

        assert_eq!(result.status, SubAgentStatus::Completed);
        assert!(result.output.contains("test result"));
        assert_eq!(result.turns_used, 1);
    }

    #[tokio::test]
    async fn test_sub_agent_max_turns_exceeded() {
        let mut mock = MockProvider::new();
        // Always return a tool call, never reply_to_request
        mock.expect_generate()
            .returning(|_, _, _, _, _, _| {
                Ok(r#"{"thought":["A","B","C","D"],"tasks":[{"task_id":"s1","tool_type":"web_search","description":"test query","depends_on":[]}]}"#.to_string())
            });

        let provider: Arc<dyn Provider> = Arc::new(mock);
        let memory = Arc::new(MemoryStore::default());
        let (tx, mut _rx) = mpsc::channel(100);
        let agent_mgr = Arc::new(crate::agent::AgentManager::new(provider.clone(), memory.clone(), Arc::new(crate::config::AppConfig::default())));
        let capabilities = Arc::new(AgentCapabilities::default());

        let spec = SubAgentSpec {
            task: "Infinite task".into(),
            max_turns: 2,
            timeout_secs: 30,
            scope: Scope::Private { user_id: "test".into() },
            user_id: "test".into(),
            spatial_offset: None,
            swarm_depth: 0,
        };

        let result = execute_sub_agent(
            "test-agent-2".into(),
            spec,
            provider,
            memory,
            tx,
            agent_mgr,
            capabilities,
            None,
        ).await;

        assert_eq!(result.status, SubAgentStatus::Failed("Max turns exceeded".into()));
        assert_eq!(result.turns_used, 2);
    }

    #[tokio::test]
    async fn test_sub_agent_pipeline_context() {
        let mut mock = MockProvider::new();
        mock.expect_generate()
            .returning(|_, _, _, ctx, _, _| {
                // Verify pipeline context is injected
                assert!(ctx.contains("PREVIOUS AGENT"));
                Ok(r#"{"thought":["A","B","C","D"],"tasks":[{"task_id":"reply","tool_type":"reply_to_request","description":"Processed pipeline data","depends_on":[]}]}"#.to_string())
            });

        let provider: Arc<dyn Provider> = Arc::new(mock);
        let memory = Arc::new(MemoryStore::default());
        let (tx, mut _rx) = mpsc::channel(100);
        let agent_mgr = Arc::new(crate::agent::AgentManager::new(provider.clone(), memory.clone(), Arc::new(crate::config::AppConfig::default())));
        let capabilities = Arc::new(AgentCapabilities::default());

        let spec = SubAgentSpec {
            task: "Continue analysis".into(),
            max_turns: 5,
            timeout_secs: 30,
            scope: Scope::Private { user_id: "test".into() },
            user_id: "test".into(),
            spatial_offset: None,
            swarm_depth: 0,
        };

        let result = execute_sub_agent(
            "pipe-agent".into(),
            spec,
            provider,
            memory,
            tx,
            agent_mgr,
            capabilities,
            Some("Previous agent found: key data point X".into()),
        ).await;

        assert_eq!(result.status, SubAgentStatus::Completed);
    }
