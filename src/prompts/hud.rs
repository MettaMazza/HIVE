use crate::models::scope::Scope;
use crate::memory::MemoryStore;
use std::sync::Arc;

pub struct HudData {
    pub temporal_metrics: String,
    pub timeline_narratives: String,
    pub active_scope: String,
    pub working_memory_load: String,
    pub scratchpad_content: Option<String>,
    pub room_roster: Option<String>,
    pub relevant_lessons: Option<String>,
    pub relevant_routines: Option<String>,
    pub global_activity: String,
    pub kg_snapshot: String,
    pub tape_focus: String,
    pub user_preferences: String,
    pub system_logs: String,
    pub recent_reasoning_traces: String,
    pub swarm_status: String,
    pub pending_alarms: String,
}

impl HudData {
    pub async fn build(scope: &Scope, memory_store: Arc<MemoryStore>) -> Self {
        let temporal_metrics = memory_store.temporal.read().await.get_formatted_hud();
        
        let mut timeline_narratives = String::new();
        let timelines = memory_store.timelines.read(scope).await;
        if let Some(t) = &timelines.last_50_turns {
            timeline_narratives.push_str(&format!("Recent Narrative (50 Turns):\n{}\n\n", t.narrative));
        }
        if let Some(t) = &timelines.last_24_hours {
            timeline_narratives.push_str(&format!("Daily Summary:\n{}\n\n", t.narrative));
        }
        if let Some(t) = &timelines.lifetime {
            timeline_narratives.push_str(&format!("Lifetime Context:\n{}\n\n", t.narrative));
        }
        
        let mut global_activity = String::new();
        {
            let stream = memory_store.activity_stream.read().await;
            if stream.is_empty() {
                global_activity.push_str("No recent global activity detected.");
            } else {
                for line in stream.iter() {
                    global_activity.push_str(&format!("{}\n", line));
                }
            }
        }
        
        let mut kg_snapshot = String::new();
        let syn = &memory_store.synaptic;
        
        let nodes = syn.get_recent_nodes(5).await;
        kg_snapshot.push_str("Recent Synaptic Nodes Added:\n");
        if nodes.is_empty() {
            kg_snapshot.push_str("  (None)\n");
        } else {
            for (n, layer) in nodes {
                kg_snapshot.push_str(&format!("  - {} [{}]\n", n, layer));
            }
        }

        let beliefs = syn.get_beliefs(5).await;
        kg_snapshot.push_str("\nCore Semantic Beliefs:\n");
        if beliefs.is_empty() {
            kg_snapshot.push_str("  (None)\n");
        } else {
            for b in beliefs {
                kg_snapshot.push_str(&format!("  - {}\n", b));
            }
        }

        let rels = syn.get_recent_relationships(5).await;
        kg_snapshot.push_str("\nRecent Knowledge Edges:\n");
        if rels.is_empty() {
            kg_snapshot.push_str("  (None)\n");
        } else {
            for (src, rel, tgt) in rels {
                kg_snapshot.push_str(&format!("  - ({}) -[{}]-> ({})\n", src, rel, tgt));
            }
        }
        
        let active_scope = match scope {
            Scope::Public {
                channel_id,
                user_id,
            } => format!(
                "Public (Broadcast Channel: {} | Active User ID: {})",
                channel_id, user_id
            ),
            Scope::Private { user_id } => format!("Private (User ID: {})", user_id),
        };

        let working_memory_load = format!(
            "{} / {}",
            memory_store.working.current_tokens().await,
            memory_store.working.max_tokens()
        );

        let content = memory_store.scratch.read(scope).await;
        let scratchpad_content = if !content.trim().is_empty() {
            Some(content.trim().to_string())
        } else {
            None
        };

        let room_roster = if let Scope::Public { .. } = scope {
            let history = memory_store.get_working_history(scope).await;
            let mut participants: std::collections::HashSet<String> = std::collections::HashSet::new();
            for event in history {
                participants.insert(event.author_name.clone());
            }
            if participants.is_empty() {
                None
            } else {
                let mut p: Vec<String> = participants.into_iter().collect();
                p.sort();
                Some(p.join(", "))
            }
        } else { None };

        // --- LESSON EXTRACTION ---
        let mut relevant_lessons = None;
        let history = memory_store.working.get_history(scope).await;
        
        // Grab up to last 3 user messages to build a keyword context pool
        let mut recent_words = std::collections::HashSet::new();
        let recent_user_msgs: Vec<_> = history.iter()
            .filter(|e| e.author_id != "system")
            .rev()
            .take(3)
            .collect();
            
        for msg in &recent_user_msgs {
            for word in msg.content.split_whitespace() {
                let clean_word: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
                if clean_word.len() > 2 {
                    recent_words.insert(clean_word.to_lowercase());
                }
            }
        }
        
        let all_lessons = memory_store.lessons.read_lessons(scope).await;
        let mut matched_lessons = Vec::new();
        
        for lesson in all_lessons {
            if lesson.keywords.iter().any(|kw| recent_words.contains(&kw.to_lowercase())) {
                matched_lessons.push(lesson);
            }
            if matched_lessons.len() >= 3 {
                break;
            }
        }
        
        if !matched_lessons.is_empty() {
            let mut out = String::new();
            for (i, lesson) in matched_lessons.iter().enumerate() {
                out.push_str(&format!("{}. [{:.1} conf] {}\n", i + 1, lesson.confidence, lesson.text));
            }
            relevant_lessons = Some(out);
        }

        // --- ROUTINE MATCHER ---
        let mut relevant_routines = None;
        let mut routines_dir = memory_store.working.get_memory_dir();
        if let Scope::Public { channel_id, .. } = scope {
            routines_dir.push(format!("public_{}", channel_id));
            routines_dir.push("system");
        } else if let Scope::Private { user_id } = scope {
            routines_dir.push(format!("private_{}", user_id));
        }
        routines_dir.push("routines");

        let mut matched_routines = Vec::new();
        if let Ok(mut rd) = tokio::fs::read_dir(&routines_dir).await {
            while let Ok(Some(entry)) = rd.next_entry().await {
                if let Ok(name) = entry.file_name().into_string()
                    && name.ends_with(".md")
                        && let Ok(content) = tokio::fs::read_to_string(entry.path()).await {
                            let content_lower = content.to_lowercase();
                            // If ANY recent user word over 4 characters is in the routine body, flag it.
                            if recent_words.iter().filter(|w| w.len() > 4).any(|kw| content_lower.contains(kw)) {
                                matched_routines.push(name);
                            }
                        }
            }
        }

        if !matched_routines.is_empty() {
            relevant_routines = Some(format!("[SYSTEM NOTICE] Matching Declarative Routines detected: {}. Use the `manage_routine` drone with `action:read` to review the methodology before proceeding.", matched_routines.join(", ")));
        }

        let prefs = memory_store.preferences.read(scope).await;
        let prefs_text = prefs.format_for_prompt();

        let grid_guard = memory_store.turing_grid.lock().await;
        let tape_focus = format!(
            "[Turing Grid — 3D Computation Engine] Cursor: [{},{},{}] | Active Cells: {} | Labels: {}",
            grid_guard.cursor.0, grid_guard.cursor.1, grid_guard.cursor.2, grid_guard.cells.len(), grid_guard.labels.len()
        );

        // Inject 20-line system log tail
        let mut system_logs = String::new();
        if let Ok(content) = tokio::fs::read_to_string("logs/hive.log").await {
            let lines: Vec<&str> = content.lines().collect();
            let tail = if lines.len() > 20 {
                &lines[lines.len() - 20..]
            } else {
                &lines[..]
            };
            system_logs.push_str(&tail.join("\n"));
        } else {
            system_logs.push_str("No recent system logs available or log file missing.");
        }

        let mut recent_reasoning_traces = String::new();
        let mut trace_count = 0;
        let mut extracted_traces = Vec::new();
        
        // history is already available from line 123: let history = memory_store.working.get_history(scope).await;
        for event in history.iter().rev() {
            if event.author_name == "Apis (Internal Timeline)" {
                // Hard cap the trace to 1500 characters to prevent Master Gauntlet blobs 
                // (e.g., 75KB) from severely spiking LLM context windows and latency.
                const TRACE_CAP: usize = 1500;
                let mut trace_text = event.content.clone();
                if trace_text.len() > TRACE_CAP {
                    let truncated: String = trace_text.chars().take(TRACE_CAP).collect();
                    trace_text = format!("{}...\n\n[...Trace truncated internally to preserve HUD responsiveness. Original trace was {} bytes.]", truncated, event.content.len());
                }

                extracted_traces.push(trace_text);
                trace_count += 1;
                if trace_count >= 3 {
                    break;
                }
            }
        }
        
        if extracted_traces.is_empty() {
            recent_reasoning_traces.push_str("No recent reasoning traces available in this active scope.");
        } else {
            // Reverse again to chronological order
            extracted_traces.reverse();
            recent_reasoning_traces.push_str(&extracted_traces.join("\n\n---\n\n"));
        }

        // --- CALENDAR ALARMS PULL ---
        let mut pending_alarms = String::new();
        if let Ok(contents) = tokio::fs::read_to_string("memory/alarms.json").await {
            if let Ok(alarms) = serde_json::from_str::<serde_json::Value>(&contents) {
                if let Some(arr) = alarms.as_array() {
                    let mut found = false;
                    for a in arr {
                        if a.get("status").and_then(|s| s.as_str()) == Some("pending") {
                            found = true;
                            let time = a.get("trigger_time").and_then(|t| t.as_str()).unwrap_or("Unknown");
                            let msg = a.get("message").and_then(|m| m.as_str()).unwrap_or("");
                            pending_alarms.push_str(&format!("  - [{}] {}\n", time, msg));
                        }
                    }
                    if !found { pending_alarms.push_str("No pending chronological alarms."); }
                } else {
                    pending_alarms.push_str("Alarms payload format invalid.");
                }
            } else {
                pending_alarms.push_str("No pending chronological alarms.");
            }
        } else {
            pending_alarms.push_str("No pending chronological alarms (file missing).");
        }

        HudData {
            temporal_metrics,
            timeline_narratives,
            active_scope,
            working_memory_load,
            scratchpad_content,
            room_roster,
            relevant_lessons,
            relevant_routines,
            global_activity,
            kg_snapshot,
            tape_focus,
            user_preferences: prefs_text,
            system_logs,
            recent_reasoning_traces,
            swarm_status: crate::agent::lifecycle::AgentLifecycle::get().format_hud_line(),
            pending_alarms,
        }
    }
}

