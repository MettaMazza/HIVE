use crate::models::tool::{ToolResult, ToolStatus};
use crate::agent::preferences::extract_tag;
use tokio::sync::mpsc;
use std::process::Stdio;
use std::sync::OnceLock;
use tokio::sync::Mutex;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;

fn daemons() -> &'static Mutex<HashMap<u32, (String, String)>> {
    static DAEMONS: OnceLock<Mutex<HashMap<u32, (String, String)>>> = OnceLock::new();
    DAEMONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn execute_process_manager(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    let action = extract_tag(&description, "action:").unwrap_or_else(|| "execute".to_string());
    
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("⚙️ Native Process Manager executing action: `{}`\n", action)).await;
    }

    let mut output;

    match action.as_str() {
        "execute" => {
            let cmd = extract_tag(&description, "command:").unwrap_or_else(|| description.clone());
            let child = tokio::process::Command::new("bash")
                .arg("-c")
                .arg(&cmd)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            match child {
                Ok(c) => {
                    let execution = timeout(Duration::from_secs(30), c.wait_with_output()).await;
                    match execution {
                        Ok(Ok(out)) => {
                            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                            if out.status.success() {
                                output = format!("Command Succeeded.\nSTDOUT:\n{}", stdout);
                                if !stderr.is_empty() {
                                    output.push_str(&format!("\nSTDERR:\n{}", stderr));
                                }
                            } else {
                                output = format!("Command Failed ({}).\nSTDOUT:\n{}\nSTDERR:\n{}", out.status, stdout, stderr);
                            }
                        }
                        Ok(Err(e)) => output = format!("I/O Error waiting for command: {}", e),
                        Err(_) => output = "Execution Timeout: Process exceeded 30 seconds and was terminated.".to_string(),
                    }
                }
                Err(e) => output = format!("Failed to spawn command: {}", e),
            }
        }
        "daemon" => {
            let cmd = extract_tag(&description, "command:").unwrap_or_default();
            if cmd.is_empty() {
                return ToolResult { task_id, output: "Error: Missing command:[...]".into(), tokens_used: 0, status: ToolStatus::Failed("Missing params".into()) };
            }

            let timestamp = chrono::Utc::now().timestamp();
            let daemon_dir = std::path::Path::new("memory/daemons");
            let _ = tokio::fs::create_dir_all(&daemon_dir).await;
            
            let log_file = format!("memory/daemons/daemon_{}.log", timestamp);
            
            let bg_cmd = format!("nohup {} > {} 2>&1 & echo $!", cmd, log_file);
            let child = tokio::process::Command::new("bash")
                .arg("-c")
                .arg(&bg_cmd)
                .output()
                .await;

            match child {
                Ok(out) => {
                    let pid_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    let pid_clean = pid_str.split_whitespace().last().unwrap_or("");
                    if let Ok(pid) = pid_clean.parse::<u32>() {
                        let mut map = daemons().lock().await;
                        map.insert(pid, (cmd.clone(), log_file.clone()));
                        output = format!("Daemon started successfully.\nPID: {}\nCommand: {}\nLog File: {}", pid, cmd, log_file);
                    } else {
                        output = format!("Failed to parse PID from background spawn: {}", pid_str);
                    }
                }
                Err(e) => output = format!("Failed to spawn daemon: {}", e),
            }
        }
        "list" => {
            let mut map = daemons().lock().await; // Lock once
            let mut active = Vec::new();
            let mut dead = Vec::new();

            for (&pid, (cmd, log)) in map.iter() {
                let status = std::process::Command::new("kill").arg("-0").arg(pid.to_string()).status();
                let is_alive = status.map(|s| s.success()).unwrap_or(false);
                if is_alive {
                    active.push(format!("PID: {} | Command: {} | Log: {}", pid, cmd, log));
                } else {
                    dead.push(pid);
                }
            }

            for pid in dead {
                map.remove(&pid);
            }

            if active.is_empty() {
                output = "No active daemons managed by HIVE.".to_string();
            } else {
                output = format!("--- Active HIVE Daemons ---\n{}", active.join("\n"));
            }
        }
        "read" => {
            let pid_str = extract_tag(&description, "pid:").unwrap_or_default();
            let lines_str = extract_tag(&description, "lines:").unwrap_or_else(|| "100".to_string());
            let lines_limit: usize = lines_str.parse().unwrap_or(100);
            
            if pid_str.is_empty() {
                return ToolResult { task_id, output: "Error: Missing pid:[...]".into(), tokens_used: 0, status: ToolStatus::Failed("Missing params".into()) };
            }

            if let Ok(pid) = pid_str.parse::<u32>() {
                let map = daemons().lock().await;
                if let Some((cmd, log_path)) = map.get(&pid) {
                    let path = log_path.clone();
                    let cmd_clone = cmd.clone();
                    drop(map); // drop lock before async read
                    
                    if let Ok(content) = tokio::fs::read_to_string(&path).await {
                        let lines: Vec<&str> = content.lines().collect();
                        let total = lines.len();
                        let start = total.saturating_sub(lines_limit);
                        let tail = lines[start..].join("\n");
                        output = format!("--- Daemon Logs PID {} ({}) ---\n{}", pid, cmd_clone, tail);
                    } else {
                        output = format!("Error: Log file {} not found or cannot be read.", path);
                    }
                } else {
                    output = format!("Error: PID {} is not managed by the HIVE Process Manager.", pid);
                }
            } else {
                output = "Invalid PID format.".to_string();
            }
        }
        "kill" => {
            let pid_str = extract_tag(&description, "pid:").unwrap_or_default();
            if pid_str.is_empty() {
                return ToolResult { task_id, output: "Error: Missing pid:[...]".into(), tokens_used: 0, status: ToolStatus::Failed("Missing params".into()) };
            }
            if let Ok(pid) = pid_str.parse::<u32>() {
                let mut map = daemons().lock().await;
                let _ = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).output();
                map.remove(&pid);
                output = format!("Force killed Daemon PID {}.", pid);
            } else {
                output = "Invalid PID format.".to_string();
            }
        }
        _ => {
            output = format!("Unknown action: {}. Valid actions: execute, daemon, list, read, kill.", action);
        }
    }

    ToolResult {
        task_id,
        output,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}
