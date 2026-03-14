use std::sync::Arc;
use tokio::sync::mpsc;
use crate::models::tool::{ToolResult, ToolStatus};
use crate::models::scope::Scope;
use crate::memory::MemoryStore;
use crate::providers::Provider;

#[allow(clippy::too_many_arguments)]
pub fn dispatch_native_tool(
    task: &crate::agent::planner::AgentTask,
    context: &str,
    scope: &Scope,
    telemetry_tx: Option<mpsc::Sender<String>>,
    memory: Arc<MemoryStore>,
    provider: Arc<dyn Provider>,
    outreach_gate: Option<Arc<crate::engine::outreach::OutreachGate>>,
    inbox: Option<Arc<crate::engine::inbox::InboxManager>>,
    drives: Option<Arc<crate::engine::drives::DriveSystem>>,
) -> Option<tokio::task::JoinHandle<ToolResult>> {
    let task_id = task.task_id.clone();
    let desc = task.description.clone();
    let tx_clone = telemetry_tx.clone();
    let tool_type = task.tool_type.as_str();

    if tool_type == "channel_reader" {
        let mem_clone = memory.clone();
        let handle = tokio::spawn(async move {
            if let Some(ref tx) = tx_clone {
                let _ = tx.send(format!("🧠 Native Channel Reader Tool executing...\n")).await;
            }
            let target_id = desc.split_whitespace().last().unwrap_or(&"").to_string();
            let pub_scope = Scope::Public { channel_id: target_id.clone(), user_id: "system".into() };

            let output = if let Ok(timeline_data) = mem_clone.timeline.read_timeline(&pub_scope).await {
                String::from_utf8_lossy(&timeline_data).to_string()
            } else {
                "Failed to read timeline for channel.".to_string()
            };
            
            ToolResult {
                task_id,
                output,
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        });
        return Some(handle);
    } 
    
    if tool_type == "outreach" {
        let handle = tokio::spawn(crate::agent::outreach::execute_outreach(
            task_id, desc, outreach_gate, inbox, drives, tx_clone,
        ));
        return Some(handle);
    } 
    
    if tool_type == "codebase_list" {
        let handle = tokio::spawn(async move {
            if let Some(ref tx) = tx_clone {
                let _ = tx.send(format!("🧠 Native Codebase List Tool executing...\n")).await;
            }
            let output = match std::process::Command::new("find").arg("src").arg("-type").arg("f").output() {
                Ok(res) => String::from_utf8_lossy(&res.stdout).to_string(),
                Err(e) => format!("Failed to list codebase: {}", e),
            };
            ToolResult { task_id, output, tokens_used: 0, status: ToolStatus::Success }
        });
        return Some(handle);
    } 
    
    if tool_type == "codebase_read" {
        let handle = tokio::spawn(async move {
            crate::agent::file_reader::execute_file_reader(task_id, desc, tx_clone).await
        });
        return Some(handle);
    } 
    
    if tool_type == "web_search" || tool_type == "researcher" {
        let handle = tokio::spawn(async move {
            crate::agent::web_tool::execute_web_search(task_id, desc, tx_clone).await
        });
        return Some(handle);
    } 
    
    if tool_type == "generate_image" {
        let ctx_str = context.to_string();
        if ctx_str.contains("[ATTACH_IMAGE]") {
            if let Some(tx) = tx_clone.clone() {
                tokio::spawn(async move {
                    let _ = tx.send("⚠️ Blocked duplicate image generation attempt.\n".into()).await;
                });
            }
            let failure_result = ToolResult {
                task_id,
                output: "FATAL SYSTEM ERROR: YOU ALREADY GENERATED AN IMAGE IN THIS TURN. YOU ARE FORBIDDEN FROM GENERATING MULTIPLE IMAGES PER USER REQUEST. STOP USING TOOLS AND REPLY TO THE USER IMMEDIATELY.".to_string(),
                tokens_used: 0,
                status: ToolStatus::Failed("Blocked Duplicate Render".to_string())
            };
            return Some(tokio::spawn(async move { failure_result }));
        } else {
            return Some(tokio::spawn(crate::agent::image_tool::execute_generate_image(task_id, desc, tx_clone)));
        }
    } 
    
    if tool_type == "voice_synthesizer" {
        let handle = tokio::spawn(async move {
            crate::agent::tts_tool::execute_voice_synthesizer(task_id, desc, tx_clone).await
        });
        return Some(handle);
    } 
    
    if tool_type == "operate_turing_grid" {
        let mem_clone = memory.clone();
        let handle = tokio::spawn(async move {
            crate::agent::turing_tool::execute_operate_turing_grid(task_id, desc, mem_clone, tx_clone).await
        });
        return Some(handle);
    } 
    
    if tool_type == "file_writer" {
        let handle = tokio::spawn(async move {
            crate::agent::file_writer::execute_file_writer(task_id, desc, None, tx_clone).await
        });
        return Some(handle);
    } 
    
    if tool_type == "read_logs" {
        let handle = tokio::spawn(async move {
            crate::agent::log_tool::execute_read_logs(task_id, desc, tx_clone).await
        });
        return Some(handle);
    }
    if tool_type == "run_bash_command" {
        let name_clone = tool_type.to_string();
        let handle = tokio::spawn(async move {
            if let Some(ref tx) = tx_clone {
                let _ = tx.send(format!("⚙️ Native {} executing...\n", name_clone)).await;
            }
            // Run directly from the project root (NOT the ALU sandbox)
            // so that files created by file_system_operator are accessible.
            let child = tokio::process::Command::new("bash")
                .arg("-c")
                .arg(&desc)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn();

            match child {
                Ok(c) => {
                    let execution = tokio::time::timeout(
                        std::time::Duration::from_secs(15),
                        c.wait_with_output(),
                    ).await;
                    match execution {
                        Ok(Ok(out)) => {
                            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                            if out.status.success() {
                                ToolResult {
                                    task_id,
                                    output: if stdout.trim().is_empty() {
                                        "Command succeeded with no output.".into()
                                    } else {
                                        stdout.trim().to_string()
                                    },
                                    tokens_used: 0,
                                    status: ToolStatus::Success,
                                }
                            } else {
                                let msg = format!("Command Failed.\nSTDOUT:\n{}\nSTDERR:\n{}", stdout, stderr);
                                ToolResult { task_id, output: msg.clone(), tokens_used: 0, status: ToolStatus::Failed(msg) }
                            }
                        }
                        Ok(Err(e)) => {
                            let msg = format!("I/O Error: {}", e);
                            ToolResult { task_id, output: msg.clone(), tokens_used: 0, status: ToolStatus::Failed(msg) }
                        }
                        Err(_) => {
                            let msg = "Execution Timeout: Process exceeded 15 seconds.".to_string();
                            ToolResult { task_id, output: msg.clone(), tokens_used: 0, status: ToolStatus::Failed(msg) }
                        }
                    }
                }
                Err(e) => {
                    let msg = format!("Failed to spawn: {}", e);
                    ToolResult { task_id, output: msg.clone(), tokens_used: 0, status: ToolStatus::Failed(msg) }
                }
            }
        });
        return Some(handle);
    }
    if tool_type == "process_manager" {
        let handle = tokio::spawn(async move {
            crate::agent::process_manager::execute_process_manager(task_id, desc, tx_clone).await
        });
        return Some(handle);
    }
    
    if tool_type == "file_system_operator" {
        let handle = tokio::spawn(async move {
            crate::agent::file_system_tool::execute_file_system_operator(task_id, desc, tx_clone).await
        });
        return Some(handle);
    } 
    
    if tool_type == "autonomy_activity" {
        let handle = tokio::spawn(async move {
            crate::agent::autonomy_tool::execute_autonomy_activity(task_id, desc, tx_clone).await
        });
        return Some(handle);
    }
    
    if tool_type == "review_reasoning" {
        let mem_clone = memory.clone();
        let scope_clone = scope.clone();
        let handle = tokio::spawn(async move {
            crate::agent::reasoning_tool::execute_review_reasoning(task_id, desc, mem_clone, scope_clone, tx_clone).await
        });
        return Some(handle);
    }
    
    if tool_type == "read_attachment" {
        let handle = tokio::spawn(async move {
            crate::agent::attachment_tool::execute_read_attachment(task_id, desc, tx_clone).await
        });
        return Some(handle);
    }
    
    if tool_type == "manage_user_preferences" {
        let mem_clone = memory.clone();
        let scope_clone = scope.clone();
        let handle = tokio::spawn(async move {
            crate::agent::preferences::execute_manage_user_preferences(task_id, desc, scope_clone, mem_clone, tx_clone).await
        });
        return Some(handle);
    } 
    
    if tool_type == "manage_skill" {
        let mem_clone = memory.clone();
        let is_admin = true;
        let handle = tokio::spawn(async move {
            crate::agent::skills::execute_manage_skill(task_id, desc, is_admin, mem_clone, tx_clone).await
        });
        return Some(handle);
    } 
    
    if tool_type == "manage_routine" {
        let mem_clone = memory.clone();
        let handle = tokio::spawn(async move {
            crate::agent::routines::execute_manage_routine(task_id, desc, mem_clone, tx_clone).await
        });
        return Some(handle);
    } 
    
    if tool_type == "manage_lessons" {
        let mem_clone = memory.clone();
        let scope_clone = scope.clone();
        let handle = tokio::spawn(async move {
            crate::agent::lessons_tool::execute_manage_lessons(task_id, desc, mem_clone, tx_clone, &scope_clone).await
        });
        return Some(handle);
    } 
    
    if tool_type == "search_timeline" {
        let mem_clone = memory.clone();
        let scope_clone = scope.clone();
        let handle = tokio::spawn(async move {
            crate::agent::timeline_tool::execute_search_timeline(task_id, desc, mem_clone, tx_clone, &scope_clone).await
        });
        return Some(handle);
    }

    if tool_type == "manage_scratchpad" {
        let mem_clone = memory.clone();
        let scope_clone = scope.clone();
        let handle = tokio::spawn(async move {
            crate::agent::scratchpad_tool::execute_manage_scratchpad(task_id, desc, mem_clone, tx_clone, &scope_clone).await
        });
        return Some(handle);
    }

    if tool_type == "operate_synaptic_graph" {
        let mem_clone = memory.clone();
        let handle = tokio::spawn(async move {
            crate::agent::synaptic_tool::execute_operate_synaptic_graph(task_id, desc, mem_clone, tx_clone).await
        });
        return Some(handle);
    }

    if tool_type == "read_core_memory" {
        let mem_clone = memory.clone();
        let scope_clone = scope.clone();
        let handle = tokio::spawn(async move {
            crate::agent::core_memory_tool::execute_read_core_memory(task_id, desc, mem_clone, tx_clone, &scope_clone).await
        });
        return Some(handle);
    } 
    
    if tool_type == "synthesizer" {
        let ctx_clone = context.to_string();
        let handle = tokio::spawn(async move {
            crate::agent::synthesis_tool::execute_synthesizer(task_id, desc, ctx_clone, provider, tx_clone).await
        });
        return Some(handle);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::planner::AgentTask;

    #[tokio::test]
    async fn test_dispatch_all_branches() {
        let mem = Arc::new(MemoryStore::default());
        let scope = Scope::Public { channel_id: "t".into(), user_id: "t".into() };
        
        use crate::providers::MockProvider;
        let mut mock_provider = MockProvider::new();
        mock_provider.expect_generate().returning(|_, _, _, _, _| Ok("Mock".to_string()));
        let provider: Arc<dyn Provider> = Arc::new(mock_provider);
        
        let tools = vec![
            "channel_reader", "outreach", "codebase_list", "codebase_read",
            "web_search", "researcher", "generate_image", "voice_synthesizer",
            "operate_turing_grid", "file_writer", "read_logs", "run_bash_command",
            "process_manager", "file_system_operator", "autonomy_activity",
            "review_reasoning", "read_attachment", "manage_user_preferences",
            "manage_skill", "manage_routine", "manage_lessons", "search_timeline",
            "manage_scratchpad", "operate_synaptic_graph", "read_core_memory",
            "synthesizer"
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
        );
        assert!(none_handle.is_none());
    }
}
