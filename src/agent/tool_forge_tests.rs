    use super::*;

    #[test]
    fn test_tags_to_json() {
        let input = "city:[London] units:[metric]";
        let json = tags_to_json(input);
        let parsed: HashMap<String, String> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.get("city"), Some(&"London".to_string()));
        assert_eq!(parsed.get("units"), Some(&"metric".to_string()));
    }

    #[test]
    fn test_extract_code() {
        let desc = "action:[create] name:[test] code:[print('hello')]";
        assert_eq!(extract_code(desc), Some("print('hello')".into()));

        // Code with brackets inside
        let desc2 = "code:[arr = [1, 2, 3]]";
        assert_eq!(extract_code(desc2), Some("arr = [1, 2, 3]".into()));
    }

    #[tokio::test]
    async fn test_forge_crud() {
        let _ = std::fs::remove_dir_all("/tmp/hive_test_forge/memory/tools");
        let forge = ToolForge::new("/tmp/hive_test_forge");
        
        // Create
        let result = forge.create_tool(
            "test_tool".into(),
            "A test tool".into(),
            "python".into(),
            "import sys, json\nargs = json.loads(sys.stdin.read())\nprint(json.dumps({'result': 'ok'}))".into(),
            "test".into(),
        ).await;
        assert!(result.is_ok());

        // List
        let list = forge.list_tools().await;
        assert!(list.contains("test_tool"));

        // Get
        let tool = forge.get_tool("test_tool").await;
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().version, 1);

        // Edit
        let result = forge.edit_tool("test_tool", "print('v2')".into()).await;
        assert!(result.is_ok());
        let tool = forge.get_tool("test_tool").await.unwrap();
        assert_eq!(tool.version, 2);

        // Disable
        forge.set_enabled("test_tool", false).await.unwrap();
        let enabled = forge.get_enabled_tools().await;
        assert!(enabled.is_empty());

        // Enable
        forge.set_enabled("test_tool", true).await.unwrap();
        let enabled = forge.get_enabled_tools().await;
        assert_eq!(enabled.len(), 1);

        // Duplicate create fails
        let result = forge.create_tool(
            "test_tool".into(), "".into(), "python".into(), "".into(), "test".into(),
        ).await;
        assert!(result.is_err());

        // Delete
        forge.delete_tool("test_tool").await.unwrap();
        assert!(forge.get_tool("test_tool").await.is_none());
    }

    #[tokio::test]
    async fn test_forge_validation() {
        let forge = ToolForge::new("/tmp/hive_test_forge_val");

        // Path traversal
        let result = forge.create_tool("../evil".into(), "".into(), "python".into(), "".into(), "".into()).await;
        assert!(result.is_err());

        // Invalid language
        let result = forge.create_tool("good".into(), "".into(), "ruby".into(), "".into(), "".into()).await;
        assert!(result.is_err());

        // Space in name
        let result = forge.create_tool("bad name".into(), "".into(), "python".into(), "".into(), "".into()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_forged_tool() {
        let _ = std::fs::remove_dir_all("/tmp/hive_test_forge_exec/memory/tools");
        let forge = ToolForge::new("/tmp/hive_test_forge_exec");
        forge.create_tool(
            "echo_tool".into(),
            "Echoes input".into(),
            "python".into(),
            "import sys, json\nargs = json.loads(sys.stdin.read())\nprint(json.dumps({'echo': args.get('message', 'empty')}))".into(),
            "test".into(),
        ).await.unwrap();

        let tool_def = forge.get_tool("echo_tool").await.unwrap();
        let result = execute_forged_tool(
            "t1".into(),
            "message:[hello world]".into(),
            tool_def,
            forge.tools_dir.clone(),
            None,
        ).await;
        assert_eq!(result.status, ToolStatus::Success);
        assert!(result.output.contains("hello world"));
    }
