    use super::*;

    #[test]
    fn test_extract_tag() {
        let desc = "action:[create] project:[my-app] model:[qwen3:32b]";
        assert_eq!(extract_tag(desc, "action"), Some("create".into()));
        assert_eq!(extract_tag(desc, "project"), Some("my-app".into()));
        assert_eq!(extract_tag(desc, "model"), Some("qwen3:32b".into()));
        assert_eq!(extract_tag(desc, "missing"), None);
    }

    #[test]
    fn test_extract_message() {
        let desc = "action:[prompt] session:[abc] message:[Build a React app with state management]";
        assert_eq!(extract_message(desc), Some("Build a React app with state management".into()));
    }

    #[test]
    fn test_extract_message_with_brackets() {
        let desc = "message:[Create an array like [1, 2, 3] in Python]";
        assert_eq!(extract_message(desc), Some("Create an array like [1, 2, 3] in Python".into()));
    }

    #[test]
    fn test_generate_config() {
        let config = generate_opencode_config(Path::new("/tmp/test"));
        let parsed: serde_json::Value = serde_json::from_str(&config).unwrap();
        assert_eq!(
            parsed["enabled_providers"][0].as_str(),
            Some("ollama")
        );
        assert_eq!(
            parsed["server"]["port"].as_u64(),
            Some(OPENCODE_PORT as u64)
        );
    }

    #[test]
    fn test_dir_size_human() {
        assert_eq!(dir_size_human(Path::new("/nonexistent")), "0 B");
    }

    #[tokio::test]
    async fn test_bridge_project_create() {
        let bridge = OpenCodeBridge::new("/tmp/hive_opencode_test");
        let _ = std::fs::remove_dir_all("/tmp/hive_opencode_test/workspace/opencode/test_proj");
        
        let result = bridge.create_project("test_proj").await;
        assert!(result.is_ok());
        assert!(PathBuf::from("/tmp/hive_opencode_test/workspace/opencode/test_proj").exists());
        
        // Duplicate should fail
        let result2 = bridge.create_project("test_proj").await;
        assert!(result2.is_err());

        // Invalid name should fail
        assert!(bridge.create_project("../evil").await.is_err());
        assert!(bridge.create_project("").await.is_err());

        // Cleanup
        let _ = std::fs::remove_dir_all("/tmp/hive_opencode_test");
    }

    #[tokio::test]
    async fn test_bridge_status_when_stopped() {
        let bridge = OpenCodeBridge::new("/tmp/hive_opencode_test2");
        let status = bridge.status().await;
        assert!(status.contains("not running"));
        let _ = std::fs::remove_dir_all("/tmp/hive_opencode_test2");
    }
