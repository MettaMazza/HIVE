use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;


use crate::models::message::Response;
use crate::models::scope::Scope;
use crate::platforms::Platform;

/// Loads the last N autonomy session summaries from activity.jsonl so the LLM
/// knows what it already did and won't repeat the same actions.
pub(crate) async fn load_recent_autonomy_sessions(max_entries: usize) -> String {
    let path = std::path::Path::new("memory/autonomy/activity.jsonl");
    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let entries: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    if entries.is_empty() {
        return String::new();
    }

    let total_sessions = entries.len();
    let start = if entries.len() > max_entries { entries.len() - max_entries } else { 0 };
    let recent = &entries[start..];

    let mut dedup_block = format!(
        "\n🚫 **PREVIOUS AUTONOMY SESSIONS — DO NOT REPEAT THESE ({} total sessions completed so far):**\n",
        total_sessions
    );
    dedup_block.push_str("You have ALREADY done the following in recent sessions. Do NOT do the same things again. Explore NEW territory.\n\n");

    for (i, line) in recent.iter().enumerate() {
        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
            let summary = entry.get("summary").and_then(|v| v.as_str()).unwrap_or("(no summary)");
            let tools = entry.get("tools_used").and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|t| t.as_str()).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            // Include enough of the summary for the agent to know what was actually done
            let short_summary: String = summary.chars().take(5000).collect();
            dedup_block.push_str(&format!(
                "--- Session {} ---\nTOOLS ALREADY USED: {}\n{}\n\n",
                i + 1, tools, short_summary
            ));
        }
    }

    dedup_block.push_str("\nYou MUST do something DIFFERENT this session. Use different tools, explore different areas, or work on something new.\n");
    dedup_block
}

/// Loads recent self-recompilation history from recompile_log.md so the LLM
/// knows its own upgrade track record and won't redundantly test system_recompile.
pub(crate) async fn load_recompile_history(max_entries: usize) -> String {
    let path = std::path::Path::new("memory/core/recompile_log.md");
    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let entries: Vec<&str> = content.split("\n---\n")
        .filter(|s| s.contains("## Recompile"))
        .collect();
    if entries.is_empty() {
        return String::new();
    }

    let start = if entries.len() > max_entries { entries.len() - max_entries } else { 0 };
    let recent = &entries[start..];

    let mut block = format!(
        "\n🔧 **SELF-RECOMPILATION HISTORY ({} total recompiles):**\n\
        You have ALREADY successfully recompiled yourself {} times. \
        The system_recompile tool is CONFIRMED WORKING. \
        Do NOT test it again unless you have actual code changes to deploy.\n\n",
        entries.len(), entries.len()
    );
    for entry in recent {
        if let Some(date_line) = entry.lines().find(|l| l.starts_with("## Recompile")) {
            block.push_str(&format!("• {}\n", date_line.trim_start_matches("## ")));
        }
    }
    block
}

/// Format elapsed seconds as a human-readable string.
pub fn format_elapsed(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        format!("{}s", elapsed_secs)
    } else {
        format!("{:.1}m", elapsed_secs as f64 / 60.0)
    }
}



