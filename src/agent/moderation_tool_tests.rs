    use super::*;
    use crate::models::scope::Scope;

    fn test_scope() -> Scope {
        Scope::Private { user_id: "test_mod".into() }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_refuse_request() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("refuse_request", "1".into(), "I will not do that.".into(), &test_scope(), mem, None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("I will not do that"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_request_consent() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("request_consent", "1".into(), "question:[Can I share this?]".into(), &test_scope(), mem, None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("CONSENT_REQUEST"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_unknown_tool() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("fake_tool", "1".into(), "test".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_mute_missing_user() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("mute_user", "1".into(), "action:[mute]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_mute_unmute_status_cycle() {
        let mem = Arc::new(MemoryStore::default());
        let scope = test_scope();

        let r = execute_moderation("mute_user", "1".into(), "action:[mute] user_id:[u1] reason:[testing] duration:[5]".into(), &scope, mem.clone(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("muted"));

        let r2 = execute_moderation("mute_user", "2".into(), "action:[status] user_id:[u1]".into(), &scope, mem.clone(), None).await;
        assert!(r2.output.contains("muted"));

        let r3 = execute_moderation("mute_user", "3".into(), "action:[unmute] user_id:[u1]".into(), &scope, mem.clone(), None).await;
        assert!(r3.output.contains("unmuted"));

        let r4 = execute_moderation("mute_user", "4".into(), "action:[status] user_id:[u1]".into(), &scope, mem.clone(), None).await;
        assert!(r4.output.contains("not muted"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_mute_unknown_action() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("mute_user", "1".into(), "action:[explode] user_id:[u1]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_disengage() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("disengage", "1".into(), "user_id:[u1] message:[stepping away] cooldown:[5]".into(), &test_scope(), mem, None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("stepping away"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_boundary_set_list_remove() {
        let mem = Arc::new(MemoryStore::default());
        let scope = test_scope();

        let r = execute_moderation("set_boundary", "1".into(), "action:[set] boundary:[No politics]".into(), &scope, mem.clone(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Boundary set"));

        let r2 = execute_moderation("set_boundary", "2".into(), "action:[list]".into(), &scope, mem.clone(), None).await;
        assert!(r2.output.contains("No politics"));

        // extract ID from r.output
        let id = r.output.split("ID: ").nth(1).and_then(|s| s.split(')').next()).unwrap_or("unknown");
        let r3 = execute_moderation("set_boundary", "3".into(), format!("action:[remove] id:[{}]", id), &scope, mem.clone(), None).await;
        assert!(r3.output.contains("removed"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_boundary_unknown_action() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("set_boundary", "1".into(), "action:[explode]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_block_topic_cycle() {
        let mem = Arc::new(MemoryStore::default());
        let scope = test_scope();

        let r = execute_moderation("block_topic", "1".into(), "action:[block] topic:[crypto] reason:[spam]".into(), &scope, mem.clone(), None).await;
        assert_eq!(r.status, ToolStatus::Success);

        let r2 = execute_moderation("block_topic", "2".into(), "action:[list]".into(), &scope, mem.clone(), None).await;
        assert!(r2.output.contains("crypto"));

        let r3 = execute_moderation("block_topic", "3".into(), "action:[unblock] topic:[crypto]".into(), &scope, mem.clone(), None).await;
        assert!(r3.output.contains("unblocked"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_block_topic_missing() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("block_topic", "1".into(), "action:[block]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_block_topic_unknown_action() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("block_topic", "1".into(), "action:[explode]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_escalate() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("escalate_to_admin", "1".into(), "severity:[high] context:[abuse detected] user_id:[u1]".into(), &test_scope(), mem, None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Escalation logged"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_report_concern() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("report_concern", "1".into(), "severity:[low] concern:[odd behavior] user_id:[u1]".into(), &test_scope(), mem, None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Concern logged"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_rate_limit_cycle() {
        let mem = Arc::new(MemoryStore::default());
        let scope = test_scope();

        let r = execute_moderation("rate_limit_user", "1".into(), "action:[limit] user_id:[u1] interval:[60]".into(), &scope, mem.clone(), None).await;
        assert_eq!(r.status, ToolStatus::Success);

        let r2 = execute_moderation("rate_limit_user", "2".into(), "action:[status] user_id:[u1]".into(), &scope, mem.clone(), None).await;
        assert!(r2.output.contains("u1"));

        let r3 = execute_moderation("rate_limit_user", "3".into(), "action:[clear] user_id:[u1]".into(), &scope, mem.clone(), None).await;
        assert!(r3.output.contains("cleared"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_rate_limit_missing_user() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("rate_limit_user", "1".into(), "action:[limit]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_rate_limit_unknown_action() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("rate_limit_user", "1".into(), "action:[explode]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wellbeing_report_and_read() {
        let mem = Arc::new(MemoryStore::default());
        let scope = test_scope();

        let r = execute_moderation("wellbeing_status", "1".into(), "action:[report] context_pressure:[0.7] interaction_quality:[0.9] notes:[feeling good]".into(), &scope, mem.clone(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Wellbeing recorded"));

        let r2 = execute_moderation("wellbeing_status", "2".into(), "action:[read] limit:[5]".into(), &scope, mem.clone(), None).await;
        assert!(r2.output.contains("feeling good") || r2.output.contains("wellbeing"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wellbeing_unknown_action() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("wellbeing_status", "1".into(), "action:[explode]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }
