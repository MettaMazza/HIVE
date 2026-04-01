use crate::models::tool::{ToolResult, ToolStatus};
use crate::agent::preferences::extract_tag;
use tokio::sync::mpsc;
use crate::engine::checkpoint::CheckpointManager;
use tokio::process::Command;

/// AutoResearch Ratchet Tool
/// Evaluates an experimental codebase change against a metric or test command.
/// If the evaluation fails, mechanically rolls back the codebase to the provided checkpoint.
pub async fn execute_ratchet_tool(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("🔬 AutoResearch Ratchet evaluating experiment...\n".to_string()).await;
    }

    let action = extract_tag(&description, "action:").unwrap_or_default();
    let command_str = extract_tag(&description, "command:").unwrap_or_default();
    let checkpoint_id = extract_tag(&description, "checkpoint_id:").unwrap_or_default();

    if action.is_empty() {
        return ToolResult {
            task_id,
            output: "Error: Missing action:[evaluate_test] or action:[evaluate_metric]".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing action".into()),
        };
    }
    
    if command_str.is_empty() || checkpoint_id.is_empty() {
        return ToolResult {
            task_id,
            output: "Error: Missing command:[...] or checkpoint_id:[...]".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing args".into()),
        };
    }

    // Prepare to execute the test script/command
    // For safety, we use 'sh -c' to evaluate custom commands correctly.
    let mut cmd_child = Command::new("sh");
    cmd_child.arg("-c").arg(&command_str);
    
    let checkpoint_mgr = CheckpointManager::new();

    match action.as_str() {
        "evaluate_test" => {
            if let Some(ref tx) = telemetry_tx {
                let _ = tx.send(format!("  ⚙️ Running test: `{}`\n", command_str)).await;
            }

            match cmd_child.output().await {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    let combined = format!("STDOUT:\n{}\nSTDERR:\n{}", stdout, stderr);

                    if output.status.success() {
                        ToolResult {
                            task_id,
                            output: format!("✅ Ratchet SUCCESS. The experiment passed.\nOutput:\n{}", combined),
                            tokens_used: 0,
                            status: ToolStatus::Success,
                        }
                    } else {
                        // The test failed. Mechanically rollback to save the LLM context and prevent hallucinations.
                        if let Some(ref tx) = telemetry_tx {
                            let _ = tx.send("  ❌ Test failed. Mechanically rolling back...\n".to_string()).await;
                        }
                        
                        match checkpoint_mgr.rollback(&checkpoint_id).await {
                            Ok(_) => ToolResult {
                                task_id,
                                output: format!("❌ Ratchet FAILED (exit code {}). Command output:\n{}\n\n[MECHANICAL ROLLBACK: The repository has been automatically reverted to checkpoint {} to guarantee a pristine state. Think step-by-step and try a completely new approach.]", output.status.code().unwrap_or(-1), combined, checkpoint_id),
                                tokens_used: 0,
                                // We return Success so the agent cleanly observes the failure as tool output.
                                status: ToolStatus::Success,
                            },
                            Err(e) => ToolResult {
                                task_id,
                                output: format!("❌ Ratchet FAILED. Command output:\n{}\n\n⚠️ CRITICAL ERROR: The mechanical rollback to '{}' failed: {}. The repository may be in a broken state.", combined, checkpoint_id, e),
                                tokens_used: 0,
                                status: ToolStatus::Failed("Rollback Failure".into()),
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = checkpoint_mgr.rollback(&checkpoint_id).await;
                    ToolResult {
                        task_id,
                        output: format!("❌ Command execution error: {}\nMechanically rolled back to {}", e, checkpoint_id),
                        tokens_used: 0,
                        status: ToolStatus::Failed("Command Execution Failure".into()),
                    }
                }
            }
        }
        "evaluate_metric" => {
            // Evaluates command output against a mathematical condition
            let condition = extract_tag(&description, "condition:").unwrap_or_default();
            if condition.is_empty() {
                return ToolResult {
                    task_id,
                    output: "Error: action:[evaluate_metric] requires condition:[< | > | == <value>]".into(),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Missing condition".into()),
                };
            }

            if let Some(ref tx) = telemetry_tx {
                let _ = tx.send(format!("  📈 Measuring metric: `{}` -> `{}`\n", command_str, condition)).await;
            }

            match cmd_child.output().await {
                Ok(output) => {
                    if !output.status.success() {
                        let _ = checkpoint_mgr.rollback(&checkpoint_id).await;
                        return ToolResult {
                            task_id,
                            output: format!("❌ Metric command failed (non-zero exit). Mechanically rolled back to {}.\nSTDERR:\n{}", checkpoint_id, String::from_utf8_lossy(&output.stderr)),
                            tokens_used: 0,
                            status: ToolStatus::Success,
                        };
                    }

                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let parsed_val: Result<f64, _> = stdout.parse();
                    
                    if let Ok(actual_val) = parsed_val {
                        // Parse condition, e.g. "< 0.5"
                        let parts: Vec<&str> = condition.split_whitespace().collect();
                        if parts.len() != 2 {
                             return ToolResult {
                                task_id,
                                output: "Error: condition must be formatted exactly as '< 0.5' or '> 90'".into(),
                                tokens_used: 0,
                                status: ToolStatus::Failed("Malformed condition".into()),
                            };
                        }

                        let op = parts[0];
                        let target_val: Result<f64, _> = parts[1].parse();

                        if let Ok(target) = target_val {
                            let success = match op {
                                "<" => actual_val < target,
                                ">" => actual_val > target,
                                "==" => (actual_val - target).abs() < f64::EPSILON,
                                "<=" => actual_val <= target,
                                ">=" => actual_val >= target,
                                _ => false,
                            };

                            if success {
                                ToolResult {
                                    task_id,
                                    output: format!("✅ Ratchet SUCCESS. Metric {} {} {} matched.\nChanges kept.", actual_val, op, target),
                                    tokens_used: 0,
                                    status: ToolStatus::Success,
                                }
                            } else {
                                // Revert
                                let _ = checkpoint_mgr.rollback(&checkpoint_id).await;
                                ToolResult {
                                    task_id,
                                    output: format!("❌ Ratchet FAILED. Metric {} did NOT satisfy `{} {}`.\n[MECHANICAL ROLLBACK: Repository reverted to {} to guarantee pristine state.]", actual_val, op, target, checkpoint_id),
                                    tokens_used: 0,
                                    status: ToolStatus::Success,
                                }
                            }
                        } else {
                            ToolResult {
                                task_id,
                                output: format!("Error: Could not parse target value '{}' as float.", parts[1]),
                                tokens_used: 0,
                                status: ToolStatus::Failed("Parse Error".into()),
                            }
                        }
                    } else {
                        // Could not parse stdout as float
                        ToolResult {
                            task_id,
                            output: format!("Error: Metric script output must be exactly one parseable float. Got: '{}'", stdout),
                            tokens_used: 0,
                            status: ToolStatus::Failed("Parse Error".into()),
                        }
                    }
                }
                Err(e) => {
                    let _ = checkpoint_mgr.rollback(&checkpoint_id).await;
                    ToolResult {
                        task_id,
                        output: format!("❌ Metric command execution error: {}. Rolled back to {}", e, checkpoint_id),
                        tokens_used: 0,
                        status: ToolStatus::Failed("Execution Error".into()),
                    }
                }
            }
        }
        _ => {
            ToolResult {
                task_id,
                output: format!("Unknown ratchet action: '{}'. Valid options: evaluate_test, evaluate_metric.", action),
                tokens_used: 0,
                status: ToolStatus::Failed("Unknown Action".into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ratchet_missing_args() {
        let r = execute_ratchet_tool("1".into(), "action:[evaluate_test]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Missing command:[...] or checkpoint_id:[...]"));
    }

    #[tokio::test]
    async fn test_ratchet_success() {
        // Evaluate essentially `true`
        let r = execute_ratchet_tool("1".into(), "action:[evaluate_test] command:[echo \"hello\"] checkpoint_id:[fake]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Success));
        assert!(r.output.contains("Ratchet SUCCESS"));
    }

    #[tokio::test]
    async fn test_ratchet_failure() {
        // Evaluate `false` should trigger rollback.
        // Rollback of non-existent "fake" checkpoint will fail, returning Failed status.
        let r = execute_ratchet_tool("1".into(), "action:[evaluate_test] command:[false] checkpoint_id:[fake]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_))); // Rollback failure returns Failed
        assert!(r.output.contains("Ratchet FAILED"));
        assert!(r.output.contains("CRITICAL ERROR")); // because rollback failed to find 'fake' checkpoint
    }

    #[tokio::test]
    async fn test_ratchet_metric_success() {
        // Output 0.4. Condition < 0.5. Should pass.
        let r = execute_ratchet_tool("1".into(), "action:[evaluate_metric] command:[echo \"0.4\"] checkpoint_id:[fake] condition:[< 0.5]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Success));
        assert!(r.output.contains("Ratchet SUCCESS"));
    }

    #[tokio::test]
    async fn test_ratchet_metric_failure() {
        // Output 0.6. Condition < 0.5. Should fail and rollback.
        let r = execute_ratchet_tool("1".into(), "action:[evaluate_metric] command:[echo \"0.6\"] checkpoint_id:[fake] condition:[< 0.5]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Success)); // Agent observes
        assert!(r.output.contains("Ratchet FAILED"));
    }
}
