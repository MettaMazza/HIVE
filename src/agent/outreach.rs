use crate::models::tool::{ToolResult, ToolStatus};
use std::sync::Arc;
use tokio::sync::mpsc;
use crate::engine::outreach::{OutreachGate, OutreachFrequency, OutreachDelivery};
use crate::engine::inbox::{InboxManager, InboxPriority};
use crate::engine::drives::DriveSystem;

/// Parse a bracketed value from a description string.
/// e.g. parse_outreach_param("action:[send] user_id:[123]", "user_id") → Some("123")
fn parse_outreach_param(desc: &str, key: &str) -> Option<String> {
    let needle = format!("{}:[", key);
    let start = desc.find(&needle)? + needle.len();
    let end = desc[start..].find(']')? + start;
    Some(desc[start..end].trim().to_string())
}

pub async fn execute_outreach(
    task_id: String,
    description: String,
    outreach_gate: Option<Arc<OutreachGate>>,
    inbox: Option<Arc<InboxManager>>,
    drives: Option<Arc<DriveSystem>>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("📨 Outreach Agent Tool executing...\n".to_string()).await;
    }
    tracing::debug!("[AGENT:outreach] ▶ task_id={}", task_id);

    let gate = match outreach_gate {
        Some(g) => g,
        None => return ToolResult {
            task_id,
            output: "Outreach subsystem not initialised.".to_string(),
            tokens_used: 0,
            status: ToolStatus::Failed("OutreachGate missing".to_string()),
        },
    };

    let action = parse_outreach_param(&description, "action")
        .unwrap_or_default()
        .to_lowercase();

    match action.as_str() {
        "set_frequency" => {
            let uid = match parse_outreach_param(&description, "user_id") {
                Some(u) => u,
                None => return ToolResult { task_id, output: "❌ Missing user_id.".to_string(), tokens_used: 0, status: ToolStatus::Failed("bad params".to_string()) },
            };
            let freq_str = parse_outreach_param(&description, "frequency").unwrap_or_default();
            match freq_str.parse::<OutreachFrequency>() {
                Ok(freq) => {
                    let msg = gate.set_frequency(&uid, freq);
                    ToolResult { task_id, output: msg, tokens_used: 0, status: ToolStatus::Success }
                }
                Err(e) => ToolResult { task_id, output: format!("❌ Invalid frequency: {}", e), tokens_used: 0, status: ToolStatus::Failed("bad frequency".to_string()) },
            }
        }
        "set_delivery" => {
            let uid = match parse_outreach_param(&description, "user_id") {
                Some(u) => u,
                None => return ToolResult { task_id, output: "❌ Missing user_id.".to_string(), tokens_used: 0, status: ToolStatus::Failed("bad params".to_string()) },
            };
            let del_str = parse_outreach_param(&description, "delivery").unwrap_or_default();
            match del_str.parse::<OutreachDelivery>() {
                Ok(delivery) => {
                    let msg = gate.set_delivery(&uid, delivery);
                    ToolResult { task_id, output: msg, tokens_used: 0, status: ToolStatus::Success }
                }
                Err(e) => ToolResult { task_id, output: format!("❌ Invalid delivery: {}", e), tokens_used: 0, status: ToolStatus::Failed("bad delivery".to_string()) },
            }
        }
        "status" => {
            let uid = match parse_outreach_param(&description, "user_id") {
                Some(u) => u,
                None => return ToolResult { task_id, output: "❌ Missing user_id.".to_string(), tokens_used: 0, status: ToolStatus::Failed("bad params".to_string()) },
            };
            let s = gate.get_settings(&uid);
            let last = s.last_outreach.map(|t| t.to_rfc3339()).unwrap_or_else(|| "never".to_string());
            let output = format!(
                "📊 Outreach settings for {}:\n- Frequency: {:?}\n- Delivery: {}\n- Last outreach: {}\n- Relationship strength: {}/100\n- Interaction count: {}",
                uid, s.frequency, s.delivery, last, s.relationship_strength, s.interaction_count
            );
            ToolResult { task_id, output, tokens_used: 0, status: ToolStatus::Success }
        }
        "inbox_status" => {
            let uid = match parse_outreach_param(&description, "user_id") {
                Some(u) => u,
                None => return ToolResult { task_id, output: "❌ Missing user_id.".to_string(), tokens_used: 0, status: ToolStatus::Failed("bad params".to_string()) },
            };
            let mgr = match inbox {
                Some(m) => m,
                None => return ToolResult { task_id, output: "Inbox missing".to_string(), tokens_used: 0, status: ToolStatus::Failed("Inbox missing".to_string()) },
            };
            let summary = mgr.get_summary(&uid);
            ToolResult { task_id, output: summary, tokens_used: 0, status: ToolStatus::Success }
        }
        _ => {
            let uid = match parse_outreach_param(&description, "user_id") {
                Some(u) => u,
                None => return ToolResult { task_id, output: "❌ Missing user_id.".to_string(), tokens_used: 0, status: ToolStatus::Failed("bad params".to_string()) },
            };
            let content = match parse_outreach_param(&description, "content") {
                Some(c) if !c.trim().is_empty() => c,
                _ => return ToolResult { task_id, output: "❌ Missing content.".to_string(), tokens_used: 0, status: ToolStatus::Failed("bad params".to_string()) },
            };

            let (can, reason) = gate.can_outreach(&uid).await;
            if !can {
                return ToolResult { task_id, output: format!("🚫 Outreach blocked: {}", reason), tokens_used: 0, status: ToolStatus::Failed("outreach_blocked".to_string()) };
            }

            let delivery = gate.get_delivery(&uid);
            let mut results = Vec::new();

            if matches!(delivery, OutreachDelivery::Dm | OutreachDelivery::Both) {
                if let Some(ref mgr) = inbox {
                    if let Some(msg) = mgr.add_message(&uid, &content) {
                        if msg.priority == InboxPriority::Notify {
                            results.push("📬 dm:queued (notify priority)".to_string());
                        } else {
                            results.push("📬 dm:queued".to_string());
                        }
                    } else {
                        results.push("🔇 dm:muted".to_string());
                    }
                } else {
                    results.push("⚠️ dm:no_inbox".to_string());
                }
            }

            if matches!(delivery, OutreachDelivery::Public | OutreachDelivery::Both) {
                let channel_id = std::env::var("OUTREACH_CHANNEL_ID").unwrap_or_default();
                if channel_id.is_empty() {
                    results.push("⚠️ public:no_channel_id (set OUTREACH_CHANNEL_ID)".to_string());
                } else {
                    results.push(format!("📢 public:queued → <@{uid}> {content}"));
                }
            }

            if results.is_empty() {
                return ToolResult { task_id, output: "🚫 Delivery policy is 'none' — no message sent.".to_string(), tokens_used: 0, status: ToolStatus::Failed("none_policy".to_string()) };
            }

            gate.record_outreach(&uid);
            if let Some(d) = drives.as_ref() {
                d.modify_drive("social_connection", 10.0).await;
            }

            let summary = results.join(", ");
            ToolResult { task_id, output: format!("✅ Outreach to {} complete: {}\nContent: {}", uid, summary, content), tokens_used: 0, status: ToolStatus::Success }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_outreach_param() {
        let desc = "action:[send] user_id:[123]";
        assert_eq!(parse_outreach_param(desc, "action"), Some("send".to_string()));
        assert_eq!(parse_outreach_param(desc, "user_id"), Some("123".to_string()));
        assert_eq!(parse_outreach_param(desc, "missing"), None);
    }

    #[tokio::test]
    async fn test_execute_outreach_missing_gate() {
        let res = execute_outreach(
            "1".into(),
            "action:[send]".into(),
            None,
            None,
            None,
            None,
        ).await;
        assert_eq!(res.status, ToolStatus::Failed("OutreachGate missing".to_string()));
    }

    #[tokio::test]
    async fn test_execute_outreach_missing_params() {
        use crate::providers::MockProvider;
        use std::env;
        
        let dir = env::temp_dir().join(format!("hive_outreach_test_params_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut mock_provider = MockProvider::new();
        mock_provider.expect_generate().returning(|_, _, _, _, _, _| Ok("Mock".to_string()));
        let gate = Arc::new(OutreachGate::new(dir.to_str().unwrap(), Arc::new(mock_provider)));
        
        let res = execute_outreach(
            "2".into(),
            "action:[set_frequency]".into(), // missing user_id
            Some(gate.clone()),
            None,
            None,
            None,
        ).await;
        assert_eq!(res.status, ToolStatus::Failed("bad params".to_string()));

        let res2 = execute_outreach(
            "3".into(),
            "user_id:[123]".into(), // missing content
            Some(gate.clone()),
            None,
            None,
            None,
        ).await;
        assert_eq!(res2.status, ToolStatus::Failed("bad params".to_string()));
    }

    #[tokio::test]
    async fn test_execute_outreach_set_frequency() {
        use crate::providers::MockProvider;
        use std::env;
        let dir = env::temp_dir().join(format!("hive_outreach_test_freq_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut mock_provider = MockProvider::new();
        mock_provider.expect_generate().returning(|_, _, _, _, _, _| Ok("Mock".to_string()));
        let gate = Arc::new(OutreachGate::new(dir.to_str().unwrap(), Arc::new(mock_provider)));
        
        let res = execute_outreach(
            "4".into(),
            "action:[set_frequency] user_id:[user456] frequency:[high]".into(),
            Some(gate.clone()),
            None,
            None,
            None,
        ).await;
        
        assert_eq!(res.status, ToolStatus::Success);
        
        let verify = execute_outreach(
            "4v".into(),
            "action:[status] user_id:[user456]".into(),
            Some(gate.clone()),
            None,
            None,
            None,
        ).await;
        assert!(verify.output.contains("High"));
    }

    #[tokio::test]
    async fn test_execute_outreach_set_delivery() {
        use crate::providers::MockProvider;
        use std::env;
        let dir = env::temp_dir().join(format!("hive_outreach_test_deliv_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut mock_provider = MockProvider::new();
        mock_provider.expect_generate().returning(|_, _, _, _, _, _| Ok("Mock".to_string()));
        let gate = Arc::new(OutreachGate::new(dir.to_str().unwrap(), Arc::new(mock_provider)));
        
        let res = execute_outreach(
            "5".into(),
            "action:[set_delivery] user_id:[user789] delivery:[dm]".into(),
            Some(gate.clone()),
            None,
            None,
            None,
        ).await;
        
        assert_eq!(res.status, ToolStatus::Success);
    }

    #[tokio::test]
    async fn test_execute_outreach_send_muted() {
        use crate::providers::MockProvider;
        use std::env;
        let dir = env::temp_dir().join(format!("hive_outreach_test_mute_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut mock_provider = MockProvider::new();
        mock_provider.expect_generate().returning(|_, _, _, _, _, _| Ok("Mock".to_string()));
        let gate = Arc::new(OutreachGate::new(dir.to_str().unwrap(), Arc::new(mock_provider)));
        
        // Mute the user first
        let _ = execute_outreach(
            "6m".into(),
            "action:[set_delivery] user_id:[user999] delivery:[none]".into(),
            Some(gate.clone()),
            None,
            None,
            None,
        ).await;

        let res = execute_outreach(
            "6".into(),
            "action:[send] user_id:[user999] content:[hello]".into(),
            Some(gate.clone()),
            None,
            None,
            None,
        ).await;
        
        // can_outreach() catches delivery=none BEFORE the none_policy path
        assert_eq!(res.status, ToolStatus::Failed("outreach_blocked".to_string()));
    }

    #[tokio::test]
    async fn test_execute_outreach_inbox_status() {
        use crate::providers::MockProvider;
        use std::env;
        let dir = env::temp_dir().join(format!("hive_outreach_test_inbox_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut mock_provider = MockProvider::new();
        mock_provider.expect_generate().returning(|_, _, _, _, _, _| Ok("YES".to_string()));
        let gate = Arc::new(OutreachGate::new(dir.to_str().unwrap(), Arc::new(mock_provider)));
        let inbox = Arc::new(InboxManager::new(dir.to_str().unwrap()));
        
        // Push a DM
        let res = execute_outreach(
            "7".into(),
            "action:[send] user_id:[user111] content:[test message]".into(),
            Some(gate.clone()),
            Some(inbox.clone()),
            None,
            None,
        ).await;
        
        assert_eq!(res.status, ToolStatus::Success);
        
        let stat = execute_outreach(
            "8".into(),
            "action:[inbox_status] user_id:[user111]".into(),
            Some(gate.clone()),
            Some(inbox.clone()),
            None,
            None,
        ).await;
        assert!(stat.output.contains("1 unread message"));
    }
}
