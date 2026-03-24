use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;
use headless_chrome::{Browser, LaunchOptions};

pub async fn execute_visualizer(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    let action = crate::agent::preferences::extract_tag(&description, "action:").unwrap_or_else(|| "take_snapshot".to_string());
    
    macro_rules! telemetry {
        ($tx:expr, $msg:expr) => {
            if let Some(ref tx) = $tx {
                let _ = tx.send($msg).await;
            }
        };
    }

    if action == "take_snapshot" {
        telemetry!(telemetry_tx, format!("  → Spinning up Headless Chrome for physical Dashboard Snapshot...\n"));
        
        // Ensure image dump directory exists
        let screenshot_dir = std::path::Path::new("memory/cached_images");
        if !screenshot_dir.exists() {
            let _ = tokio::fs::create_dir_all(screenshot_dir).await;
        }

        // Browser operations are heavily blocking; spin off thread
        let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
            let opts = LaunchOptions {
                window_size: Some((1400, 900)),
                ..Default::default()
            };
            
            let browser = Browser::new(opts).map_err(|e| format!("Browser launch failed: {:?}", e))?;
            let tab = browser.new_tab().map_err(|e| format!("Failed to open tab: {:?}", e))?;
            
            // Navigate to local dashboard
            tab.navigate_to("http://127.0.0.1:3030").map_err(|e| format!("Navigation failed: {:?}", e))?;
            tab.wait_until_navigated().map_err(|e| format!("Wait for navigation failed: {:?}", e))?;

            // We must intentionally block for 3-5 seconds to let Vis.js physically compute layout gravity algorithms natively in DOM.
            std::thread::sleep(std::time::Duration::from_secs(4));

            // Capture Screenshot natively
            let png_data = tab.capture_screenshot(
                headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                None,
                None,
                true
            ).map_err(|e| format!("Screenshot capture failed: {:?}", e))?;

            // Save absolute path for Discord attachment payload
            let cwd = std::env::current_dir().unwrap_or_default();
            let abs_path = cwd.join("memory/cached_images/brain_snapshot.png");
            
            std::fs::write(&abs_path, png_data).map_err(|e| format!("Failed to save physical snapshot: {}", e))?;
            
            Ok(abs_path.to_string_lossy().into_owned())
        }).await;

        match result {
            Ok(Ok(filepath)) => {
                telemetry!(telemetry_tx, format!("  ✅ Biological memory snapshot captured securely.\n"));
                
                // Formulate the precise syntax the Discord platform expects natively to attach the image.
                return ToolResult {
                     task_id,
                     output: format!("[ATTACH_IMAGE]({})", filepath),
                     tokens_used: 0,
                     status: ToolStatus::Success,
                };
            }
            Ok(Err(e)) => {
                telemetry!(telemetry_tx, format!("  ❌ Snapshot physical DOM Error: {}\n", e));
                return ToolResult { task_id, output: e, tokens_used: 0, status: ToolStatus::Failed("Browser Crash".into()) };
            }
            Err(e) => {
                return ToolResult { task_id, output: format!("Tokio Panic: {:?}", e), tokens_used: 0, status: ToolStatus::Failed("Thread Panic".into()) };
            }
        }
    }

    ToolResult {
        task_id,
        output: "Error: Unrecognized action in visualizer tool.".into(),
        tokens_used: 0,
        status: ToolStatus::Failed("Bad Action".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_bad_action() {
        let r = execute_visualizer("1".into(), "action:[explode]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
        assert!(r.output.contains("Unrecognized"));
    }
}
