    use super::*;
    use crate::agent::planner::AgentTask;

    #[tokio::test]
    async fn test_dispatch_all_branches() {
        let mem = Arc::new(MemoryStore::default());
        let scope = Scope::Public { channel_id: "t".into(), user_id: "t".into() };
        
        use crate::providers::MockProvider;
        let mut mock_provider = MockProvider::new();
        mock_provider.expect_generate().returning(|_, _, _, _, _, _| Ok("Mock".to_string()));
        let provider: Arc<dyn Provider> = Arc::new(mock_provider);
        
        let tools = vec![
            "channel_reader", "outreach", "codebase_list", "codebase_read",
            "web_search", "researcher", "generate_image", "voice_synthesizer",
            "take_snapshot", "send_email", "set_alarm", "smart_home", "system_recompile", "project_contributors",
            "operate_turing_grid", "file_writer", "read_logs", "run_bash_command",
            "process_manager", "file_system_operator", "autonomy_activity",
            "review_reasoning", "read_attachment", "manage_user_preferences",
            "manage_skill", "manage_routine", "manage_lessons", "manage_goals", "tool_forge", "search_timeline",
            "manage_scratchpad", "operate_synaptic_graph", "read_core_memory",
            "download", "list_cached_images",
            // Swarm delegation tools
            "delegate", "research_swarm",
            // Self-moderation tools
            "refuse_request", "disengage", "mute_user", "set_boundary", "block_topic",
            "escalate_to_admin", "report_concern", "rate_limit_user", "request_consent", "wellbeing_status",
        ];
        
        for t in tools {
            let task = AgentTask {
                task_id: "1".into(),
                tool_type: t.into(),
                description: "mock action:[read]".into(),
                depends_on: vec![],
            source: None,
            };
            
            let handle = dispatch_native_tool(
                &task,
                "context",
                &scope,
                None,
                mem.clone(),
                provider.clone(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            );
            
            assert!(handle.is_some(), "Tool {} should return a handle", t);
        }
        
        // Blocked duplicate image logic
        let img_task = AgentTask {
            task_id: "2".into(),
            tool_type: "generate_image".into(),
            description: "mock".into(),
            depends_on: vec![],
            source: None,
        };
        let dup_handle = dispatch_native_tool(
            &img_task,
            "[ATTACH_IMAGE] Context from before",
            &scope,
            None,
            mem.clone(),
            provider.clone(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert!(dup_handle.is_some());
        
        // Unknown tool
        let missing = AgentTask {
            task_id: "3".into(),
            tool_type: "fake_drone_99".into(),
            description: "mock".into(),
            depends_on: vec![],
            source: None,
        };
        let none_handle = dispatch_native_tool(
            &missing,
            "context",
            &scope,
            None,
            mem.clone(),
            provider.clone(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert!(none_handle.is_none());
    }
