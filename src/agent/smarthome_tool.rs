use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;
use crate::agent::preferences::extract_tag;
use serde_json::json;
use reqwest::Client;

pub async fn execute_smarthome(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    let action = extract_tag(&description, "action:").unwrap_or_else(|| "smart_home".to_string());
    
    macro_rules! telemetry {
        ($tx:expr, $msg:expr) => {
            if let Some(ref tx) = $tx {
                let _ = tx.send($msg).await;
            }
        };
    }

    if action == "smart_home" {
        telemetry!(telemetry_tx, "  → Mapping spatial network parameters...\n".into());
        
        let device = extract_tag(&description, "device:").unwrap_or_default();
        let state = extract_tag(&description, "state:").unwrap_or_default();

        if device.is_empty() || state.is_empty() {
            return ToolResult { task_id, output: "Error: Missing 'device:' or 'state:' structural flags.".into(), tokens_used: 0, status: ToolStatus::Failed("Missing Params".into()) };
        }

        let base_url = std::env::var("SMART_HOME_URL").unwrap_or_default();
        let auth_token = std::env::var("SMART_HOME_TOKEN").unwrap_or_default();

        if base_url.is_empty() {
            telemetry!(telemetry_tx, "  ⚠️ Warning: `SMART_HOME_URL` is empty. Emulating physical local deployment.\n".into());
            return ToolResult { 
                 task_id, 
                 output: format!("Simulated smart networking endpoint call for {} => {}", device, state), 
                 tokens_used: 0, 
                 status: ToolStatus::Success 
            };
        }

        // Generic webhook formatting structurally
        let payload = json!({
            "device": device,
            "state": state
        });

        telemetry!(telemetry_tx, format!("  → Bridging physical REST HTTP POST to {}...\n", base_url));

        let client = Client::new();
        let mut req = client.post(&base_url).json(&payload);

        if !auth_token.is_empty() {
            req = req.bearer_auth(auth_token);
        }

        match req.send().await {
            Ok(res) => {
                if res.status().is_success() {
                    telemetry!(telemetry_tx, "  ✅ Spatial environment altered physically.\n".into());
                    return ToolResult { task_id, output: format!("Successfully mapped state `{}` to device `{}`", state, device), tokens_used: 0, status: ToolStatus::Success };
                } else {
                    let err_text = res.text().await.unwrap_or_else(|_| "Unknown spatial HTTP error mapping.".into());
                    return ToolResult { task_id, output: format!("Target network returned status: {:?}", err_text), tokens_used: 0, status: ToolStatus::Failed("HTTP Blocked".into()) };
                }
            }
            Err(e) => {
                return ToolResult { task_id, output: format!("Tokio Network exception mapping native REST: {}", e), tokens_used: 0, status: ToolStatus::Failed("Socket Failure".into()) };
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
    async fn test_missing_params() {
        let r = execute_smarthome("1".into(), "action:[smart_home]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_simulated_no_url() {
        unsafe { std::env::remove_var("SMART_HOME_URL"); }
        let r = execute_smarthome("1".into(), "action:[smart_home] device:[light_1] state:[on]".into(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Simulated"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_bad_action() {
        let r = execute_smarthome("1".into(), "action:[explode]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }
}
