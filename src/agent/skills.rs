use crate::models::tool::{ToolResult, ToolStatus};
use crate::memory::MemoryStore;
use std::sync::Arc;
use tokio::sync::mpsc;

fn extract_tag(desc: &str, tag: &str) -> Option<String> {
    if let Some(start_idx) = desc.find(tag) {
        let after_tag = &desc[start_idx + tag.len()..];
        if after_tag.starts_with('[')
            && let Some(end_idx) = after_tag.find(']') {
                return Some(after_tag[1..end_idx].trim().to_string());
            }
    }
    None
}

pub async fn execute_manage_skill(
    task_id: String,
    description: String,
    is_admin: bool,
    memory: Arc<MemoryStore>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("⚙️ Manage Skill Drone executing...\n".to_string()).await;
    }
    tracing::debug!("[AGENT:skills] ▶ task_id={} is_admin={}", task_id, is_admin);

    if !is_admin {
        return ToolResult { 
            task_id, 
            output: "PERMISSION DENIED: The Skills System allows remote code execution and is strictly limited to Admin users. Request denied.".into(), 
            tokens_used: 0, 
            status: ToolStatus::Failed("Permission Denied".into()) 
        };
    }

    let action = extract_tag(&description, "action:").unwrap_or("list".to_string());
    let skill_name = extract_tag(&description, "name:").unwrap_or("".to_string());
    let scope_str = extract_tag(&description, "scope:").unwrap_or("public_general".to_string());
    
    // Safety check against path traversal
    if skill_name.contains("..") || skill_name.contains('/') {
        return ToolResult { task_id, output: "Error: Invalid skill name.".into(), tokens_used: 0, status: ToolStatus::Failed("Path traversal".into()) };
    }

    // Double Scoping Storage path
    let mut skills_dir = memory.working.get_memory_dir();
    if scope_str.starts_with("public_") {
        skills_dir.push(format!("public_{}", scope_str.replace("public_", "")));
        skills_dir.push("system"); // System or active user mapping
    } else {
        skills_dir.push(format!("private_{}", scope_str.replace("private_", "")));
    }
    skills_dir.push("skills");

    let _ = tokio::fs::create_dir_all(&skills_dir).await;

    let output = match action.as_str() {
        "list" => {
            let mut entries = vec![];
            if let Ok(mut rd) = tokio::fs::read_dir(&skills_dir).await {
                while let Ok(Some(entry)) = rd.next_entry().await {
                    if let Ok(name) = entry.file_name().into_string()
                        && (name.ends_with(".py") || name.ends_with(".sh")) {
                            entries.push(name);
                        }
                }
            }
            if entries.is_empty() {
                "No custom skills found for this scope.".into()
            } else {
                format!("Available Custom Skills:\n- {}", entries.join("\n- "))
            }
        }
        "create" => {
            if skill_name.is_empty() || (!skill_name.ends_with(".py") && !skill_name.ends_with(".sh")) {
                "Error: Skill name must end with .py or .sh".into()
            } else {
                let content = if let Some(idx) = description.find("content:[") {
                    let mut end = description.len();
                    if description.ends_with("]") {
                        end -= 1;
                    }
                    description[idx + 9..end].trim().to_string()
                } else {
                    return ToolResult { task_id, output: "Error: Missing content.".into(), tokens_used: 0, status: ToolStatus::Failed("No content".into()) };
                };
                
                let target_path = skills_dir.join(&skill_name);
                match tokio::fs::write(&target_path, content).await {
                    Ok(_) => {
                        // Make bash scripts executable
                        if skill_name.ends_with(".sh") {
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                if let Ok(mut perms) = tokio::fs::metadata(&target_path).await.map(|m| m.permissions()) {
                                    perms.set_mode(0o755);
                                    let _ = tokio::fs::set_permissions(&target_path, perms).await;
                                }
                            }
                        }
                        format!("Successfully created custom skill: {}", skill_name)
                    }
                    Err(e) => format!("Failed to create skill: {}", e)
                }
            }
        }
        "info" => {
            if skill_name.is_empty() {
                "Error: Must specify name:[skill.ext]".into()
            } else {
                let target_path = skills_dir.join(&skill_name);
                if !target_path.exists() {
                    format!("Error: Skill '{}' not found in current scope.", skill_name)
                } else if let Ok(content) = tokio::fs::read_to_string(&target_path).await {
                    let mut args = std::collections::HashSet::new();
                    // Heuristic extraction
                    if skill_name.ends_with(".py") {
                        if content.contains("argparse") {
                            for line in content.lines() {
                                if line.contains("add_argument") {
                                    args.insert(line.trim().to_string());
                                }
                            }
                        }
                        if content.contains("sys.argv") || content.contains("sys.stdin") {
                            args.insert("Requires parameters via sys.argv[] or stdin JSON".to_string());
                        }
                    } else if skill_name.ends_with(".sh") {
                        for i in 1..=9 {
                            if content.contains(&format!("${}", i)) {
                                args.insert(format!("Positional ARG ${}", i));
                            }
                        }
                    }
                    if args.is_empty() {
                        format!("Skill '{}' requires no obvious arguments, or uses non-standard parsing.", skill_name)
                    } else {
                        let arg_list: Vec<String> = args.into_iter().collect();
                        format!("Skill '{}' expected arguments heuristic:\n- {}", skill_name, arg_list.join("\n- "))
                    }
                } else {
                    format!("Failed to read skill '{}'.", skill_name)
                }
            }
        }
        "execute" => {
            if skill_name.is_empty() {
                "Error: Must specify name:[skill.py]".into()
            } else {
                let target_path = skills_dir.join(&skill_name);
                if !target_path.exists() {
                    format!("Error: Skill '{}' does not exist in the current scope.", skill_name)
                } else {
                    let cmd = if skill_name.ends_with(".py") { "python3" } else { "bash" };
                    let path_str = target_path.to_string_lossy().to_string();
                    
                    if let Some(ref tx) = telemetry_tx {
                        let _ = tx.send(format!("🚀 SPARKING SKILL: `{} {}`\n", cmd, path_str)).await;
                    }

                    match tokio::time::timeout(
                        std::time::Duration::from_secs(30),
                        tokio::process::Command::new(cmd)
                            .arg(&path_str)
                            .output()
                    ).await {
                        Ok(Ok(exec_output)) => {
                            let stdout = String::from_utf8_lossy(&exec_output.stdout);
                            let stderr = String::from_utf8_lossy(&exec_output.stderr);
                            if exec_output.status.success() {
                                format!("--- SKILL SUCCESS ---\n{}\n{}", stdout, stderr)
                            } else {
                                format!("--- SKILL FAILED (Code: {}) ---\n{}\n{}", exec_output.status, stdout, stderr)
                            }
                        }
                        Ok(Err(e)) => format!("Failed to invoke process: {}", e),
                        Err(_) => "Skill execution timed out after 30 seconds. Process killed.".into()
                    }
                }
            }
        }
        _ => format!("Unknown action '{}'. Use create, list, or execute.", action)
    };

    tracing::debug!("[AGENT:skills] ◀ task_id={} action='{}' output_len={}", task_id, action, output.len());
    ToolResult { task_id, output, tokens_used: 0, status: ToolStatus::Success }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tag_skills() {
        assert_eq!(extract_tag("name:[hello.py]", "name:"), Some("hello.py".into()));
        assert_eq!(extract_tag("invalid", "name:"), None);
    }

    #[tokio::test]
    async fn test_skills_tool_execute() {
        let mem = Arc::new(MemoryStore::default());
        
        // 1. Not admin
        let res = execute_manage_skill("1".into(), "".into(), false, mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Failed("Permission Denied".into()));

        // 2. Traversal protection
        let res = execute_manage_skill("2".into(), "name:[../script.py]".into(), true, mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Failed("Path traversal".into()));

        // 3. Create missing extension
        let res = execute_manage_skill("3".into(), "action:[create] name:[bad_name]".into(), true, mem.clone(), None).await;
        assert!(res.output.contains("must end with .py or .sh"));

        // 4. Create missing content
        let res = execute_manage_skill("4".into(), "action:[create] name:[test.py]".into(), true, mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Failed("No content".into()));

        // 5. Create valid Python
        let py_code = "action:[create] name:[hello.py] content:[print(\"Hello Python\")]";
        let res = execute_manage_skill("5".into(), py_code.into(), true, mem.clone(), None).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(res.output.contains("Successfully created"));

        // 6. Create valid Bash
        let sh_code = "action:[create] name:[hello.sh] content:[echo \"Hello Bash\"]";
        let res = execute_manage_skill("6".into(), sh_code.into(), true, mem.clone(), None).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(res.output.contains("Successfully created"));

        // 7. List
        let res = execute_manage_skill("7".into(), "action:[list]".into(), true, mem.clone(), None).await;
        assert!(res.output.contains("hello.py"));
        assert!(res.output.contains("hello.sh"));

        // 8. Execute Python
        let res = execute_manage_skill("8".into(), "action:[execute] name:[hello.py]".into(), true, mem.clone(), None).await;
        assert!(res.output.contains("Hello Python"));

        // 9. Execute missing
        let res = execute_manage_skill("9".into(), "action:[execute] name:[ghost.py]".into(), true, mem.clone(), None).await;
        assert!(res.output.contains("does not exist"));

        // 10. Unknown action
        let res = execute_manage_skill("10".into(), "action:[fly]".into(), true, mem.clone(), None).await;
        assert!(res.output.contains("Unknown action"));

        // 11. Execute Timeout (Python sleep)
        let sleep_code = "action:[create] name:[sleep.py] content:[import time\ntime.sleep(2)]";
        execute_manage_skill("11".into(), sleep_code.into(), true, mem.clone(), None).await;

        // Since the timeout is hardcoded to 30s and we don't want tests to take 30s,
        // we won't natively wait 30s. Instead, we can't easily mock the timeout without changing the source code.
        // It's acceptable for the 30s timeout branch to remain uncovered unless we extract the timeout duration.
    }
}
