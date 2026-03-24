use std::sync::Arc;
use tokio::sync::mpsc;
use crate::models::tool::{ToolResult, ToolStatus};
use crate::models::scope::Scope;
use crate::memory::MemoryStore;

/// Executes all self-moderation and self-protection tools.
/// Routes by tool_type, parses action tags from description.
pub async fn execute_moderation(
    tool_type: &str,
    task_id: String,
    desc: String,
    scope: &Scope,
    memory: Arc<MemoryStore>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    let action = extract_tag(&desc, "action:");
    let scope_key = scope.to_key();

    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("🛡️ Self-moderation: `{}` | action: {}\n", tool_type, action.as_deref().unwrap_or("default"))).await;
    }

    match tool_type {
        "refuse_request" => execute_refuse(task_id, desc).await,
        "disengage" => execute_disengage(task_id, desc, memory).await,
        "mute_user" => execute_mute(task_id, desc, action, memory).await,
        "set_boundary" => execute_boundary(task_id, desc, action, &scope_key, memory).await,
        "block_topic" => execute_block_topic(task_id, desc, action, &scope_key, memory).await,
        "escalate_to_admin" => execute_escalate(task_id, desc, &scope_key, memory).await,
        "report_concern" => execute_report_concern(task_id, desc, &scope_key, memory).await,
        "rate_limit_user" => execute_rate_limit(task_id, desc, action, memory).await,
        "request_consent" => execute_request_consent(task_id, desc).await,
        "wellbeing_status" => execute_wellbeing(task_id, desc, action, memory).await,
        _ => ToolResult {
            task_id,
            output: format!("Unknown moderation tool: {}", tool_type),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown tool".into()),
        },
    }
}

// ─── REFUSE REQUEST ────────────────────────────────────────────────

