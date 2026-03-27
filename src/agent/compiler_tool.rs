use crate::models::tool::{ToolResult, ToolStatus};
use crate::models::scope::Scope;
use tokio::sync::mpsc;
use crate::agent::preferences::extract_tag;

pub async fn execute_compiler(
    task_id: String,
    description: String,
    scope: Scope,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    let action = extract_tag(&description, "action:").unwrap_or_else(|| "system_recompile".to_string());
    
    macro_rules! telemetry {
        ($tx:expr, $msg:expr) => {
            if let Some(ref tx) = $tx {
                let _ = tx.send($msg).await;
            }
        };
    }

    if action == "system_recompile" {
        // ── SAFETY GATE: Run test suite BEFORE building ──────────────────
        // This catches logic bugs that compile but break behavior (e.g.
        // blocking .await on infinite tasks, incorrect concurrency, etc.)
        telemetry!(telemetry_tx, "  🧪 Running test suite before compilation (safety gate)...\n".into());

        match tokio::process::Command::new("cargo")
            .arg("test")
            .output()
            .await
        {
            Ok(test_output) => {
                if !test_output.status.success() {
                    let stderr = String::from_utf8_lossy(&test_output.stderr);
                    let stdout = String::from_utf8_lossy(&test_output.stdout);
                    telemetry!(telemetry_tx, "  ❌ Test suite FAILED — recompile ABORTED.\n".into());
                    return ToolResult {
                        task_id,
                        output: format!("RECOMPILE ABORTED: Test suite failed. Fix these failures before retrying.\n\nTest output:\n{}\n{}", stdout.chars().take(3000).collect::<String>(), stderr.chars().take(3000).collect::<String>()),
                        tokens_used: 0,
                        status: ToolStatus::Failed("Tests Failed".into()),
                    };
                }
                telemetry!(telemetry_tx, "  ✅ All tests passed. Proceeding to compilation.\n".into());
            }
            Err(e) => {
                telemetry!(telemetry_tx, "  ❌ Failed to run test suite — recompile ABORTED.\n".into());
                return ToolResult {
                    task_id,
                    output: format!("RECOMPILE ABORTED: Could not run cargo test: {}", e),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Test Runner Failure".into()),
                };
            }
        }

        telemetry!(telemetry_tx, "  ⚙️ Initiating native self-compilation array via `cargo build --release`...\n".into());
        telemetry!(telemetry_tx, "  🕒 Expected duration: 1-5 minutes depending on hardware limits.\n".into());

        // Log what's about to be compiled — human-readable change tracker
        {
            let diff = tokio::process::Command::new("git")
                .args(["diff", "--stat", "HEAD"])
                .output().await;
            let log = tokio::process::Command::new("git")
                .args(["log", "--oneline", "-5"])
                .output().await;

            let diff_text = diff.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
            let log_text = log.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();

            let explanation = if diff_text.trim().is_empty() {
                "The system rebuilt itself from unchanged source code (verification test).".to_string()
            } else {
                format!("The system rebuilt itself after detecting code changes. {} file(s) modified.", diff_text.lines().count().saturating_sub(1))
            };

            let entry = format!(
                "\n## Recompile — {}\n\n**Code changes since last commit:**\n{}\n\n**Recent commits:**\n{}\n\n**What this means:** {}\n\n---\n",
                chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
                if diff_text.trim().is_empty() { "None — recompiling identical code.".into() } else { diff_text },
                log_text.trim(),
                explanation,
            );

            let log_path = std::path::PathBuf::from("memory/core/recompile_log.md");
            let existing = tokio::fs::read_to_string(&log_path).await.unwrap_or_else(|_| "# Self-Recompilation Log\n".to_string());
            let _ = tokio::fs::write(&log_path, format!("{}{}", existing, entry)).await;
            tracing::info!("[UPGRADE_DAEMON] Recompile changelog written to memory/core/recompile_log.md");
        }

        match tokio::process::Command::new("cargo")
            .arg("build")
            .arg("--release")
            .output()
            .await 
        {
            Ok(output) => {
                let _stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if output.status.success() {
                    telemetry!(telemetry_tx, "  ✅ Compilation Successful! Preparing binary hot-swap.\n".into());
                    
                    // Copy the binary explicitly to avoid lock conflicts during the shell script swap
                    let _ = std::fs::copy("target/release/HIVE", "HIVE_next");
                    
                    // Save resume state so Apis continues where she left off after restart
                    let resume = serde_json::json!({
                        "scope": scope,
                        "message": "System recompile completed successfully. I have been upgraded and restarted. Resuming operations. Confirm to the user that the upgrade was successful."
                    });
                    let _ = std::fs::create_dir_all("memory/core");
                    let _ = std::fs::write("memory/core/resume.json", serde_json::to_string_pretty(&resume).unwrap_or_default());
                    tracing::info!("[UPGRADE_DAEMON] Resume state saved to memory/core/resume.json");
                    
                    telemetry!(telemetry_tx, "  🔄 Engaging detached upgrade script and gracefully exiting physical process...\n".into());
                    
                    // Spawn script fully detached
                    let _ = tokio::process::Command::new("bash")
                        .arg("upgrade.sh")
                        .spawn();
                    
                    // Allow any pending replies, telemetry, and disk flushes to complete
                    // before killing the process. Without this, concurrent tasks like
                    // reply_to_request get killed mid-flight.
                    tracing::warn!("[UPGRADE_DAEMON] Flushing pending operations before exit (5s grace)...");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    
                    // Terminate the host natively
                    std::process::exit(0);
                } else {
                    telemetry!(telemetry_tx, "  ❌ Compilation blocked by Rust compiler errors.\n".into());
                    return ToolResult { 
                        task_id, 
                        output: format!("Fatal compiler error:\n{}", stderr), 
                        tokens_used: 0, 
                        status: ToolStatus::Failed("Compiler Error".into()) 
                    };
                }
            }
            Err(e) => {
                return ToolResult { 
                    task_id, 
                    output: format!("Failed to invoke cargo binaries globally: {}", e), 
                    tokens_used: 0, 
                    status: ToolStatus::Failed("Spawn Failure".into()) 
                };
            }
        }
    }

    ToolResult {
        task_id,
        output: "Error: Unrecognized tool intent action.".into(),
        tokens_used: 0,
        status: ToolStatus::Failed("Bad Action".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_bad_action() {
        let scope = Scope::Private { user_id: "test".into() };
        let r = execute_compiler("1".into(), "action:[explode]".into(), scope, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Unrecognized"));
    }
}