pub fn format_hud(data: &HudData) -> String {
    let mut sections = Vec::new();

    sections.push("## Apis HUD (Live System Context)".to_string());
    
    sections.push(data.temporal_metrics.clone());
    sections.push("".to_string());
    
    sections.push("### Active Chronological Alarms".to_string());
    sections.push(data.pending_alarms.clone());
    sections.push("".to_string());

    if !data.timeline_narratives.is_empty() {
        sections.push(data.timeline_narratives.clone());
    }

    sections.push("### Active Scope & Room Roster".to_string());
    sections.push(format!("Active Context Boundary: {}", data.active_scope));
    sections.push("You are strictly bound by this scope context.".to_string());
    sections.push("".to_string());

    sections.push("### Working Memory Status (Tier 1)".to_string());
    sections.push(format!("Current Load: {} tokens", data.working_memory_load));
    sections.push("*(Note: Autosave function will trigger near capacity)*".to_string());
    sections.push("".to_string());

    if let Some(ref content) = data.scratchpad_content {
        sections.push("### Scratchpad (Tier 5)".to_string());
        sections.push(format!("Current workspace contents:\n{}", content));
        sections.push("".to_string());
    }

    if let Some(ref roster) = data.room_roster {
        sections.push("### Room Roster".to_string());
        sections.push(format!("Current participants: {}", roster));
        sections.push("".to_string());
    }

    sections.push("### Theory of Mind & User Preferences".to_string());
    sections.push(data.user_preferences.clone());
    sections.push("".to_string());

    sections.push("### Global Activity Stream (Anonymized Tail)".to_string());
    sections.push(data.global_activity.clone());
    sections.push("".to_string());

    sections.push("### Knowledge Graph (Synaptic Live Overview)".to_string());
    sections.push(data.kg_snapshot.clone());
    sections.push("".to_string());

    sections.push("### Turing Grid (3D Computation Engine)".to_string());
    sections.push(data.tape_focus.clone());
    sections.push("".to_string());

    sections.push("### Swarm Status".to_string());
    sections.push(data.swarm_status.clone());
    sections.push("".to_string());

    sections.push("### Recent Reasoning Traces (Last 3)".to_string());
    sections.push("You can introspect further into the past using the `review_reasoning` tool.".to_string());
    sections.push(data.recent_reasoning_traces.clone());
    sections.push("".to_string());

    sections.push("### Recent System Logs (Tail)".to_string());
    sections.push("You can introspect further using the `read_logs` tool.".to_string());
    sections.push(data.system_logs.clone());

    sections.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::scope::Scope;
    use crate::memory::MemoryStore;
    use std::sync::Arc;


    #[tokio::test]
    async fn test_hud_build_private_scope() {
        let mem = Arc::new(MemoryStore::default());
        let scope = Scope::Private { user_id: "user123".to_string() };
        let hud = HudData::build(&scope, mem).await;
        
        assert_eq!(hud.active_scope, "Private (User ID: user123)");
        assert!(hud.working_memory_load.ends_with(" / 256000"));
        assert_eq!(hud.room_roster, None); // Private scope should have no room roster
    }

    #[tokio::test]
    async fn test_hud_build_public_scope() {
        let unique_id = format!("hud_pub_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let pub_scope = Scope::Public { channel_id: unique_id.clone(), user_id: "test_user".to_string() };
        
        let test_dir = std::env::temp_dir().join(unique_id.clone());
        let mem = Arc::new(MemoryStore::new(Some(test_dir)));
        let _ = mem.scratch.write(&pub_scope, "First note").await;
        let _ = mem.scratch.append(&pub_scope, "Second note").await;
        
        mem.add_event(crate::models::message::Event {
            platform: "test".into(),
            scope: pub_scope.clone(),
            author_name: "Alice".into(),
            author_id: "alice1".into(),
            content: "Ping".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await;
        
        mem.add_event(crate::models::message::Event {
            platform: "test".into(),
            scope: pub_scope.clone(),
            author_name: "Bob".into(),
            author_id: "bob1".into(),
            content: "Pong".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await;

        let hud = HudData::build(&pub_scope, mem).await;

        assert_eq!(hud.active_scope, format!("Public (Broadcast Channel: {} | Active User ID: test_user)", unique_id));
        assert!(hud.working_memory_load.ends_with(" / 256000"));
        assert_eq!(hud.scratchpad_content, Some("First noteSecond note\n".to_string()));
        assert_eq!(hud.room_roster, Some("Alice, Bob".to_string()));
    }

    #[tokio::test]
    async fn test_format_hud_with_scratchpad() {
        let data = HudData {
            temporal_metrics: "Current System Time: 2026-03-07T12:00:00Z".to_string(),
            timeline_narratives: String::new(),
            active_scope: "Public (Broadcast Channel)".to_string(),
            working_memory_load: "100 / 256000".to_string(),
            scratchpad_content: Some("Test note".to_string()),
            room_roster: None,
            relevant_lessons: None,
            relevant_routines: None,
            global_activity: String::new(),
            kg_snapshot: String::new(),
            tape_focus: String::new(),
            user_preferences: String::new(),
            system_logs: String::new(),
            recent_reasoning_traces: String::new(),
            swarm_status: String::new(),
            pending_alarms: String::new(),
        };
        
        let output = format_hud(&data);
        assert!(output.contains("Current System Time: 2026-03-07T12:00:00Z"));
        assert!(output.contains("Active Context Boundary: Public (Broadcast Channel)"));
        assert!(output.contains("100 / 256000"));
        assert!(output.contains("Test note"));
    }

    #[tokio::test]
    async fn test_hud_build_with_scratchpad_populated() {
        let unique_id = format!("hud_populated_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let test_dir = std::env::temp_dir().join(unique_id);
        let memory_store = Arc::new(MemoryStore::new(Some(test_dir)));
        let scope = Scope::Public { channel_id: "main".into(), user_id: "u1".into() };
        memory_store.scratch.write(&scope, "System instructions testing...").await.unwrap();

        let hud = HudData::build(&scope, memory_store).await;
        assert_eq!(hud.scratchpad_content, Some("System instructions testing...".to_string()));
    }
}