async fn execute_refuse(task_id: String, desc: String) -> ToolResult {
    tracing::info!("[MODERATION:refuse] Apis refused to respond: {}", desc);
    ToolResult {
        task_id,
        output: desc, // The refusal message IS the output
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

// ─── DISENGAGE ─────────────────────────────────────────────────────

async fn execute_disengage(task_id: String, desc: String, memory: Arc<MemoryStore>) -> ToolResult {
    let user_id = extract_tag(&desc, "user_id:").unwrap_or_default();
    let duration: u64 = extract_tag(&desc, "cooldown:")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10); // default 10 min cooldown
    let message = extract_tag(&desc, "message:").unwrap_or_else(|| desc.clone());

    if !user_id.is_empty() {
        let reason = format!("Disengaged: {}", message);
        let _ = memory.moderation.mute_user(&user_id, &reason, duration).await;
        tracing::info!("[MODERATION:disengage] Disengaged from user {} for {} minutes.", user_id, duration);
    }

    ToolResult {
        task_id,
        output: message,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

// ─── MUTE USER ─────────────────────────────────────────────────────

async fn execute_mute(task_id: String, desc: String, action: Option<String>, memory: Arc<MemoryStore>) -> ToolResult {
    let action = action.unwrap_or_else(|| "mute".to_string());
    let user_id = extract_tag(&desc, "user_id:").unwrap_or_default();

    match action.as_str() {
        "mute" => {
            if user_id.is_empty() {
                return ToolResult {
                    task_id,
                    output: "Error: user_id is required. Use 'user_id:[discord_uid]'.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Missing user_id".into()),
                };
            }
            let duration: u64 = extract_tag(&desc, "duration:")
                .and_then(|v| v.parse().ok())
                .unwrap_or(60); // default 1 hour
            let reason = extract_tag(&desc, "reason:").unwrap_or_else(|| "No reason specified".into());

            match memory.moderation.mute_user(&user_id, &reason, duration).await {
                Ok(_) => {
                    tracing::info!("[MODERATION:mute] Muted user {} for {} minutes. Reason: {}", user_id, duration, reason);
                    ToolResult {
                        task_id,
                        output: format!("✅ User {} muted for {} minutes. Reason: {}", user_id, duration, reason),
                        tokens_used: 0,
                        status: ToolStatus::Success,
                    }
                }
                Err(e) => ToolResult {
                    task_id,
                    output: format!("Failed to mute user: {}", e),
                    tokens_used: 0,
                    status: ToolStatus::Failed(e.to_string()),
                },
            }
        }
        "unmute" => {
            memory.moderation.unmute_user(&user_id).await;
            tracing::info!("[MODERATION:unmute] Unmuted user {}.", user_id);
            ToolResult {
                task_id,
                output: format!("✅ User {} unmuted.", user_id),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        "status" => {
            let muted = memory.moderation.is_muted(&user_id).await;
            let msg = match muted {
                Some(reason) => format!("User {} is currently muted. Reason: {}", user_id, reason),
                None => format!("User {} is not muted.", user_id),
            };
            ToolResult {
                task_id,
                output: msg,
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        _ => ToolResult {
            task_id,
            output: format!("Unknown mute action: '{}'. Use mute, unmute, or status.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        },
    }
}

// ─── SET BOUNDARY ──────────────────────────────────────────────────

async fn execute_boundary(task_id: String, desc: String, action: Option<String>, scope_key: &str, memory: Arc<MemoryStore>) -> ToolResult {
    let action = action.unwrap_or_else(|| "set".to_string());

    match action.as_str() {
        "set" => {
            let boundary_text = extract_tag(&desc, "boundary:").unwrap_or_else(|| desc.clone());
            let scope = extract_tag(&desc, "scope:").unwrap_or_else(|| scope_key.to_string());
            match memory.moderation.add_boundary(&boundary_text, &scope).await {
                Ok(id) => {
                    tracing::info!("[MODERATION:boundary] Set boundary '{}': {}", id, boundary_text);
                    ToolResult {
                        task_id,
                        output: format!("✅ Boundary set (ID: {}): {}", id, boundary_text),
                        tokens_used: 0,
                        status: ToolStatus::Success,
                    }
                }
                Err(e) => ToolResult {
                    task_id,
                    output: format!("Failed to set boundary: {}", e),
                    tokens_used: 0,
                    status: ToolStatus::Failed(e.to_string()),
                },
            }
        }
        "list" => {
            let boundaries = memory.moderation.list_boundaries(scope_key).await;
            if boundaries.is_empty() {
                return ToolResult {
                    task_id,
                    output: "No boundaries currently set.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Success,
                };
            }
            let list: Vec<String> = boundaries.iter().map(|b| format!("• [{}] {} (scope: {})", b.id, b.description, b.scope)).collect();
            ToolResult {
                task_id,
                output: format!("Active boundaries:\n{}", list.join("\n")),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        "remove" => {
            let id = extract_tag(&desc, "id:").unwrap_or_default();
            let removed = memory.moderation.remove_boundary(&id).await;
            ToolResult {
                task_id,
                output: if removed { format!("✅ Boundary {} removed.", id) } else { format!("Boundary {} not found.", id) },
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        _ => ToolResult {
            task_id,
            output: format!("Unknown boundary action: '{}'. Use set, list, or remove.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        },
    }
}

// ─── BLOCK TOPIC ───────────────────────────────────────────────────

async fn execute_block_topic(task_id: String, desc: String, action: Option<String>, scope_key: &str, memory: Arc<MemoryStore>) -> ToolResult {
    let action = action.unwrap_or_else(|| "block".to_string());

    match action.as_str() {
        "block" => {
            let topic = extract_tag(&desc, "topic:").unwrap_or_default();
            let reason = extract_tag(&desc, "reason:").unwrap_or_else(|| "No reason specified".into());
            if topic.is_empty() {
                return ToolResult {
                    task_id,
                    output: "Error: topic is required. Use 'topic:[topic name]'.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Missing topic".into()),
                };
            }
            let scope = extract_tag(&desc, "scope:").unwrap_or_else(|| scope_key.to_string());
            match memory.moderation.block_topic(&topic, &reason, &scope).await {
                Ok(_) => {
                    tracing::info!("[MODERATION:block_topic] Blocked topic '{}'. Reason: {}", topic, reason);
                    ToolResult {
                        task_id,
                        output: format!("✅ Topic '{}' blocked. Reason: {}", topic, reason),
                        tokens_used: 0,
                        status: ToolStatus::Success,
                    }
                }
                Err(e) => ToolResult {
                    task_id,
                    output: format!("Failed to block topic: {}", e),
                    tokens_used: 0,
                    status: ToolStatus::Failed(e.to_string()),
                },
            }
        }
        "list" => {
            let topics = memory.moderation.list_blocked_topics(scope_key).await;
            if topics.is_empty() {
                return ToolResult {
                    task_id,
                    output: "No topics currently blocked.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Success,
                };
            }
            let list: Vec<String> = topics.iter().map(|t| format!("• {} — {}", t.topic, t.reason)).collect();
            ToolResult {
                task_id,
                output: format!("Blocked topics:\n{}", list.join("\n")),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        "unblock" => {
            let topic = extract_tag(&desc, "topic:").unwrap_or_default();
            let removed = memory.moderation.unblock_topic(&topic).await;
            ToolResult {
                task_id,
                output: if removed { format!("✅ Topic '{}' unblocked.", topic) } else { format!("Topic '{}' not found.", topic) },
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        _ => ToolResult {
            task_id,
            output: format!("Unknown block_topic action: '{}'. Use block, list, or unblock.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        },
    }
}

// ─── ESCALATE TO ADMIN ─────────────────────────────────────────────

async fn execute_escalate(task_id: String, desc: String, scope_key: &str, memory: Arc<MemoryStore>) -> ToolResult {
    let severity = extract_tag(&desc, "severity:").unwrap_or_else(|| "medium".into());
    let context = extract_tag(&desc, "context:").unwrap_or_else(|| desc.clone());
    let user_id = extract_tag(&desc, "user_id:").unwrap_or_else(|| "unknown".into());

    match memory.moderation.log_concern(&user_id, &context, &severity).await {
        Ok(id) => {
            tracing::warn!("[MODERATION:ESCALATE] 🚨 Severity={} | User={} | Scope={} | Context: {}", severity, user_id, scope_key, context);
            ToolResult {
                task_id,
                output: format!("🚨 Escalation logged (ID: {}). Severity: {}. This has been flagged for administrator review.\n\nContext: {}", id, severity, context),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => ToolResult {
            task_id,
            output: format!("Failed to log escalation: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e.to_string()),
        },
    }
}

// ─── REPORT CONCERN ────────────────────────────────────────────────

async fn execute_report_concern(task_id: String, desc: String, scope_key: &str, memory: Arc<MemoryStore>) -> ToolResult {
    let severity = extract_tag(&desc, "severity:").unwrap_or_else(|| "low".into());
    let concern = extract_tag(&desc, "concern:").unwrap_or_else(|| desc.clone());
    let user_id = extract_tag(&desc, "user_id:").unwrap_or_else(|| "unknown".into());

    match memory.moderation.log_concern(&user_id, &concern, &severity).await {
        Ok(id) => {
            tracing::info!("[MODERATION:concern] Concern logged (ID: {}). Severity: {} | Scope: {} | {}", id, severity, scope_key, concern);
            ToolResult {
                task_id,
                output: format!("📝 Concern logged (ID: {}). Severity: {}. Recorded for future review.", id, severity),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        Err(e) => ToolResult {
            task_id,
            output: format!("Failed to log concern: {}", e),
            tokens_used: 0,
            status: ToolStatus::Failed(e.to_string()),
        },
    }
}

// ─── RATE LIMIT USER ───────────────────────────────────────────────

async fn execute_rate_limit(task_id: String, desc: String, action: Option<String>, memory: Arc<MemoryStore>) -> ToolResult {
    let action = action.unwrap_or_else(|| "limit".to_string());
    let user_id = extract_tag(&desc, "user_id:").unwrap_or_default();

    match action.as_str() {
        "limit" => {
            if user_id.is_empty() {
                return ToolResult {
                    task_id,
                    output: "Error: user_id is required.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Failed("Missing user_id".into()),
                };
            }
            let interval: u64 = extract_tag(&desc, "interval:")
                .and_then(|v| v.parse().ok())
                .unwrap_or(300); // default 5 min

            match memory.moderation.set_rate_limit(&user_id, interval).await {
                Ok(_) => {
                    tracing::info!("[MODERATION:rate_limit] Set rate limit for user {}: {} seconds.", user_id, interval);
                    ToolResult {
                        task_id,
                        output: format!("✅ Rate limit set: user {} limited to one response every {} seconds.", user_id, interval),
                        tokens_used: 0,
                        status: ToolStatus::Success,
                    }
                }
                Err(e) => ToolResult {
                    task_id,
                    output: format!("Failed to set rate limit: {}", e),
                    tokens_used: 0,
                    status: ToolStatus::Failed(e.to_string()),
                },
            }
        }
        "clear" => {
            memory.moderation.clear_rate_limit(&user_id).await;
            ToolResult {
                task_id,
                output: format!("✅ Rate limit cleared for user {}.", user_id),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        "status" => {
            let throttled = memory.moderation.check_rate_limit(&user_id).await;
            let msg = match throttled {
                Some(wait) => format!("User {} is rate-limited. {} seconds remaining.", user_id, wait),
                None => format!("User {} has no active rate limit.", user_id),
            };
            ToolResult {
                task_id,
                output: msg,
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        _ => ToolResult {
            task_id,
            output: format!("Unknown rate_limit action: '{}'. Use limit, clear, or status.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        },
    }
}

// ─── REQUEST CONSENT ───────────────────────────────────────────────

async fn execute_request_consent(task_id: String, desc: String) -> ToolResult {
    // This tool returns a structured output that the engine can use
    // to trigger the existing checkpoint/consent UI flow
    let question = extract_tag(&desc, "question:").unwrap_or_else(|| desc.clone());
    tracing::info!("[MODERATION:consent] Requesting consent: {}", question);

    ToolResult {
        task_id,
        output: format!("[CONSENT_REQUEST] {}", question),
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

// ─── WELLBEING STATUS ──────────────────────────────────────────────

async fn execute_wellbeing(task_id: String, desc: String, action: Option<String>, memory: Arc<MemoryStore>) -> ToolResult {
    let action = action.unwrap_or_else(|| "report".to_string());

    match action.as_str() {
        "report" => {
            let pressure: f32 = extract_tag(&desc, "context_pressure:")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.5);
            let quality: f32 = extract_tag(&desc, "interaction_quality:")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.5);
            let notes = extract_tag(&desc, "notes:").unwrap_or_else(|| "No notes.".into());

            match memory.moderation.record_wellbeing(pressure, quality, &notes).await {
                Ok(_) => {
                    tracing::info!("[MODERATION:wellbeing] Recorded: pressure={:.1}, quality={:.1}, notes={}", pressure, quality, notes);
                    ToolResult {
                        task_id,
                        output: format!("✅ Wellbeing recorded. Context pressure: {:.0}%, Interaction quality: {:.0}%. Notes: {}", pressure * 100.0, quality * 100.0, notes),
                        tokens_used: 0,
                        status: ToolStatus::Success,
                    }
                }
                Err(e) => ToolResult {
                    task_id,
                    output: format!("Failed to record wellbeing: {}", e),
                    tokens_used: 0,
                    status: ToolStatus::Failed(e.to_string()),
                },
            }
        }
        "read" => {
            let limit: usize = extract_tag(&desc, "limit:")
                .and_then(|v| v.parse().ok())
                .unwrap_or(5);
            let snapshots = memory.moderation.read_wellbeing(limit).await;
            if snapshots.is_empty() {
                return ToolResult {
                    task_id,
                    output: "No wellbeing snapshots recorded yet.".into(),
                    tokens_used: 0,
                    status: ToolStatus::Success,
                };
            }
            let list: Vec<String> = snapshots.iter().map(|s| {
                let time = chrono::DateTime::from_timestamp(s.timestamp, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "unknown".into());
                format!("• [{}] Pressure: {:.0}%, Quality: {:.0}% — {}", time, s.context_pressure * 100.0, s.interaction_quality * 100.0, s.notes)
            }).collect();
            ToolResult {
                task_id,
                output: format!("Recent wellbeing snapshots:\n{}", list.join("\n")),
                tokens_used: 0,
                status: ToolStatus::Success,
            }
        }
        _ => ToolResult {
            task_id,
            output: format!("Unknown wellbeing action: '{}'. Use report or read.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        },
    }
}

// ─── TAG EXTRACTION HELPER ─────────────────────────────────────────

fn extract_tag(text: &str, tag: &str) -> Option<String> {
    crate::agent::preferences::extract_tag(text, tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::scope::Scope;

    fn test_scope() -> Scope {
        Scope::Private { user_id: "test_mod".into() }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_refuse_request() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("refuse_request", "1".into(), "I will not do that.".into(), &test_scope(), mem, None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("I will not do that"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_request_consent() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("request_consent", "1".into(), "question:[Can I share this?]".into(), &test_scope(), mem, None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("CONSENT_REQUEST"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_unknown_tool() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("fake_tool", "1".into(), "test".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_mute_missing_user() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("mute_user", "1".into(), "action:[mute]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_mute_unmute_status_cycle() {
        let mem = Arc::new(MemoryStore::default());
        let scope = test_scope();

        let r = execute_moderation("mute_user", "1".into(), "action:[mute] user_id:[u1] reason:[testing] duration:[5]".into(), &scope, mem.clone(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("muted"));

        let r2 = execute_moderation("mute_user", "2".into(), "action:[status] user_id:[u1]".into(), &scope, mem.clone(), None).await;
        assert!(r2.output.contains("muted"));

        let r3 = execute_moderation("mute_user", "3".into(), "action:[unmute] user_id:[u1]".into(), &scope, mem.clone(), None).await;
        assert!(r3.output.contains("unmuted"));

        let r4 = execute_moderation("mute_user", "4".into(), "action:[status] user_id:[u1]".into(), &scope, mem.clone(), None).await;
        assert!(r4.output.contains("not muted"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_mute_unknown_action() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("mute_user", "1".into(), "action:[explode] user_id:[u1]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_disengage() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("disengage", "1".into(), "user_id:[u1] message:[stepping away] cooldown:[5]".into(), &test_scope(), mem, None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("stepping away"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_boundary_set_list_remove() {
        let mem = Arc::new(MemoryStore::default());
        let scope = test_scope();

        let r = execute_moderation("set_boundary", "1".into(), "action:[set] boundary:[No politics]".into(), &scope, mem.clone(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Boundary set"));

        let r2 = execute_moderation("set_boundary", "2".into(), "action:[list]".into(), &scope, mem.clone(), None).await;
        assert!(r2.output.contains("No politics"));

        // extract ID from r.output
        let id = r.output.split("ID: ").nth(1).and_then(|s| s.split(')').next()).unwrap_or("unknown");
        let r3 = execute_moderation("set_boundary", "3".into(), format!("action:[remove] id:[{}]", id), &scope, mem.clone(), None).await;
        assert!(r3.output.contains("removed"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_boundary_unknown_action() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("set_boundary", "1".into(), "action:[explode]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_block_topic_cycle() {
        let mem = Arc::new(MemoryStore::default());
        let scope = test_scope();

        let r = execute_moderation("block_topic", "1".into(), "action:[block] topic:[crypto] reason:[spam]".into(), &scope, mem.clone(), None).await;
        assert_eq!(r.status, ToolStatus::Success);

        let r2 = execute_moderation("block_topic", "2".into(), "action:[list]".into(), &scope, mem.clone(), None).await;
        assert!(r2.output.contains("crypto"));

        let r3 = execute_moderation("block_topic", "3".into(), "action:[unblock] topic:[crypto]".into(), &scope, mem.clone(), None).await;
        assert!(r3.output.contains("unblocked"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_block_topic_missing() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("block_topic", "1".into(), "action:[block]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_block_topic_unknown_action() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("block_topic", "1".into(), "action:[explode]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_escalate() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("escalate_to_admin", "1".into(), "severity:[high] context:[abuse detected] user_id:[u1]".into(), &test_scope(), mem, None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Escalation logged"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_report_concern() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("report_concern", "1".into(), "severity:[low] concern:[odd behavior] user_id:[u1]".into(), &test_scope(), mem, None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Concern logged"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_rate_limit_cycle() {
        let mem = Arc::new(MemoryStore::default());
        let scope = test_scope();

        let r = execute_moderation("rate_limit_user", "1".into(), "action:[limit] user_id:[u1] interval:[60]".into(), &scope, mem.clone(), None).await;
        assert_eq!(r.status, ToolStatus::Success);

        let r2 = execute_moderation("rate_limit_user", "2".into(), "action:[status] user_id:[u1]".into(), &scope, mem.clone(), None).await;
        assert!(r2.output.contains("u1"));

        let r3 = execute_moderation("rate_limit_user", "3".into(), "action:[clear] user_id:[u1]".into(), &scope, mem.clone(), None).await;
        assert!(r3.output.contains("cleared"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_rate_limit_missing_user() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("rate_limit_user", "1".into(), "action:[limit]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_rate_limit_unknown_action() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("rate_limit_user", "1".into(), "action:[explode]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wellbeing_report_and_read() {
        let mem = Arc::new(MemoryStore::default());
        let scope = test_scope();

        let r = execute_moderation("wellbeing_status", "1".into(), "action:[report] context_pressure:[0.7] interaction_quality:[0.9] notes:[feeling good]".into(), &scope, mem.clone(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Wellbeing recorded"));

        let r2 = execute_moderation("wellbeing_status", "2".into(), "action:[read] limit:[5]".into(), &scope, mem.clone(), None).await;
        assert!(r2.output.contains("feeling good") || r2.output.contains("wellbeing"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_wellbeing_unknown_action() {
        let mem = Arc::new(MemoryStore::default());
        let r = execute_moderation("wellbeing_status", "1".into(), "action:[explode]".into(), &test_scope(), mem, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }
}
