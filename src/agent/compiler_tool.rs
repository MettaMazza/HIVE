use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;
use crate::agent::preferences::extract_tag;

pub async fn execute_compiler(
    task_id: String,
    description: String,
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
        telemetry!(telemetry_tx, "  ⚙️ Initiating native self-compilation array via `cargo build --release`...\n".into());
        telemetry!(telemetry_tx, "  🕒 Expected duration: 1-5 minutes depending on hardware limits.\n".into());

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
                    
                    telemetry!(telemetry_tx, "  🔄 Engaging detached upgrade script and gracefully exiting physical process...\n".into());
                    
                    // Spawn script fully detached
                    let _ = tokio::process::Command::new("bash")
                        .arg("upgrade.sh")
                        .spawn();
                        
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
        let r = execute_compiler("1".into(), "action:[explode]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Unrecognized"));
    }
}