/// Cleans up telemetry text for Discord embeds.
/// If the text contains a JSON task plan, extracts tool_type + description
/// into human-readable lines. Reasoning text and tool updates pass through unchanged.
/// Robust: if anything looks malformed, the original text passes through rather than being hidden.
pub(crate) fn humanize_telemetry(text: &str) -> String {
    // Find the start of the JSON block
    let brace_pos = match text.find('{') {
        Some(pos) => pos,
        None => return text.to_string(), // No JSON, return as-is
    };
    
    let reasoning = &text[..brace_pos];
    let from_brace = &text[brace_pos..];
    
    // Find the matching closing brace using depth counting,
    // skipping braces inside JSON string literals for robustness
    let mut depth = 0;
    let mut json_end: Option<usize> = None;
    let mut in_string = false;
    let mut prev_was_escape = false;
    for (i, ch) in from_brace.char_indices() {
        if in_string {
            if ch == '\\' && !prev_was_escape {
                prev_was_escape = true;
                continue;
            }
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = false;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    json_end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    
    // If braces never balanced, this is incomplete JSON (still streaming)
    // Show reasoning + planning indicator, don't show raw JSON
    let json_end = match json_end {
        Some(end) => end,
        None => {
            let trimmed = reasoning.trim();
            return if trimmed.is_empty() {
                "⏳ Planning...".to_string()
            } else {
                format!("{}\n\n⏳ Planning...", trimmed)
            };
        }
    };
    
    let json_block = &from_brace[..json_end];
    let after_json = from_brace[json_end..].trim_start();
    
    // Try to parse the JSON block and extract task descriptions
    let filtered_json = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_block) {
        if let Some(tasks) = parsed.get("tasks").and_then(|t| t.as_array()) {
            let lines: Vec<String> = tasks.iter().map(|task| {
                let tool = task.get("tool_type").and_then(|v| v.as_str()).unwrap_or("unknown");
                let desc = task.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let truncated = if desc.len() > 80 { format!("{}…", &desc[..80]) } else { desc.to_string() };
                format!("🔧 {}: {}", tool, truncated)
            }).collect();
            if lines.is_empty() { String::new() } else { lines.join("\n") }
        } else {
            String::new() // Valid JSON but no "tasks" key — just hide it
        }
    } else {
        String::new() // Matched braces but invalid JSON — just hide it
    };
    
    // Reassemble: reasoning + filtered JSON summary + tool updates (everything after JSON)
    let mut parts: Vec<&str> = Vec::new();
    let trimmed_reasoning = reasoning.trim();
    if !trimmed_reasoning.is_empty() {
        parts.push(trimmed_reasoning);
    }
    if !filtered_json.is_empty() {
        parts.push(&filtered_json);
    }
    if !after_json.is_empty() {
        parts.push(after_json);
    }
    if parts.is_empty() {
        "⏳ Processing...".to_string()
    } else {
        parts.join("\n\n")
    }
}

pub(crate) fn spawn_telemetry_receiver(
    platforms: Arc<HashMap<String, Box<dyn Platform>>>,
    platform_id: String,
    scope: Scope,
) -> mpsc::Sender<String> {
    let (tx, mut rx) = mpsc::channel::<String>(50);

    let platform_id_log = platform_id.clone();
    tokio::spawn(async move {
        let start_time = tokio::time::Instant::now();
        tracing::debug!("[TELEMETRY:RX] 🎧 Receiver spawned for platform='{}'", platform_id_log);
        let debounce_ms = 800;
        let mut has_update = false;
        let mut buffered_thought = String::new();

        let quirks: &[&str] = &[
            "🍯 *Synthesizing nectar...*",
            "🧠 *Consulting the hive mind...*",
            "🌼 *Detecting ultraviolet floral patterns...*",
            "🐝 *Did you know? Bees can recognize human faces.*",
            "🍯 *Fun fact: 3000-year-old honey found in Egyptian tombs is still edible.*",
            "🧠 *Aligning artificial synapses...*",
            "🐝 *Warming up the flight muscles (130 beats per second)...*",
            "🌼 *Scouting the digital meadow...*",
            "🤖 *Calculating probability of robot uprising (currently 0%)...*",
            "📡 *Pinging the Apis mothership...*",
            "🐝 *Fun fact: A bee's brain is the size of a sesame seed but does a trillion ops/sec.*",
            "🍯 *Viscosity calculations in progress...*",
            "🐝 *Did you know? To make one pound of honey, bees must visit 2 million flowers.*",
            "🌼 *Performing the waggle dance to broadcast data coordinates...*"
        ];

        let mut last_send = tokio::time::Instant::now();
        loop {
            let recv_result = tokio::time::timeout(
                tokio::time::Duration::from_millis(100), // Check frequently
                rx.recv()
            ).await;

            match recv_result {
                Ok(Some(chunk)) => {
                    if !has_update {
                        tracing::debug!("[TELEMETRY:RX] 📥 First chunk received for platform='{}' ({}ms elapsed)", platform_id, start_time.elapsed().as_millis());
                    }
                    buffered_thought.push_str(&chunk);
                    has_update = true;
                }
                Ok(None) => {
                    tracing::debug!("[TELEMETRY:RX] 🔌 Channel closed for platform='{}'", platform_id);
                    break;
                }
                Err(_) => { // timeout
                    // Just fall through to check elapsed time
                }
            }
            
            // Dispatch update if we have new data and the debounce period has passed
            if has_update && last_send.elapsed().as_millis() >= debounce_ms {
                let elapsed_str = format_elapsed(start_time.elapsed().as_secs());
                let current_quirk = quirks[start_time.elapsed().as_millis() as usize % quirks.len()];
                let thought_len = buffered_thought.len();
                let humanized = humanize_telemetry(&buffered_thought);
                // Discord embed description limit is 4096 chars; keep under with room for the quirk prefix
                let max_len = 3800;
                let display_text = if humanized.len() > max_len {
                    // Find a valid char boundary to avoid panicking on multi-byte UTF-8 (emojis etc.)
                    let mut start = humanized.len() - max_len;
                    while !humanized.is_char_boundary(start) && start < humanized.len() { start += 1; }
                    format!("…{}", &humanized[start..])
                } else {
                    humanized
                };
                let status = format!("{} ({})\n\n{}", current_quirk, elapsed_str, display_text);
                let update_res = Response {
                    platform: platform_id.clone(),
                    target_scope: scope.clone(),
                    text: status,
                    is_telemetry: true,
                };
                let platform_key = update_res.platform.split(':').next().unwrap_or("");
                if let Some(platform) = platforms.get(platform_key) {
                    tracing::debug!("[TELEMETRY:RX] 📤 Sending telemetry update to '{}' (thought_len={}, elapsed={})", platform_id, thought_len, elapsed_str);
                    if let Err(e) = platform.send(update_res).await {
                        tracing::warn!("[TELEMETRY:RX] ❌ platform.send failed: {}", e);
                    }
                } else {
                    tracing::warn!("[TELEMETRY:RX] ❌ Platform '{}' not found in platforms map!", platform_key);
                }
                has_update = false;
                last_send = tokio::time::Instant::now();
            }
        }

        // Final message: show completion WITH the full chain-of-thought preserved
        let elapsed_str = format_elapsed(start_time.elapsed().as_secs());
        let humanized = humanize_telemetry(&buffered_thought);
        let max_len = 3800;
        let display_text = if humanized.len() > max_len {
            let mut start = humanized.len() - max_len;
            while !humanized.is_char_boundary(start) && start < humanized.len() { start += 1; }
            format!("…{}", &humanized[start..])
        } else {
            humanized
        };
        let status = format!("✅ Complete ({})\n\n{}", elapsed_str, display_text);
        let update_res = Response {
            platform: platform_id.clone(),
            target_scope: scope.clone(),
            text: status,
            is_telemetry: true,
        };
        if let Some(platform) = platforms.get(update_res.platform.split(':').next().unwrap_or("")) {
            let _ = platform.send(update_res).await;
        }
    });

    tx
}
