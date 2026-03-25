use crate::models::tool::{ToolResult, ToolStatus};
use tokio::sync::mpsc;
use crate::agent::preferences::extract_tag;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── ALARMS ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlarmPayload {
    pub id: String,
    pub trigger_time: String,
    pub message: String,
    pub status: String,
}

const ALARMS_PATH: &str = "memory/alarms.json";

// ── EVENTS ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start_time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub recurring: Option<String>, // daily, weekly, monthly, or None
    pub created_at: String,
}

const EVENTS_PATH: &str = "memory/calendar_events.json";

async fn load_events() -> Vec<CalendarEvent> {
    match tokio::fs::read_to_string(EVENTS_PATH).await {
        Ok(json_str) => serde_json::from_str(&json_str).unwrap_or_else(|_| vec![]),
        Err(_) => vec![],
    }
}

async fn save_events(events: &[CalendarEvent]) -> Result<(), String> {
    let _ = std::fs::create_dir_all("memory");
    let json = serde_json::to_string_pretty(events).map_err(|e| e.to_string())?;
    tokio::fs::write(EVENTS_PATH, json).await.map_err(|e| e.to_string())
}

/// Parse time from relative (+5m, +1h, +2d) or ISO8601
fn parse_time(time_str: &str) -> Result<DateTime<Utc>, String> {
    if time_str.starts_with("+") && time_str.ends_with("m") {
        let mins: i64 = time_str[1..time_str.len()-1].parse().map_err(|_| "Bad minutes".to_string())?;
        Ok(Utc::now() + Duration::minutes(mins))
    } else if time_str.starts_with("+") && time_str.ends_with("h") {
        let hrs: i64 = time_str[1..time_str.len()-1].parse().map_err(|_| "Bad hours".to_string())?;
        Ok(Utc::now() + Duration::hours(hrs))
    } else if time_str.starts_with("+") && time_str.ends_with("d") {
        let days: i64 = time_str[1..time_str.len()-1].parse().map_err(|_| "Bad days".to_string())?;
        Ok(Utc::now() + Duration::days(days))
    } else {
        DateTime::parse_from_rfc3339(time_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| "Invalid time format. Use +5m, +1h, +2d or full ISO RFC3339.".to_string())
    }
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

    match action.as_str() {
        // ── ALARM: Set ──
        "set_alarm" => {
            telemetry!(telemetry_tx, "  → Processing temporal alarm...\n".into());
            
            let time_str = extract_tag(&description, "time:").unwrap_or_default();
            let message = extract_tag(&description, "message:").unwrap_or_default();

            if time_str.is_empty() || message.is_empty() {
                return ToolResult { task_id, output: "Error: Missing 'time:' or 'message:' params.".into(), tokens_used: 0, status: ToolStatus::Failed("Missing Params".into()) };
            }

            let trigger_time = match parse_time(&time_str) {
                Ok(t) => t,
                Err(e) => return ToolResult { task_id, output: format!("Error: {}", e), tokens_used: 0, status: ToolStatus::Failed("Bad Parse".into()) },
            };

            telemetry!(telemetry_tx, format!("  → Trigger time: {}\n", trigger_time.to_rfc3339()));

            let alarm = AlarmPayload {
                id: uuid::Uuid::new_v4().to_string(),
                trigger_time: trigger_time.to_rfc3339(),
                message: message.clone(),
                status: "pending".into(),
            };

            let alarms_path = Path::new(ALARMS_PATH);
            let _ = std::fs::create_dir_all("memory");
            
            let mut alarms: Vec<AlarmPayload> = match tokio::fs::read_to_string(&alarms_path).await {
                Ok(json_str) => serde_json::from_str(&json_str).unwrap_or_else(|_| vec![]),
                Err(_) => vec![],
            };

            alarms.push(alarm);
            
            if let Ok(json_str) = serde_json::to_string_pretty(&alarms) {
                if let Err(e) = tokio::fs::write(&alarms_path, json_str).await {
                    return ToolResult { task_id, output: format!("FS write failure: {}", e), tokens_used: 0, status: ToolStatus::Failed("FS Error".into()) };
                }
                telemetry!(telemetry_tx, "  ✅ Alarm set.\n".into());
                return ToolResult { task_id, output: format!("Alarm set for {}.", trigger_time.to_rfc3339()), tokens_used: 0, status: ToolStatus::Success };
            } else {
                return ToolResult { task_id, output: "Error serializing JSON.".into(), tokens_used: 0, status: ToolStatus::Failed("Serialization".into()) };
            }
        }

        // ── ALARM: List ──
        "list_alarms" => {
            let alarms_path = Path::new(ALARMS_PATH);
            let alarms: Vec<AlarmPayload> = match tokio::fs::read_to_string(&alarms_path).await {
                Ok(json_str) => serde_json::from_str(&json_str).unwrap_or_else(|_| vec![]),
                Err(_) => vec![],
            };

            if alarms.is_empty() {
                return ToolResult { task_id, output: "No alarms set.".into(), tokens_used: 0, status: ToolStatus::Success };
            }

            let mut output = format!("⏰ {} alarm(s):\n", alarms.len());
            for a in &alarms {
                output.push_str(&format!("\n• [{}] {} — {} ({})", a.id[..8].to_string(), a.trigger_time, a.message, a.status));
            }
            ToolResult { task_id, output, tokens_used: 0, status: ToolStatus::Success }
        }

        // ── EVENT: Create ──
        "create_event" => {
            let title = match extract_tag(&description, "title:") {
                Some(t) if !t.is_empty() => t,
                _ => return ToolResult { task_id, output: "Error: 'title:' is required.".into(), tokens_used: 0, status: ToolStatus::Failed("Missing title".into()) },
            };

            let start_str = match extract_tag(&description, "start:") {
                Some(s) if !s.is_empty() => s,
                _ => return ToolResult { task_id, output: "Error: 'start:' time is required.".into(), tokens_used: 0, status: ToolStatus::Failed("Missing start".into()) },
            };

            let start_time = match parse_time(&start_str) {
                Ok(t) => t,
                Err(e) => return ToolResult { task_id, output: format!("Error parsing start time: {}", e), tokens_used: 0, status: ToolStatus::Failed("Bad Parse".into()) },
            };

            let end_time = if let Some(end_str) = extract_tag(&description, "end:") {
                match parse_time(&end_str) {
                    Ok(t) => Some(t.to_rfc3339()),
                    Err(e) => return ToolResult { task_id, output: format!("Error parsing end time: {}", e), tokens_used: 0, status: ToolStatus::Failed("Bad Parse".into()) },
                }
            } else {
                None
            };

            telemetry!(telemetry_tx, format!("  → Creating event '{}' at {}...\n", title, start_time.to_rfc3339()));

            let event = CalendarEvent {
                id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
                title: title.clone(),
                start_time: start_time.to_rfc3339(),
                end_time,
                location: extract_tag(&description, "location:"),
                description: extract_tag(&description, "details:"),
                recurring: extract_tag(&description, "recurring:"),
                created_at: Utc::now().to_rfc3339(),
            };

            let mut events = load_events().await;
            events.push(event.clone());
            if let Err(e) = save_events(&events).await {
                return ToolResult { task_id, output: format!("Error saving: {}", e), tokens_used: 0, status: ToolStatus::Failed("FS Error".into()) };
            }

            telemetry!(telemetry_tx, "  ✅ Event created.\n".into());
            ToolResult {
                task_id,
                output: format!("Event '{}' created (id: {}) at {}. Total events: {}", title, event.id, event.start_time, events.len()),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }

        // ── EVENT: List ──
        "list_events" => {
            telemetry!(telemetry_tx, "  → Loading calendar events...\n".into());
            let events = load_events().await;
            if events.is_empty() {
                return ToolResult { task_id, output: "No calendar events.".into(), tokens_used: 0, status: ToolStatus::Success };
            }

            let mut output = format!("📅 {} event(s):\n", events.len());
            for e in &events {
                output.push_str(&format!("\n• {} (id: {}) — {}", e.title, e.id, e.start_time));
                if let Some(ref end) = e.end_time { output.push_str(&format!(" → {}", end)); }
                if let Some(ref loc) = e.location { output.push_str(&format!(" @ {}", loc)); }
                if let Some(ref rec) = e.recurring { output.push_str(&format!(" 🔁 {}", rec)); }
                if let Some(ref desc) = e.description { output.push_str(&format!("\n  {}", desc)); }
            }
            ToolResult { task_id, output, tokens_used: 0, status: ToolStatus::Success }
        }

        // ── EVENT: Delete ──
        "delete_event" => {
            let id = match extract_tag(&description, "id:") {
                Some(i) if !i.is_empty() => i,
                _ => return ToolResult { task_id, output: "Error: 'id:' is required.".into(), tokens_used: 0, status: ToolStatus::Failed("Missing id".into()) },
            };

            telemetry!(telemetry_tx, format!("  → Deleting event {}...\n", id));
            let mut events = load_events().await;
            let before = events.len();
            events.retain(|e| e.id != id);

            if events.len() == before {
                return ToolResult { task_id, output: format!("No event with id '{}'.", id), tokens_used: 0, status: ToolStatus::Failed("Not found".into()) };
            }

            if let Err(e) = save_events(&events).await {
                return ToolResult { task_id, output: format!("Error saving: {}", e), tokens_used: 0, status: ToolStatus::Failed("FS Error".into()) };
            }

            telemetry!(telemetry_tx, "  ✅ Event deleted.\n".into());
            ToolResult { task_id, output: format!("Event '{}' deleted. {} events remaining.", id, events.len()), tokens_used: 0, status: ToolStatus::Success }
        }

        _ => ToolResult {
            task_id,
            output: format!("Error: Unknown action '{}'. Use: set_alarm, list_alarms, create_event, list_events, delete_event.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Bad Action".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_alarm_minutes() {
        let r = execute_calendar("1".into(), "action:[set_alarm] time:[+5m] message:[test alarm]".into(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Alarm"));
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
    async fn test_create_event() {
        let r = execute_calendar("1".into(), "action:[create_event] title:[Team Meeting] start:[+1h] location:[Office]".into(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Team Meeting"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_event_missing_title() {
        let r = execute_calendar("1".into(), "action:[create_event] start:[+1h]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_list_events() {
        let r = execute_calendar("1".into(), "action:[list_events]".into(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_delete_event_nonexistent() {
        let r = execute_calendar("1".into(), "action:[delete_event] id:[fake123]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_bad_action() {
        let r = execute_calendar("1".into(), "action:[explode] time:[+5m] message:[x]".into(), None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }
}
