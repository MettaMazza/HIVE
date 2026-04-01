use crate::models::tool::{ToolResult, ToolStatus};
use crate::memory::MemoryStore;
use crate::models::scope::Scope;
use crate::agent::preferences::extract_tag;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::io::{AsyncBufReadExt, BufReader};

pub async fn execute_search_timeline(
    task_id: String,
    description: String,
    _memory: Arc<MemoryStore>,
    telemetry_tx: Option<mpsc::Sender<String>>,
    current_scope: &Scope,
    drives: Option<Arc<crate::engine::drives::DriveSystem>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("🧠 Timeline Drone executing...\n".to_string()).await;
    }
    tracing::debug!("[AGENT:timeline] ▶ task_id={}", task_id);
    
    // Partial uncertainty reduction: contextual retrieval helps settle entropy
    if let Some(d) = drives {
        d.modify_drive("uncertainty", -10.0).await;
    }

    let action = extract_tag(&description, "action:").unwrap_or("search".to_string()).to_lowercase();
    let query_raw = extract_tag(&description, "query:").unwrap_or_default().to_lowercase();
    let limit_str = extract_tag(&description, "limit:").unwrap_or("50".to_string());
    let limit: usize = limit_str.parse().unwrap_or(50);
    let offset_str = extract_tag(&description, "offset:").unwrap_or("0".to_string());
    let offset: usize = offset_str.parse().unwrap_or(0);
    let scope_override = extract_tag(&description, "scope:").unwrap_or_default().to_lowercase();

    // Split query into individual search terms for ANY-word matching
    let query_terms: Vec<&str> = query_raw.split_whitespace().collect();
    let is_browse = action == "recent" || action == "browse" || action == "read";
    let is_exact = action == "exact";
    let is_semantic_cluster = action == "semantic_cluster";
    let is_browse = is_browse || is_semantic_cluster || (action == "search" && query_raw.is_empty());

    // The real directory structure is:
    //   memory/public_{channel_id}/{user_id}/timeline.jsonl   (public)
    //   memory/private_{user_id}/timeline.jsonl               (private)
    //
    // By default, we search the CURRENT user's timeline in the current channel.
    // scope:[channel] → search ALL users' timelines within the current channel
    // scope:[all_public] → search ALL users across ALL public channels
    // scope:[<channel_id>] → search ALL users within a specific channel

    let timeline_paths: Vec<std::path::PathBuf> = if scope_override == "all_public" {
        // Sweep ALL public channels, ALL users
        collect_all_timelines_under("memory", "public_").await
    } else if scope_override == "channel" {
        // Search ALL users in the current channel
        match current_scope {
            Scope::Public { channel_id, .. } => {
                let channel_dir = format!("memory/public_{}", channel_id);
                collect_user_timelines_in(&channel_dir).await
            }
            Scope::Private { user_id } => {
                vec![std::path::PathBuf::from(format!("memory/private_{}/timeline.jsonl", user_id))]
            }
        }
    } else if !scope_override.is_empty() {
        // Search ALL users in a specific channel by ID
        let channel_dir = format!("memory/public_{}", scope_override);
        collect_user_timelines_in(&channel_dir).await
    } else {
        // Default: search current user's timeline in the current channel
        let path = match current_scope {
            Scope::Public { channel_id, user_id } => {
                std::path::PathBuf::from(format!("memory/public_{}/{}/timeline.jsonl", channel_id, user_id))
            }
            Scope::Private { user_id } => {
                std::path::PathBuf::from(format!("memory/private_{}/timeline.jsonl", user_id))
            }
        };
        tracing::debug!("[AGENT:timeline] Default scope path: {:?} exists={}", path, path.exists());
        vec![path]
    };

    tracing::debug!("[AGENT:timeline] query_raw='{}' query_terms={:?} scope_override='{}' paths_count={} paths={:?}", 
        query_raw, query_terms, scope_override, timeline_paths.len(), timeline_paths);

    // ── SEMANTIC SEARCH (Vector Embeddings) ──────────────────────────
    // If action:[semantic], use vector similarity instead of keyword matching.
    if action == "semantic" {
        if query_raw.is_empty() {
            return ToolResult {
                task_id,
                output: "Semantic search requires a query. Use: action:[semantic] query:[your question]".to_string(),
                tokens_used: 0,
                status: ToolStatus::Success,
            };
        }

        let embed_client = match _memory.embed_client.as_ref() {
            Some(c) => c,
            None => {
                return ToolResult {
                    task_id,
                    output: "Vector embeddings not available (HIVE_EMBED_MODEL not configured). Falling back to keyword search with action:[search].".to_string(),
                    tokens_used: 0,
                    status: ToolStatus::Success,
                };
            }
        };

        // Embed the query
        let query_vec = match embed_client.embed(&query_raw).await {
            Ok(v) => v,
            Err(e) => {
                return ToolResult {
                    task_id,
                    output: format!("Failed to embed query: {}. Try keyword search with action:[search].", e),
                    tokens_used: 0,
                    status: ToolStatus::Success,
                };
            }
        };

        // Determine source filter based on query hints
        let source_filter = if description.to_lowercase().contains("source:[timeline]") {
            Some(crate::memory::vector_index::SourceType::Timeline)
        } else if description.to_lowercase().contains("source:[synaptic]") {
            Some(crate::memory::vector_index::SourceType::Synaptic)
        } else if description.to_lowercase().contains("source:[lesson]") {
            Some(crate::memory::vector_index::SourceType::Lesson)
        } else {
            None // Search all memory types
        };

        let results = _memory.vector_index.search(&query_vec, limit, source_filter).await;

        if results.is_empty() {
            return ToolResult {
                task_id,
                output: format!("No semantic matches found for '{}'. The vector index may be empty — embeddings build up as new data is added.", query_raw),
                tokens_used: 0,
                status: ToolStatus::Success,
            };
        }

        let mut output = format!("Semantic Search Results for '{}' ({} matches, searching across all memory):\n\n", query_raw, results.len());
        for (score, entry) in &results {
            let source_label = match entry.source {
                crate::memory::vector_index::SourceType::Timeline => "TIMELINE",
                crate::memory::vector_index::SourceType::Synaptic => "SYNAPTIC",
                crate::memory::vector_index::SourceType::Lesson => "LESSON",
            };
            output.push_str(&format!("[{} | {:.1}% match | {}] {}\n\n", 
                source_label, score * 100.0, entry.timestamp, entry.text_preview));
        }

        let (t, s, l) = _memory.vector_index.stats().await;
        output.push_str(&format!("\n[Vector Index: {} timeline, {} synaptic, {} lesson embeddings indexed]", t, s, l));

        return ToolResult {
            task_id,
            output,
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    if timeline_paths.is_empty() {
        return ToolResult {
            task_id,
            output: "No timelines found for the requested scope.".to_string(),
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    // Search across all targeted timeline files
    let mut results = Vec::new();
    let mut searched_count = 0;

    for timeline_path in &timeline_paths {
        match tokio::fs::File::open(&timeline_path).await {
            Ok(file) => {
                searched_count += 1;
                let reader = BufReader::new(file);
                let mut lines = reader.lines();
                let mut all_lines = Vec::new();
                while let Ok(Some(line)) = lines.next_line().await {
                    all_lines.push(line);
                }

                tracing::debug!("[AGENT:timeline] Opened {:?}, read {} lines, is_browse={} is_exact={}, query_terms={:?}", 
                    timeline_path, all_lines.len(), is_browse, is_exact, query_terms);

                // Label for multi-file results: extract user_id from path
                let parent_name = timeline_path.parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");

                let mut skipped = 0;
                let mut last_dt: Option<chrono::DateTime<chrono::FixedOffset>> = None;
                for line in all_lines.iter().rev() {
                    // Browse: return all. Exact: full-phrase match. Search: any-word match.
                    let matches = if is_browse {
                        true
                    } else if is_exact {
                        let line_lower = line.to_lowercase();
                        line_lower.contains(&query_raw)
                    } else {
                        let line_lower = line.to_lowercase();
                        query_terms.iter().any(|term| line_lower.contains(term))
                    };

                    if matches
                        && let Ok(json) = serde_json::from_str::<serde_json::Value>(line)
                            && let (Some(author), Some(content)) = (json["author_name"].as_str(), json["content"].as_str()) {
                                // Skip internal reasoning traces — these are accessed via review_reasoning
                                let author_id = json["author_id"].as_str().unwrap_or("");
                                if author_id == "internal" {
                                    continue;
                                }

                                if skipped < offset {
                                    skipped += 1;
                                    continue;
                                }

                                let timestamp_str = json["timestamp"].as_str().unwrap_or("1970-01-01T00:00:00Z");
                                let current_dt = chrono::DateTime::parse_from_rfc3339(timestamp_str).ok();
                                
                                if is_semantic_cluster {
                                    if let (Some(curr), Some(last)) = (current_dt, last_dt) {
                                        let hours = last.signed_duration_since(curr).num_hours();
                                        if hours >= 1 {
                                            results.push(format!("--- [SESSION BREAK] (Delta: {}h) ---", hours));
                                        }
                                    }
                                    last_dt = current_dt;
                                }

                                let msg_idx = json["message_index"].as_u64().unwrap_or(0);

                                let prefix = if timeline_paths.len() > 1 {
                                    format!("[{} | Msg {}] [{}] {}", timestamp_str, msg_idx, parent_name, author)
                                } else {
                                    format!("[{} | Msg {}] {}", timestamp_str, msg_idx, author)
                                };
                                let truncated_content = if content.len() > 500 {
                                    let safe: String = content.chars().take(500).collect();
                                    format!("{}... [truncated, {} total chars]", safe, content.len())
                                } else {
                                    content.to_string()
                                };
                                results.push(format!("{}: {}", prefix, truncated_content));
                                if results.len() >= limit {
                                    break;
                                }
                            }
                }
            }
            Err(e) => {
                tracing::warn!("[AGENT:timeline] FAILED to open {:?}: {}", timeline_path, e);
            }
        }

        if results.len() >= limit {
            break;
        }
    }

    if searched_count == 0 {
        return ToolResult {
            task_id,
            output: "No long-term timeline exists for this scope yet.".to_string(),
            tokens_used: 0,
            status: ToolStatus::Success,
        };
    }

    results.reverse(); // Chronological order

    if results.is_empty() {
        ToolResult {
            task_id,
            output: format!("No matches found for '{}' across {} timeline(s) searched.", query_raw, searched_count),
            tokens_used: 0,
            status: ToolStatus::Success,
        }
    } else {
        let mut final_text = String::new();
        if results.len() >= limit {
            final_text.push_str(&format!("[SYSTEM WARNING LIMIT HIT: This search reached limit:[{}]. If you need OLDER history, you MUST immediately run search_timeline again with `offset:[{}]` to paginate backwards and skip these {} recent matches before trying to answer from incomplete data!]\n\n", limit, offset + limit, offset + limit));
        }
        final_text.push_str(&format!("Timeline Search Results for '{}' ({} timeline(s) searched):\n\n{}", query_raw, searched_count, results.join("\n\n")));
        ToolResult {
            task_id,
            output: final_text,
            tokens_used: 0,
            status: ToolStatus::Success,
        }
    }
}

/// Collect all timeline.jsonl files from user subdirectories within a channel directory
async fn collect_user_timelines_in(channel_dir: &str) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(channel_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.path().is_dir() {
                let tl = entry.path().join("timeline.jsonl");
                if tl.exists() {
                    paths.push(tl);
                }
            }
        }
    }
    paths
}

/// Collect all timeline.jsonl files across all public channels and their user subdirectories
async fn collect_all_timelines_under(base: &str, prefix: &str) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(base).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) && entry.path().is_dir() {
                let channel_dir = entry.path().to_string_lossy().to_string();
                let mut user_paths = collect_user_timelines_in(&channel_dir).await;
                paths.append(&mut user_paths);
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_execute_search_timeline() {
        let mem = Arc::new(MemoryStore::default());
        let scope = Scope::Private { user_id: "test_tl_user".to_string() };

        // Empty query triggers browse mode (returns recent entries, not an error)
        let res = execute_search_timeline("1".into(), "limit:[5]".into(), mem.clone(), None, &scope, None).await;
        assert_eq!(res.status, ToolStatus::Success);

        // Empty timeline search should yield success but "no timeline"
        let res = execute_search_timeline("1".into(), "query:[apple]".into(), mem.clone(), None, &scope, None).await;
        assert!(res.output.contains("No long-term timeline exists"));

        // Setup some fake timeline data at the CORRECT path: memory/private_{user_id}/timeline.jsonl
        let dir = std::path::PathBuf::from("memory/private_test_tl_user");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let file_path = dir.join("timeline.jsonl");
        
        // Write some JSONL lines
        let ev1 = serde_json::json!({"author_name": "User", "content": "I like apple pie."}).to_string();
        let ev2 = serde_json::json!({"author_name": "Apis", "content": "I prefer banana bread."}).to_string();
        let ev3 = serde_json::json!({"author_name": "User", "content": "Another apple reference."}).to_string();
        
        tokio::fs::write(&file_path, format!("{}\n{}\n{}\n", ev1, ev2, ev3)).await.unwrap();

        // Search for 'apple' with standard limit
        let res = execute_search_timeline("1".into(), "query:[apple]".into(), mem.clone(), None, &scope, None).await;
        assert_eq!(res.status, ToolStatus::Success);
        assert!(res.output.contains("User: I like apple pie."));
        assert!(res.output.contains("User: Another apple reference."));
        assert!(!res.output.contains("banana"));

        // Search for 'apple' with limit 1
        let res = execute_search_timeline("2".into(), "query:[apple] limit:[1]".into(), mem.clone(), None, &scope, None).await;
        assert_eq!(res.status, ToolStatus::Success);
        assert!(!res.output.contains("User: I like apple pie.")); // Older one should be dropped
        assert!(res.output.contains("User: Another apple reference.")); // Newer one is kept

        // Cleanup
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn test_cross_channel_search_with_user_subdirs() {
        let mem = Arc::new(MemoryStore::default());
        // We're in channel_A as user1
        let scope = Scope::Public { channel_id: "xchannel_A".to_string(), user_id: "xuser1".to_string() };

        // Create a timeline in a DIFFERENT channel (channel_B) under a user subdir
        let dir_b = std::path::PathBuf::from("memory/public_xchannel_B/xzenzic_user");
        tokio::fs::create_dir_all(&dir_b).await.unwrap();
        let ev = serde_json::json!({"author_name": "Zenzic", "content": "Orthogonal inversion mirrored"}).to_string();
        tokio::fs::write(dir_b.join("timeline.jsonl"), format!("{}\n", ev)).await.unwrap();

        // Searching from channel_A for Zenzic (default scope = own user) should fail
        let res = execute_search_timeline("1".into(), "query:[zenzic]".into(), mem.clone(), None, &scope, None).await;
        assert!(res.output.contains("No long-term timeline exists"), "Default scope should not see other channels");

        // With scope:[xchannel_B], should find it (searches all users in that channel)
        let res = execute_search_timeline("2".into(), "query:[zenzic] scope:[xchannel_B]".into(), mem.clone(), None, &scope, None).await;
        assert_eq!(res.status, ToolStatus::Success);
        assert!(res.output.contains("Zenzic"), "Cross-channel search should find Zenzic, got: {}", res.output);

        // Cleanup
        let _ = tokio::fs::remove_dir_all("memory/public_xchannel_B").await;
        let _ = tokio::fs::remove_dir_all("memory/public_xchannel_A").await;
    }

    #[tokio::test]
    async fn test_channel_wide_search() {
        let mem = Arc::new(MemoryStore::default());
        let scope = Scope::Public { channel_id: "ychannel_1".to_string(), user_id: "yuser_alice".to_string() };

        // Create timelines for TWO users in the same channel
        let dir_alice = std::path::PathBuf::from("memory/public_ychannel_1/yuser_alice");
        let dir_bob = std::path::PathBuf::from("memory/public_ychannel_1/yuser_bob");
        tokio::fs::create_dir_all(&dir_alice).await.unwrap();
        tokio::fs::create_dir_all(&dir_bob).await.unwrap();

        let ev_alice = serde_json::json!({"author_name": "Alice", "content": "Hello world from Alice"}).to_string();
        let ev_bob = serde_json::json!({"author_name": "Bob", "content": "Hello world from Bob"}).to_string();
        tokio::fs::write(dir_alice.join("timeline.jsonl"), format!("{}\n", ev_alice)).await.unwrap();
        tokio::fs::write(dir_bob.join("timeline.jsonl"), format!("{}\n", ev_bob)).await.unwrap();

        // Default search: only finds Alice (current user)
        let res = execute_search_timeline("1".into(), "query:[hello]".into(), mem.clone(), None, &scope, None).await;
        assert!(res.output.contains("Alice"), "Should find Alice: {}", res.output);
        assert!(!res.output.contains("Bob"), "Should NOT find Bob in default scope: {}", res.output);

        // scope:[channel] search: finds BOTH users
        let res = execute_search_timeline("2".into(), "query:[hello] scope:[channel]".into(), mem.clone(), None, &scope, None).await;
        assert!(res.output.contains("Alice"), "Channel search should find Alice: {}", res.output);
        assert!(res.output.contains("Bob"), "Channel search should find Bob: {}", res.output);

        // Cleanup
        let _ = tokio::fs::remove_dir_all("memory/public_ychannel_1").await;
    }
}
