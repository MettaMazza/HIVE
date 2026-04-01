use crate::models::tool::{ToolResult, ToolStatus};
use crate::agent::preferences::extract_tag;
use tokio::sync::mpsc;

/// Native Git Tool — first-class git operations with structured output.
/// Replaces delegating git operations through run_bash_command.
pub async fn execute_git_tool(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("🔀 Native Git Tool executing...\n".to_string()).await;
    }

    let action = extract_tag(&description, "action:").unwrap_or_default();

    if action.is_empty() {
        return ToolResult {
            task_id,
            output: "Error: Missing action:[...]. Valid actions: status, diff, commit, log, blame, branch, branches, stash, stash_pop, checkout.".into(),
            tokens_used: 0,
            status: ToolStatus::Failed("Missing action".into()),
        };
    }

    let result = match action.as_str() {
        "status" => {
            run_git(&["status", "--porcelain", "--branch"]).await
        }
        "diff" => {
            let staged = extract_tag(&description, "staged:").unwrap_or_default();
            if staged == "true" {
                run_git(&["diff", "--staged"]).await
            } else {
                run_git(&["diff"]).await
            }
        }
        "commit" => {
            let message = extract_tag(&description, "message:").unwrap_or_default();
            if message.is_empty() {
                Err("Error: Missing message:[...] for commit.".into())
            } else {
                // Stage all changes first
                let stage_result = run_git(&["add", "-A"]).await;
                if let Err(e) = stage_result {
                    return ToolResult {
                        task_id,
                        output: format!("Failed to stage: {}", e),
                        tokens_used: 0,
                        status: ToolStatus::Failed("Stage failed".into()),
                    };
                }
                run_git(&["commit", "-m", &message]).await
            }
        }
        "log" => {
            let limit = extract_tag(&description, "limit:")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(10);
            let limit_str = format!("-{}", limit);
            run_git(&["log", "--oneline", "--decorate", &limit_str]).await
        }
        "blame" => {
            let path = extract_tag(&description, "path:").unwrap_or_default();
            let line = extract_tag(&description, "line:")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1);
            if path.is_empty() {
                Err("Error: Missing path:[...] for blame.".into())
            } else {
                let range = format!("{},+10", line);
                run_git(&["blame", &path, "-L", &range]).await
            }
        }
        "branch" => {
            let name = extract_tag(&description, "name:").unwrap_or_default();
            if name.is_empty() {
                Err("Error: Missing name:[...] for branch creation.".into())
            } else {
                run_git(&["checkout", "-b", &name]).await
            }
        }
        "branches" => {
            run_git(&["branch", "-a", "--no-color"]).await
        }
        "stash" => {
            let message = extract_tag(&description, "message:")
                .unwrap_or_else(|| "auto-stash".to_string());
            run_git(&["stash", "push", "-m", &message]).await
        }
        "stash_pop" => {
            run_git(&["stash", "pop"]).await
        }
        "checkout" => {
            let target = extract_tag(&description, "target:").unwrap_or_default();
            if target.is_empty() {
                Err("Error: Missing target:[...] for checkout (branch name or commit hash).".into())
            } else {
                run_git(&["checkout", &target]).await
            }
        }
        other => {
            Err(format!("Unknown git action: '{}'. Valid actions: status, diff, commit, log, blame, branch, branches, stash, stash_pop, checkout.", other))
        }
    };

    match result {
        Ok(output) => ToolResult {
            task_id,
            output,
            tokens_used: 0,
            status: ToolStatus::Success,
        },
        Err(error) => ToolResult {
            task_id,
            output: error.clone(),
            tokens_used: 0,
            status: ToolStatus::Failed(error),
        },
    }
}

/// Execute a git command and return stdout on success or stderr on failure.
async fn run_git(args: &[&str]) -> Result<String, String> {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new("git")
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .output()
    ).await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                let combined = if stdout.trim().is_empty() && !stderr.trim().is_empty() {
                    // Some git commands (like checkout) write to stderr even on success
                    stderr.trim().to_string()
                } else if stdout.trim().is_empty() {
                    "Command succeeded with no output.".to_string()
                } else {
                    stdout.trim().to_string()
                };
                Ok(combined)
            } else {
                Err(format!("git {} failed:\n{}{}", args[0], stdout, stderr))
            }
        }
        Ok(Err(e)) => Err(format!("Failed to execute git: {}", e)),
        Err(_) => Err(format!("git {} timed out after 30 seconds", args[0])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_git_missing_action() {
        let r = execute_git_tool("1".into(), "".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Missing action"));
    }

    #[tokio::test]
    async fn test_git_unknown_action() {
        let r = execute_git_tool("1".into(), "action:[explode]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Unknown git action"));
    }

    #[tokio::test]
    async fn test_git_status() {
        // This test runs in the HIVE repo itself, so git status should work
        let r = execute_git_tool("1".into(), "action:[status]".into(), None).await;
        // May succeed or fail depending on git being available, but shouldn't panic
        // In CI/CD or Docker without git, this will fail gracefully
        if r.status == ToolStatus::Success {
            // The output should contain branch info
            assert!(!r.output.is_empty());
        }
    }

    #[tokio::test]
    async fn test_git_log() {
        let r = execute_git_tool("1".into(), "action:[log] limit:[3]".into(), None).await;
        if r.status == ToolStatus::Success {
            assert!(!r.output.is_empty());
        }
    }

    #[tokio::test]
    async fn test_git_branches() {
        let r = execute_git_tool("1".into(), "action:[branches]".into(), None).await;
        if r.status == ToolStatus::Success {
            assert!(!r.output.is_empty());
        }
    }

    #[tokio::test]
    async fn test_git_diff() {
        let r = execute_git_tool("1".into(), "action:[diff]".into(), None).await;
        // Diff may return empty if no changes, which is still success
        if r.status == ToolStatus::Success {
            assert!(!r.output.is_empty());
        }
    }

    #[tokio::test]
    async fn test_git_commit_missing_message() {
        let r = execute_git_tool("1".into(), "action:[commit]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Missing message"));
    }

    #[tokio::test]
    async fn test_git_blame_missing_path() {
        let r = execute_git_tool("1".into(), "action:[blame]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Missing path"));
    }

    #[tokio::test]
    async fn test_git_branch_missing_name() {
        let r = execute_git_tool("1".into(), "action:[branch]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Missing name"));
    }

    #[tokio::test]
    async fn test_git_checkout_missing_target() {
        let r = execute_git_tool("1".into(), "action:[checkout]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Missing target"));
    }

    #[tokio::test]
    async fn test_git_diff_staged() {
        let r = execute_git_tool("1".into(), "action:[diff] staged:[true]".into(), None).await;
        // Should not panic regardless of git state
        if r.status == ToolStatus::Success {
            assert!(!r.output.is_empty());
        }
    }
}
