use crate::models::tool::{ToolResult, ToolStatus};
use crate::memory::MemoryStore;
use tokio::sync::mpsc;
use std::sync::Arc;
use crate::agent::preferences::extract_tag;

pub async fn execute_operate_turing_grid(
    task_id: String,
    description: String,
    memory: Arc<MemoryStore>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    let action = extract_tag(&description, "action:")
        .unwrap_or("read".to_string())
        .split_whitespace()
        .next()
        .unwrap_or("read")
        .to_lowercase();

    if let Some(ref tx) = telemetry_tx {
        let _ = tx
            .send(format!(
                "🚀 Turing Grid Drone executing action: `{}`\n",
                action
            ))
            .await;
    }
    tracing::debug!("[AGENT:turing_grid] ▶ task_id={} action='{}'", task_id, action);

    let output;

    match action.as_str() {
        "move" => {
            let dx = extract_tag(&description, "dx:")
                .unwrap_or("0".to_string())
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse::<i32>()
                .unwrap_or(0);
            let dy = extract_tag(&description, "dy:")
                .unwrap_or("0".to_string())
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse::<i32>()
                .unwrap_or(0);
            let dz = extract_tag(&description, "dz:")
                .unwrap_or("0".to_string())
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse::<i32>()
                .unwrap_or(0);

            let mut g = memory.turing_grid.lock().await;
            g.move_cursor(dx, dy, dz).await;
            let (x, y, z) = g.get_cursor();
            output = format!(
                "Moved Read/Write head. Current coordinates: ({}, {}, {})",
                x, y, z
            );
        }
        "scan" => {
            let radius = extract_tag(&description, "radius:")
                .unwrap_or("5".to_string())
                .split_whitespace()
                .next()
                .unwrap_or("5")
                .parse::<i32>()
                .unwrap_or(5);
            let g = memory.turing_grid.lock().await;
            let results = g.scan(radius);

            let mut out = String::new();
            for (coords, fmt) in results {
                out.push_str(&format!(
                    "* Cell ({}, {}, {}) [Format: {}]\n",
                    coords.0, coords.1, coords.2, fmt
                ));
            }
            if out.is_empty() {
                output = format!("No non-empty cells found within radius {}.", radius);
            } else {
                output = format!("--- Radar Scan (Radius {}): ---\n{}", radius, out);
            }
        }
        "read_range" => {
            let x_range = extract_tag(&description, "x_bounds:").unwrap_or_default();
            let y_range = extract_tag(&description, "y_bounds:").unwrap_or_default();
            let z_range = extract_tag(&description, "z_bounds:").unwrap_or_default();

            // Format example: x_bounds:[0,10]
            let parse_bounds = |raw: &str| -> Option<(i32, i32)> {
                let s = raw.trim().trim_start_matches('[').trim_end_matches(']');
                let mut parts = s.split(',');
                if let (Some(p1), Some(p2)) = (parts.next(), parts.next()) {
                    if let (Ok(min), Ok(max)) = (p1.trim().parse::<i32>(), p2.trim().parse::<i32>()) {
                        return Some((min, max));
                    }
                }
                None
            };

            let (xmin, xmax) = parse_bounds(&x_range).unwrap_or((0, 0));
            let (ymin, ymax) = parse_bounds(&y_range).unwrap_or((0, 0));
            let (zmin, zmax) = parse_bounds(&z_range).unwrap_or((0, 0));

            let g = memory.turing_grid.lock().await;
            let mut out = String::new();
            let mut found = 0;
            
            // To prevent massive loops, bound delta max to 20
            let x_bound = (xmax - xmin).abs().min(20);
            let y_bound = (ymax - ymin).abs().min(20);
            let z_bound = (zmax - zmin).abs().min(20);

            for x in xmin..=(xmin + x_bound) {
                for y in ymin..=(ymin + y_bound) {
                    for z in zmin..=(zmin + z_bound) {
                        if let Some(cell) = g.read_at(x, y, z) {
                            found += 1;
                            let limit_content: String = cell.content.chars().take(300).collect();
                            out.push_str(&format!("* Cell ({}, {}, {}) [{}]: {}\n", x, y, z, cell.format, limit_content.replace('\n', " ")));
                        }
                    }
                }
            }

            if found == 0 {
                output = format!("No cells found in range X[{}-{}] Y[{}-{}] Z[{}-{}].", xmin, xmin+x_bound, ymin, ymin+y_bound, zmin, zmin+z_bound);
            } else {
                output = format!("--- Turing Range Read ({} cells) ---\n{}", found, out);
            }
        }
        "write" => {
            let format_tag = extract_tag(&description, "format:").unwrap_or("text".to_string());
            let format_str = format_tag.split_whitespace().next().unwrap_or("text").to_string();
            let content = if let Some(idx) = description.find("content:") {
                description[idx + 8..].trim().to_string()
            } else {
                return ToolResult {
                    task_id,
                    output: "Error: No content provided for write.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Missing field".into()),
                };
            };

            let mut g = memory.turing_grid.lock().await;
            let (x, y, z) = g.get_cursor();

            if let Err(e) = g.write_current(&format_str, &content).await {
                return ToolResult {
                    task_id,
                    output: format!("Failed to write: {}", e),
                    tokens_used: 0,
                    status: ToolStatus::Success,
                };
            }
            output = format!("Successfully wrote payload to cell ({}, {}, {}).", x, y, z);
        }
        "read" => {
            let g = memory.turing_grid.lock().await;
            let (x, y, z) = g.get_cursor();

            if let Some(cell) = g.read_current() {
                let links_info = if cell.links.is_empty() {
                    String::new()
                } else {
                    let link_strs: Vec<String> = cell.links.iter()
                        .map(|(lx, ly, lz)| format!("({},{},{})", lx, ly, lz))
                        .collect();
                    format!("\nLinks → {}", link_strs.join(", "))
                };
                let history_info = if cell.history.is_empty() {
                    String::new()
                } else {
                    format!("\nHistory: {} previous version(s) available", cell.history.len())
                };
                output = format!(
                    "Cell ({}, {}, {}) [Format: {}, Status: {}]:\n{}{}{}",
                    x, y, z, cell.format, cell.status, cell.content, links_info, history_info
                );
            } else {
                output = format!("Cell ({}, {}, {}) is empty.", x, y, z);
            }
        }
        "execute" => {
            let (format_str, content): (String, String) = {
                let mut g = memory.turing_grid.lock().await;
                let cell_info = g
                    .read_current()
                    .map(|cell| (cell.format.clone(), cell.content.clone()));

                match cell_info {
                    Some(info) => {
                        let _ = g.update_status("Running").await;
                        info
                    }
                    None => {
                        return ToolResult {
                            task_id,
                            output: "Error: Current cell is empty. Cannot execute.".into(),
                            tokens_used: 0,
                            status: ToolStatus::Success,
                        };
                    }
                }
            };

            let execute_result = memory.alu.execute_cell(&format_str, &content).await;

            let mut g = memory.turing_grid.lock().await;
            match execute_result {
                Ok(stdout) => {
                    let _ = g.update_status("Idle").await;
                    output = format!("Cell Executed Successfully.\nSTDOUT:\n{}", stdout);
                }
                Err(e) => {
                    let _ = g.update_status("Failed").await;
                    output = e;
                }
            }
        }

        // ──────────────────────────────────────────────
        //  NEW ACTIONS
        // ──────────────────────────────────────────────

        "deploy_daemon" => {
            let interval = extract_tag(&description, "interval:")
                .unwrap_or("60".to_string())
                .split_whitespace()
                .next()
                .unwrap_or("60")
                .parse::<u64>()
                .unwrap_or(60)
                .max(10); // Minimum 10 seconds to protect hardware

            let (format_str, content, coord_idx): (String, String, (i32, i32, i32)) = {
                let mut g = memory.turing_grid.lock().await;
                let cell_info = g.read_current().map(|c| (c.format.clone(), c.content.clone()));
                let coords = g.get_cursor();

                match cell_info {
                    Some(info) => {
                        // Apply Lock
                        match g.set_daemon_active(true).await {
                            Ok(true) => (info.0, info.1, coords),
                            Ok(false) => {
                                return ToolResult {
                                    task_id,
                                    output: format!("Error: A Daemon is already actively running on cell ({},{},{}). Write new data to reset it.", coords.0, coords.1, coords.2),
                                    tokens_used: 0,
                                    status: ToolStatus::Failed("Lock contention".into()),
                                };
                            }
                            Err(e) => {
                                return ToolResult {
                                    task_id,
                                    output: format!("Error setting lock: {}", e),
                                    tokens_used: 0,
                                    status: ToolStatus::Failed("IO Error".into()),
                                };
                            }
                        }
                    }
                    None => {
                        return ToolResult {
                            task_id,
                            output: "Error: Current cell is empty. Cannot deploy daemon.".into(),
                            tokens_used: 0,
                            status: ToolStatus::Success,
                        };
                    }
                }
            };

            // Spawn the detached async loop
            let mem_clone = memory.clone();
            tokio::spawn(async move {
                tracing::info!("[DAEMON] Turing Daemon spawned on ({},{},{}) interval: {}s", coord_idx.0, coord_idx.1, coord_idx.2, interval);
                let (x, y, z) = coord_idx;
                
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
                    
                    // Re-check lock just in case user overwrote cell manually during loop
                    {
                        let check_g = mem_clone.turing_grid.lock().await;
                        if let Some(cell) = check_g.read_at(x, y, z) {
                            if !cell.daemon_active {
                                tracing::info!("[DAEMON] Daemon killed automatically on ({},{},{}) due to lock removal.", x, y, z);
                                break;
                            }
                        } else {
                            break; // cell deleted
                        }
                        
                        // We must update status to Running specifically for this daemon's target cell
                        // But wait! update_status affects the current cursor, which might be anywhere now.
                        // We will skip updating the "status" text in background daemons to avoid racing the user's cursor.
                    }

                    match mem_clone.alu.execute_cell(&format_str, &content).await {
                        Ok(stdout) => {
                            if !stdout.is_empty() {
                                let daemon_output = format!("TURING DAEMON ({},{},{}) interval [{}]\n{}", x, y, z, interval, stdout);
                                let ev = crate::models::message::Event {
                                    platform: "system:daemon".to_string(),
                                    scope: crate::models::scope::Scope::Private { user_id: "system".to_string() },
                                    author_name: "Internal Daemon".to_string(),
                                    author_id: "daemon".to_string(),
                                    content: daemon_output,
                                    timestamp: Some(chrono::Utc::now().to_rfc3339()),
                                    message_index: None,
                                };
                                let _ = mem_clone.add_event(ev).await;
                            }
                        }
                        Err(e) => {
                            let daemon_error = format!("TURING DAEMON ({},{},{}) FAILED\n{}", x, y, z, e);
                            let ev = crate::models::message::Event {
                                    platform: "system:daemon".to_string(),
                                    scope: crate::models::scope::Scope::Private { user_id: "system".to_string() },
                                    author_name: "Internal Daemon".to_string(),
                                    author_id: "daemon".to_string(),
                                    content: daemon_error,
                                    timestamp: Some(chrono::Utc::now().to_rfc3339()),
                                    message_index: None,
                                };
                            let _ = mem_clone.add_event(ev).await;
                            // Failsafe exit loop if the execution throws an error (e.g., bad syntax) to prevent spam
                            let mut kill_g = mem_clone.turing_grid.lock().await;
                            let old_cur = kill_g.get_cursor();
                            kill_g.cursor = coord_idx;
                            let _ = kill_g.set_daemon_active(false).await;
                            kill_g.cursor = old_cur;
                            break;
                        }
                    }
                }
            });

            output = format!("Successfully deployed Turing Daemon at ({},{},{}) running every {} seconds in the background.", coord_idx.0, coord_idx.1, coord_idx.2, interval);
        }

        "index" => {
            let g = memory.turing_grid.lock().await;
            output = g.get_index();
        }

        "label" => {
            let name = match extract_tag(&description, "name:") {
                Some(n) => n.split_whitespace().next().unwrap_or("").to_string(),
                None => {
                    return ToolResult {
                        task_id,
                        output: "Error: No label name provided. Use 'name:[label_name]'.".into(),
                        tokens_used: 0,
                        status: ToolStatus::Failed("Missing field".into()),
                    };
                }
            };
            if name.is_empty() {
                return ToolResult {
                    task_id,
                    output: "Error: Label name cannot be empty.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Empty name".into()),
                };
            }

            let mut g = memory.turing_grid.lock().await;
            let (x, y, z) = g.get_cursor();
            if let Err(e) = g.set_label(&name).await {
                output = format!("Failed to set label: {}", e);
            } else {
                output = format!("Label '{}' set at coordinates ({}, {}, {}).", name, x, y, z);
            }
        }

        "goto" => {
            let name = match extract_tag(&description, "name:") {
                Some(n) => n.split_whitespace().next().unwrap_or("").to_string(),
                None => {
                    return ToolResult {
                        task_id,
                        output: "Error: No label name provided. Use 'name:[label_name]'.".into(),
                        tokens_used: 0,
                        status: ToolStatus::Failed("Missing field".into()),
                    };
                }
            };

            let mut g = memory.turing_grid.lock().await;
            match g.goto_label(&name).await {
                Some((x, y, z)) => {
                    output = format!("Jumped to label '{}' at ({}, {}, {}).", name, x, y, z);
                }
                None => {
                    // List available labels to help
                    let available: Vec<String> = g.labels.keys().cloned().collect();
                    if available.is_empty() {
                        output = format!("Label '{}' not found. No labels have been set yet.", name);
                    } else {
                        output = format!(
                            "Label '{}' not found. Available labels: {}",
                            name,
                            available.join(", ")
                        );
                    }
                }
            }
        }

        "link" => {
            let tx_val = extract_tag(&description, "target_x:")
                .unwrap_or("0".to_string())
                .split_whitespace().next().unwrap_or("0")
                .parse::<i32>().unwrap_or(0);
            let ty_val = extract_tag(&description, "target_y:")
                .unwrap_or("0".to_string())
                .split_whitespace().next().unwrap_or("0")
                .parse::<i32>().unwrap_or(0);
            let tz_val = extract_tag(&description, "target_z:")
                .unwrap_or("0".to_string())
                .split_whitespace().next().unwrap_or("0")
                .parse::<i32>().unwrap_or(0);

            let mut g = memory.turing_grid.lock().await;
            let (x, y, z) = g.get_cursor();

            match g.add_link((tx_val, ty_val, tz_val)).await {
                Ok(true) => {
                    output = format!(
                        "Linked cell ({},{},{}) → ({},{},{}).",
                        x, y, z, tx_val, ty_val, tz_val
                    );
                }
                Ok(false) => {
                    output = format!(
                        "Error: Cell ({},{},{}) is empty. Write data before linking.",
                        x, y, z
                    );
                }
                Err(e) => {
                    output = format!("Link failed: {}", e);
                }
            }
        }

        "history" => {
            let g = memory.turing_grid.lock().await;
            let (x, y, z) = g.get_cursor();

            match g.get_history() {
                Some(hist) if !hist.is_empty() => {
                    let mut out = format!(
                        "--- Version History for Cell ({},{},{}) ({} entries) ---\n",
                        x, y, z, hist.len()
                    );
                    for (i, snap) in hist.iter().enumerate() {
                        let preview: String = snap.content.chars().take(100).collect();
                        out.push_str(&format!(
                            "  v-{}: [{}] {} (at {})\n",
                            i + 1, snap.format, preview, snap.timestamp
                        ));
                    }
                    output = out;
                }
                Some(_) => {
                    output = format!("Cell ({},{},{}) has no version history.", x, y, z);
                }
                None => {
                    output = format!("Cell ({},{},{}) is empty — no history.", x, y, z);
                }
            }
        }

        "undo" => {
            let mut g = memory.turing_grid.lock().await;
            let (x, y, z) = g.get_cursor();

            match g.undo().await {
                Ok(true) => {
                    let cell = g.read_current().unwrap();
                    output = format!(
                        "Undo successful. Cell ({},{},{}) restored to previous version.\nContent: {}",
                        x, y, z, cell.content
                    );
                }
                Ok(false) => {
                    output = format!(
                        "Cannot undo: Cell ({},{},{}) has no version history.",
                        x, y, z
                    );
                }
                Err(e) => {
                    output = format!("Undo failed: {}", e);
                }
            }
        }

        "pipeline" => {
            let cells_raw = match extract_tag(&description, "cells:") {
                Some(c) => c,
                None => {
                    return ToolResult {
                        task_id,
                        output: "Error: No cells specified. Use 'cells:[(x,y,z),(x,y,z),...]'.".into(),
                        tokens_used: 0,
                        status: ToolStatus::Failed("Missing field".into()),
                    };
                }
            };

            // Parse coordinate tuples from the cells tag
            let mut pipeline_cells: Vec<(String, String)> = Vec::new();
            {
                let g = memory.turing_grid.lock().await;

                // Parse coordinate tuples: find (N,N,N) patterns without regex
                let mut coords: Vec<(i32, i32, i32)> = Vec::new();
                let mut remaining = cells_raw.as_str();
                while let Some(open) = remaining.find('(') {
                    if let Some(close) = remaining[open..].find(')') {
                        let inner = &remaining[open + 1..open + close];
                        let parts: Vec<&str> = inner.split(',').collect();
                        if parts.len() == 3
                            && let (Ok(x), Ok(y), Ok(z)) = (
                                parts[0].trim().parse::<i32>(),
                                parts[1].trim().parse::<i32>(),
                                parts[2].trim().parse::<i32>(),
                            ) {
                                coords.push((x, y, z));
                            }
                        remaining = &remaining[open + close + 1..];
                    } else {
                        break;
                    }
                }

                if coords.is_empty() {
                    return ToolResult {
                        task_id,
                        output: "Error: Could not parse cell coordinates. Use format: cells:[(0,0,0),(1,0,0)]".into(),
                        tokens_used: 0,
                        status: ToolStatus::Failed("Parse error".into()),
                    };
                }

                for (x, y, z) in &coords {
                    match g.read_at(*x, *y, *z) {
                        Some(cell) => {
                            pipeline_cells.push((cell.format.clone(), cell.content.clone()));
                        }
                        None => {
                            return ToolResult {
                                task_id,
                                output: format!("Error: Cell ({},{},{}) is empty. Pipeline aborted.", x, y, z),
                                tokens_used: 0,
                                status: ToolStatus::Failed("Empty cell in pipeline".into()),
                            };
                        }
                    }
                }
            }

            // Update statuses to Running
            {
                // We don't update individual cell statuses for the pipeline since
                // it would require moving the cursor. Just run them.
            }

            match memory.alu.execute_pipeline(&pipeline_cells).await {
                Ok(result) => {
                    output = format!("Pipeline executed successfully ({} cells).\n\n{}", pipeline_cells.len(), result);
                }
                Err(e) => {
                    output = format!("Pipeline failed.\n\n{}", e);
                }
            }
        }

        _ => {
            return ToolResult {
                task_id,
                output: format!("Unknown action: {}", action),
                tokens_used: 0,
                status: ToolStatus::Failed("Invalid command".into()),
            };
        }
    }

    ToolResult {
        task_id,
        output,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

#[cfg(test)]
mod tests {
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
}
