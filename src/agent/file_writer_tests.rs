    use super::*;

    #[tokio::test]
    async fn test_execute_file_writer_missing_args() {
        let res = execute_file_writer("1".into(), "".into(), None, None).await;
        assert!(matches!(res.status, ToolStatus::Failed(_)));
    }

    #[tokio::test]
    async fn test_execute_file_writer_flow() {
        let tmp = tempfile::tempdir().unwrap();
        let drafts_dir = tmp.path().join("drafts");
        let output_dir = tmp.path().join("rendered");

        let get_composer = || {
            Some(crate::computer::document::DocumentComposer::with_dirs(
                drafts_dir.clone(),
                output_dir.clone(),
            ))
        };

        // 1. Start Document
        let start_desc = "action:[start] id:[test1] title:[My Test Phase]";
        let res = execute_file_writer("2".into(), start_desc.into(), get_composer(), None).await;
        assert!(res.output.contains("started"));

        // 2. Add Section (no payload)
        let bad_add = "action:[add_section] id:[test1] heading:[Intro]";
        let res = execute_file_writer("3".into(), bad_add.into(), get_composer(), None).await;
        assert_eq!(res.status, ToolStatus::Failed("Missing Content".into()));

        // 3. Add Section (with payload)
        let add = "action:[add_section] id:[test1] heading:[Intro] content:Hello from tests!";
        let res = execute_file_writer("4".into(), add.into(), get_composer(), None).await;
        assert!(res.output.contains("Added section"));

        // 4. Render
        let render = "action:[render] id:[test1]";
        let res = execute_file_writer("5".into(), render.into(), get_composer(), None).await;
        // Headless Chrome rendering might fail gracefully or succeed. Both are handled.
        assert!(res.output.contains("complete") || res.output.contains("Failed to render PDF"));

        // 5. Unknown Action
        let unknown = "action:[ghost] id:[test1]";
        let res = execute_file_writer("6".into(), unknown.into(), get_composer(), None).await;
        assert!(res.output.contains("Unknown document action"));
    }

    #[tokio::test]
    async fn test_execute_file_writer_editing_actions() {
        let tmp = tempfile::tempdir().unwrap();
        let drafts_dir = tmp.path().join("drafts");
        let output_dir = tmp.path().join("rendered");

        let get_composer = || {
            Some(crate::computer::document::DocumentComposer::with_dirs(
                drafts_dir.clone(),
                output_dir.clone(),
            ))
        };

        // Setup: Start + two sections
        let _ = execute_file_writer("s".into(), "action:[start] id:[edit_test] title:[Edit Test]".into(), get_composer(), None).await;
        let _ = execute_file_writer("a1".into(), "action:[add_section] id:[edit_test] heading:[First] content:Hello world".into(), get_composer(), None).await;
        let _ = execute_file_writer("a2".into(), "action:[add_section] id:[edit_test] heading:[Second] content:Goodbye world".into(), get_composer(), None).await;

        // 1. List drafts
        let res = execute_file_writer("l".into(), "action:[list_drafts] id:[any]".into(), get_composer(), None).await;
        assert!(res.output.contains("edit_test"));

        // 2. Inspect
        let res = execute_file_writer("i".into(), "action:[inspect] id:[edit_test]".into(), get_composer(), None).await;
        assert!(res.output.contains("[0]"));
        assert!(res.output.contains("First"));
        assert!(res.output.contains("[1]"));
        assert!(res.output.contains("Second"));

        // 3. Edit section
        let res = execute_file_writer("e".into(), "action:[edit_section] id:[edit_test] index:[0] heading:[Updated First] content:New content here".into(), get_composer(), None).await;
        assert!(res.output.contains("updated"));

        // Verify edit took effect
        let res = execute_file_writer("i2".into(), "action:[inspect] id:[edit_test]".into(), get_composer(), None).await;
        assert!(res.output.contains("Updated First"));
        assert!(res.output.contains("New content"));

        // 4. Remove section
        let res = execute_file_writer("r".into(), "action:[remove_section] id:[edit_test] index:[1]".into(), get_composer(), None).await;
        assert!(res.output.contains("removed"));

        // Verify removal
        let res = execute_file_writer("i3".into(), "action:[inspect] id:[edit_test]".into(), get_composer(), None).await;
        assert!(res.output.contains("Sections: 1"));

        // 5. Update theme
        let res = execute_file_writer("t".into(), "action:[update_theme] id:[edit_test] theme:[cyberpunk]".into(), get_composer(), None).await;
        assert!(res.output.contains("cyberpunk"));

        // 6. Edit section out of range
        let res = execute_file_writer("bad".into(), "action:[edit_section] id:[edit_test] index:[99] heading:[X] content:Y".into(), get_composer(), None).await;
        assert!(res.output.contains("out of range"));

        // 7. Inspect non-existent draft
        let res = execute_file_writer("no".into(), "action:[inspect] id:[nonexistent]".into(), get_composer(), None).await;
        assert!(res.output.contains("Failed"));
    }
