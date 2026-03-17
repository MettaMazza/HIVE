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
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("🧠 Timeline Drone executing...\n".to_string()).await;
    }
    tracing::debug!("[AGENT:timeline] ▶ task_id={}", task_id);
    let action = extract_tag(&description, "action:").unwrap_or("search".to_string()).to_lowercase();
    let query_raw = extract_tag(&description, "query:").unwrap_or_default().to_lowercase();
    let limit_str = extract_tag(&description, "limit:").unwrap_or("20".to_string());
    let limit: usize = limit_str.parse().unwrap_or(20);
    let scope_override = extract_tag(&description, "scope:").unwrap_or_default().to_lowercase();

    // Split query into individual search terms for ANY-word matching
    let query_terms: Vec<&str> = query_raw.split_whitespace().collect();
    let is_browse = action == "recent" || action == "browse" || action == "read" || (action == "search" && query_raw.is_empty());

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

                tracing::debug!("[AGENT:timeline] Opened {:?}, read {} lines, is_browse={}, searching for any of {:?}", 
                    timeline_path, all_lines.len(), is_browse, query_terms);

                // Label for multi-file results: extract user_id from path
                let parent_name = timeline_path.parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");

                for line in all_lines.iter().rev() {
                    // Browse mode: return all entries. Search mode: match any query term.
                    let matches = if is_browse {
                        true
                    } else {
                        let line_lower = line.to_lowercase();
                        query_terms.iter().any(|term| line_lower.contains(term))
                    };

                    if matches
                        && let Ok(json) = serde_json::from_str::<serde_json::Value>(line)
                            && let (Some(author), Some(content)) = (json["author_name"].as_str(), json["content"].as_str()) {
                                let prefix = if timeline_paths.len() > 1 {
                                    format!("[{}] {}", parent_name, author)
                                } else {
                                    author.to_string()
                                };
                                results.push(format!("{}: {}", prefix, content));
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
        ToolResult {
            task_id,
            output: format!("Timeline Search Results for '{}' ({} timeline(s) searched):\n\n{}", query_raw, searched_count, results.join("\n\n")),
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
        let res = execute_search_timeline("1".into(), "limit:[5]".into(), mem.clone(), None, &scope).await;
        assert_eq!(res.status, ToolStatus::Success);

        // Empty timeline search should yield success but "no timeline"
        let res = execute_search_timeline("1".into(), "query:[apple]".into(), mem.clone(), None, &scope).await;
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
        let res = execute_search_timeline("1".into(), "query:[apple]".into(), mem.clone(), None, &scope).await;
        assert_eq!(res.status, ToolStatus::Success);
        assert!(res.output.contains("User: I like apple pie."));
        assert!(res.output.contains("User: Another apple reference."));
        assert!(!res.output.contains("banana"));

        // Search for 'apple' with limit 1
        let res = execute_search_timeline("2".into(), "query:[apple] limit:[1]".into(), mem.clone(), None, &scope).await;
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
        let res = execute_search_timeline("1".into(), "query:[zenzic]".into(), mem.clone(), None, &scope).await;
        assert!(res.output.contains("No long-term timeline exists"), "Default scope should not see other channels");

        // With scope:[xchannel_B], should find it (searches all users in that channel)
        let res = execute_search_timeline("2".into(), "query:[zenzic] scope:[xchannel_B]".into(), mem.clone(), None, &scope).await;
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
        let res = execute_search_timeline("1".into(), "query:[hello]".into(), mem.clone(), None, &scope).await;
        assert!(res.output.contains("Alice"), "Should find Alice: {}", res.output);
        assert!(!res.output.contains("Bob"), "Should NOT find Bob in default scope: {}", res.output);

        // scope:[channel] search: finds BOTH users
        let res = execute_search_timeline("2".into(), "query:[hello] scope:[channel]".into(), mem.clone(), None, &scope).await;
        assert!(res.output.contains("Alice"), "Channel search should find Alice: {}", res.output);
        assert!(res.output.contains("Bob"), "Channel search should find Bob: {}", res.output);

        // Cleanup
        let _ = tokio::fs::remove_dir_all("memory/public_ychannel_1").await;
    }
}
