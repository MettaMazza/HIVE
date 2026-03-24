use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;
use crate::agent::preferences::extract_tag;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlarmPayload {
    pub id: String,
    pub trigger_time: String,
    pub message: String,
    pub status: String,
}

pub async fn execute_calendar(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    let action = extract_tag(&description, "action:").unwrap_or_else(|| "set_alarm".to_string());
    
    macro_rules! telemetry {
        ($tx:expr, $msg:expr) => {
            if let Some(ref tx) = $tx {
                let _ = tx.send($msg).await;
            }
        };
    }

    if action == "set_alarm" {
        telemetry!(telemetry_tx, "  → Processing temporal alarm parsing array...\n".into());
        
        let time_str = extract_tag(&description, "time:").unwrap_or_default();
        let message = extract_tag(&description, "message:").unwrap_or_default();

        if time_str.is_empty() || message.is_empty() {
            return ToolResult { task_id, output: "Error: Missing 'time:' or 'message:' params.".into(), tokens_used: 0, status: ToolStatus::Failed("Missing Params".into()) };
        }

        // Extremely basic time offset parser (+Xm, +Xh, +Xd) natively or explicit ISO.
        let trigger_time: DateTime<Utc> = if time_str.starts_with("+") && time_str.ends_with("m") {
            let mins: i64 = time_str[1..time_str.len()-1].parse().unwrap_or(0);
            Utc::now() + Duration::minutes(mins)
        } else if time_str.starts_with("+") && time_str.ends_with("h") {
            let hrs: i64 = time_str[1..time_str.len()-1].parse().unwrap_or(0);
            Utc::now() + Duration::hours(hrs)
        } else if time_str.starts_with("+") && time_str.ends_with("d") {
            let days: i64 = time_str[1..time_str.len()-1].parse().unwrap_or(0);
            Utc::now() + Duration::days(days)
        } else {
            match DateTime::parse_from_rfc3339(&time_str) {
                Ok(dt) => dt.with_timezone(&Utc),
                Err(_) => {
                    return ToolResult { task_id, output: "Error: Invalid time format. Ensure +5m, +1h, +2d or full ISO RFC3339".into(), tokens_used: 0, status: ToolStatus::Failed("Bad Parse".into()) };
                }
            }
        };

        telemetry!(telemetry_tx, format!("  → Extracted trigger time: {}\n", trigger_time.to_rfc3339()));

        let alarm = AlarmPayload {
            id: uuid::Uuid::new_v4().to_string(),
            trigger_time: trigger_time.to_rfc3339(),
            message: message.clone(),
            status: "pending".into(),
        };

        // Thread safe JSON read/write bounds natively
        let alarms_path = Path::new("memory").join("alarms.json");
        let _ = std::fs::create_dir_all("memory");
        
        let mut alarms: Vec<AlarmPayload> = match tokio::fs::read_to_string(&alarms_path).await {
            Ok(json_str) => serde_json::from_str(&json_str).unwrap_or_else(|_| vec![]),
            Err(_) => vec![],
        };

        alarms.push(alarm);
        
        if let Ok(json_str) = serde_json::to_string_pretty(&alarms) {
            if let Err(e) = tokio::fs::write(&alarms_path, json_str).await {
                return ToolResult { task_id, output: format!("FS write lock failure: {}", e), tokens_used: 0, status: ToolStatus::Failed("FS Lock".into()) };
            }
            telemetry!(telemetry_tx, "  ✅ Temporal hook successfully lodged into Chronos.\n".into());
            return ToolResult { task_id, output: format!("Alarm successfully set for {}.", trigger_time.to_rfc3339()), tokens_used: 0, status: ToolStatus::Success };
        } else {
            return ToolResult { task_id, output: "Error serializing JSON payload string natively.".into(), tokens_used: 0, status: ToolStatus::Failed("Serialization".into()) };
        }
    }

    ToolResult {
        task_id,
        output: "Error: Unrecognized action natively.".into(),
        tokens_used: 0,
        status: ToolStatus::Failed("Bad Action".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_alarm_minutes() {
        let r = execute_calendar("1".into(), "action:[set_alarm] time:[+5m] message:[test alarm]".into(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Alarm successfully set"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_alarm_hours() {
        let r = execute_calendar("1".into(), "action:[set_alarm] time:[+1h] message:[hourly check]".into(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_alarm_days() {
        let r = execute_calendar("1".into(), "action:[set_alarm] time:[+2d] message:[daily check]".into(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_alarm_missing_params() {
        let r = execute_calendar("1".into(), "action:[set_alarm]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_alarm_bad_time() {
        let r = execute_calendar("1".into(), "action:[set_alarm] time:[not_a_time] message:[x]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_bad_action() {
        let r = execute_calendar("1".into(), "action:[explode] time:[+5m] message:[x]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }
}
