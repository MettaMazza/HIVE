    use super::*;

    #[tokio::test]
    async fn test_operate_turing_grid() {
        let memory = Arc::new(MemoryStore::default());
        let (tx, _rx) = tokio::sync::mpsc::channel(10);

        // Test move
        let r1 = execute_operate_turing_grid(
            "id".into(),
            "action:move dx:1 dy:2 dz:3".into(),
            memory.clone(),
            Some(tx.clone()),
        )
        .await;
        assert!(r1.output.contains("(1, 2, 3)"));

        // Test write
        let r2 = execute_operate_turing_grid(
            "id".into(),
            "action:write format:text content:hello grid test".into(),
            memory.clone(),
            Some(tx.clone()),
        )
        .await;
        assert!(r2.output.contains("Successfully wrote"));

        // Test read
        let r3 = execute_operate_turing_grid(
            "id".into(),
            "action:read".into(),
            memory.clone(),
            Some(tx.clone()),
        )
        .await;
        assert!(r3.output.contains("hello grid test"));

        // Test write fail (no content tag)
        let r4 = execute_operate_turing_grid(
            "id".into(),
            "action:write format:text".into(),
            memory.clone(),
            Some(tx.clone()),
        )
        .await;
        assert!(r4.output.contains("No content provided"));

        // Test scan
        let r5 = execute_operate_turing_grid(
            "id".into(),
            "action:scan radius:5".into(),
            memory.clone(),
            Some(tx.clone()),
        )
        .await;
        assert!(r5.output.contains("Radar Scan") && r5.output.contains("(1, 2, 3)"));

        // Test unknown action
        let r6 = execute_operate_turing_grid(
            "id".into(),
            "action:invalid".into(),
            memory.clone(),
            Some(tx.clone()),
        )
        .await;
        assert!(r6.output.contains("Unknown action"));

        // Test execute on empty cell (move to empty cell first)
        let _ = execute_operate_turing_grid(
            "id".into(),
            "action:move dx:99 dy:99 dz:99".into(),
            memory.clone(),
            Some(tx.clone()),
        )
        .await;
        let r7 = execute_operate_turing_grid(
            "id".into(),
            "action:execute".into(),
            memory.clone(),
            Some(tx.clone()),
        )
        .await;
        assert!(r7.output.contains("Current cell is empty"));
    }

    #[tokio::test]
    async fn test_tool_index_action() {
        let memory = Arc::new(MemoryStore::default());
        let (tx, _rx) = tokio::sync::mpsc::channel(10);

        // Write some cells
        let _ = execute_operate_turing_grid(
            "id".into(),
            "action:write format:text content:origin data".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;
        let _ = execute_operate_turing_grid(
            "id".into(),
            "action:move dx:1 dy:0 dz:0".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;
        let _ = execute_operate_turing_grid(
            "id".into(),
            "action:write format:python content:print('hello')".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;

        let result = execute_operate_turing_grid(
            "id".into(),
            "action:index".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;

        assert!(result.output.contains("2 cells"));
        assert!(result.output.contains("0,0,0"));
        assert!(result.output.contains("1,0,0"));
        assert!(result.output.contains("origin data"));
    }

    #[tokio::test]
    async fn test_tool_label_and_goto() {
        let memory = Arc::new(MemoryStore::default());
        let (tx, _rx) = tokio::sync::mpsc::channel(10);

        // Move and label
        let _ = execute_operate_turing_grid(
            "id".into(),
            "action:move dx:5 dy:5 dz:5".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;
        let label_result = execute_operate_turing_grid(
            "id".into(),
            "action:label name:home_base".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;
        assert!(label_result.output.contains("home_base"));
        assert!(label_result.output.contains("(5, 5, 5)"));

        // Move away
        let _ = execute_operate_turing_grid(
            "id".into(),
            "action:move dx:-5 dy:-5 dz:-5".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;

        // Goto label
        let goto_result = execute_operate_turing_grid(
            "id".into(),
            "action:goto name:home_base".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;
        assert!(goto_result.output.contains("Jumped to label"));
        assert!(goto_result.output.contains("(5, 5, 5)"));

        // Goto non-existent
        let missing = execute_operate_turing_grid(
            "id".into(),
            "action:goto name:doesnt_exist".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;
        assert!(missing.output.contains("not found"));

        // Label without name
        let no_name = execute_operate_turing_grid(
            "id".into(),
            "action:label".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;
        assert!(no_name.output.contains("Error"));
    }

    #[tokio::test]
    async fn test_tool_history_and_undo() {
        let memory = Arc::new(MemoryStore::default());
        let (tx, _rx) = tokio::sync::mpsc::channel(10);

        // Write v1
        let _ = execute_operate_turing_grid(
            "id".into(),
            "action:write format:text content:version one".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;

        // Write v2
        let _ = execute_operate_turing_grid(
            "id".into(),
            "action:write format:text content:version two".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;

        // Check history
        let hist = execute_operate_turing_grid(
            "id".into(),
            "action:history".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;
        assert!(hist.output.contains("1 entries"));
        assert!(hist.output.contains("version one"));

        // Undo
        let undo = execute_operate_turing_grid(
            "id".into(),
            "action:undo".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;
        assert!(undo.output.contains("Undo successful"));
        assert!(undo.output.contains("version one"));

        // Undo again — no more history
        let undo2 = execute_operate_turing_grid(
            "id".into(),
            "action:undo".into(),
            memory.clone(),
            Some(tx.clone()),
        ).await;
        assert!(undo2.output.contains("no version history"));
    }
