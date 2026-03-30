    use super::*;

    #[test]
    fn test_sleep_config_defaults() {
        let config = SleepConfig::default();
        assert_eq!(config.micro_batch_size, 2);
        assert_eq!(config.micro_epochs, 1);
        assert_eq!(config.auto_sleep_interval_secs, 43200);
        assert!((config.micro_lr - 1e-5).abs() < 1e-10);
    }

    #[test]
    fn test_quality_score_first_pass() {
        let good = GoldenExample {
            ts: Utc::now().to_rfc3339(),
            system_prompt: "sys".into(),
            user_msg: "user".into(),
            agent_ctx: "ctx".into(),
            response: "A".repeat(500), // Good length
            tools: vec!["web_search".into()],
            attempts: 1, // First pass
        };

        let bad = GoldenExample {
            ts: Utc::now().to_rfc3339(),
            system_prompt: "sys".into(),
            user_msg: "user".into(),
            agent_ctx: "ctx".into(),
            response: "ok".into(), // Too short
            tools: vec![],
            attempts: 3, // Multiple retries
        };

        let good_score = compute_quality_score(&good);
        let bad_score = compute_quality_score(&bad);
        assert!(good_score > bad_score, "First-pass + tools + good length should score higher");
    }

    #[test]
    fn test_sleep_report_display() {
        let report = SleepReport {
            version: "apis-v3-20260327".into(),
            parent: "apis-v2-20260326".into(),
            golden_used: 2,
            pairs_used: 0,
            identity_reinforced: true,
            duration_secs: 12.5,
            timestamp: Utc::now().to_rfc3339(),
            quality_scores: vec![4.5, 3.0],
        };
        let display = format!("{}", report);
        assert!(display.contains("apis-v3"));
        assert!(display.contains("2 examples"));
    }

    #[tokio::test]
    async fn test_sleep_cycle_no_data() {
        let tmp = std::env::temp_dir().join(format!("hive_sleep_test_{}", std::process::id()));
        let teacher = Arc::new(Teacher::new(Some(tmp.clone())));
        let cycle = SleepCycle::new(teacher, None);

        // No data = should not auto-sleep
        assert!(!cycle.should_auto_sleep().await);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[tokio::test]
    async fn test_sleep_status_display() {
        let tmp = std::env::temp_dir().join(format!("hive_sleep_status_{}", std::process::id()));
        let teacher = Arc::new(Teacher::new(Some(tmp.clone())));
        let cycle = SleepCycle::new(teacher, None);

        let status = cycle.status().await;
        assert!(status.last_sleep.is_none());
        let display = format!("{}", status);
        assert!(display.contains("never"));

        std::fs::remove_dir_all(&tmp).ok();
    }
