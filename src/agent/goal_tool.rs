use crate::engine::goals::{GoalSource, GoalStatus, GoalStore};
use crate::models::scope::Scope;
use crate::models::tool::{ToolResult, ToolStatus};
use crate::providers::Provider;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Extract a tag value from a description string: `tag:[value]`
fn extract_tag(desc: &str, tag: &str) -> Option<String> {
    let pattern = format!("{}:[", tag);
    if let Some(start_idx) = desc.find(&pattern) {
        let after = &desc[start_idx + pattern.len()..];
        if let Some(end_idx) = after.find(']') {
            return Some(after[..end_idx].trim().to_string());
        }
    }
    None
}

pub async fn execute_goal_tool(
    task_id: String,
    description: String,
    scope: Scope,
    goal_store: Arc<GoalStore>,
    provider: Arc<dyn Provider>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("🎯 Goal System processing...\n".into()).await;
    }
    tracing::debug!("[AGENT:goal_tool] ▶ task_id={} desc_len={}", task_id, description.len());

    let action = extract_tag(&description, "action")
        .unwrap_or_else(|| "list".into())
        .to_lowercase();

    let tree = goal_store.get_tree(&scope).await;

    let output = match action.as_str() {
        "create" => {
            let title = match extract_tag(&description, "title") {
                Some(t) => t,
                None => return ToolResult {
                    task_id, output: "Missing required tag: title:[...]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing title".into()),
                },
            };
            let desc_text = extract_tag(&description, "description").unwrap_or_default();
            let priority: f64 = extract_tag(&description, "priority")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.5);
            let tags: Vec<String> = extract_tag(&description, "tags")
                .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
                .unwrap_or_default();
            
            let deps_raw = extract_tag(&description, "depends_on").unwrap_or_default();
            let dependencies: Vec<String> = if deps_raw.is_empty() { vec![] } else {
                deps_raw.split(',').map(|s| s.trim().to_string()).collect()
            };

            let id = tree.add_root_goal(title.clone(), desc_text, priority, GoalSource::User, tags).await;
            if !dependencies.is_empty() {
                tree.set_dependencies(&id, dependencies).await;
            }
            
            format!("✅ Created goal '{}' (id: {})", title, id)
        }

        "decompose" => {
            let goal_id = match extract_tag(&description, "id") {
                Some(id) => id,
                None => return ToolResult {
                    task_id, output: "Missing required tag: id:[...]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing id".into()),
                },
            };

            let goal = match tree.get_goal(&goal_id).await {
                Some(g) => g,
                None => return ToolResult {
                    task_id, output: format!("Goal '{}' not found.", goal_id),
                    tokens_used: 0, status: ToolStatus::Failed("Goal not found".into()),
                },
            };

            let subgoals = crate::agent::goal_planner::decompose_goal(&goal, provider).await;
            if subgoals.is_empty() {
                return ToolResult {
                    task_id, output: "Decomposition produced no subgoals.".into(),
                    tokens_used: 0, status: ToolStatus::Failed("No subgoals".into()),
                };
            }

            let mut result_lines = vec![format!("🔀 Decomposed '{}' into {} subgoals:", goal.title, subgoals.len())];
            for (title, desc, priority) in subgoals {
                let tags = goal.tags.clone();
                if let Some(sub_id) = tree.add_subgoal(&goal_id, title.clone(), desc, priority, tags).await {
                    result_lines.push(format!("  └─ {} (priority: {:.1}, id: {})", title, priority, sub_id));
                }
            }
            result_lines.join("\n")
        }

        "list" => {
            let prompt = tree.format_for_prompt().await;
            let (total, completed) = tree.stats().await;
            format!("GOAL TREE ({} total, {} completed):\n{}", total, completed, prompt)
        }

        "status" => {
            let goal_id = match extract_tag(&description, "id") {
                Some(id) => id,
                None => return ToolResult {
                    task_id, output: "Missing required tag: id:[...]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing id".into()),
                },
            };

            let new_status = match extract_tag(&description, "status").as_deref() {
                Some("completed") | Some("complete") | Some("done") => GoalStatus::Completed,
                Some("active") => GoalStatus::Active,
                Some("pending") => GoalStatus::Pending,
                Some("failed") => GoalStatus::Failed,
                Some("blocked") => GoalStatus::Blocked,
                _ => return ToolResult {
                    task_id, output: "Missing or invalid status. Use: completed, active, pending, failed, blocked".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Invalid status".into()),
                },
            };

            match tree.update_status_safe(&goal_id, new_status.clone()).await {
                Ok(true) => format!("✅ Updated goal {} to {:?}", goal_id, new_status),
                Ok(false) => return ToolResult {
                    task_id, output: format!("Goal '{}' not found.", goal_id),
                    tokens_used: 0, status: ToolStatus::Failed("Goal not found".into()),
                },
                Err(e) => return ToolResult {
                    task_id, output: e,
                    tokens_used: 0, status: ToolStatus::Failed("Dependency Error".into()),
                }
            }
        }

        "progress" => {
            let goal_id = match extract_tag(&description, "id") {
                Some(id) => id,
                None => return ToolResult {
                    task_id, output: "Missing required tag: id:[...]".into(),
                    tokens_used: 0, status: ToolStatus::Failed("Missing id".into()),
                },
            };
            let evidence = extract_tag(&description, "evidence").unwrap_or_else(|| "Progress noted.".into());
            let delta: f64 = extract_tag(&description, "delta")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.1);

            if tree.add_evidence(&goal_id, evidence.clone(), delta).await {
                let goal = tree.get_goal(&goal_id).await;
                let progress = goal.map(|g| g.progress).unwrap_or(0.0);
                format!("✅ Added evidence to goal. Progress now: {:.0}%", progress * 100.0)
            } else {
                return ToolResult {
                    task_id, output: format!("Goal '{}' not found.", goal_id),
                    tokens_used: 0, status: ToolStatus::Failed("Goal not found".into()),
                };
            }
        }

        "prune" => {
            let pruned = tree.prune_completed().await;
            format!("🗑️ Pruned {} completed goals and their subtrees.", pruned)
        }

        _ => {
            return ToolResult {
                task_id,
                output: format!("Unknown action '{}'. Available: create, decompose, list, status, progress, prune", action),
                tokens_used: 0,
                status: ToolStatus::Failed("Unknown action".into()),
            };
        }
    };

    ToolResult {
        task_id,
        output,
        tokens_used: 0,
        status: ToolStatus::Success,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::goals::GoalStore;
    use crate::models::scope::Scope;
    use crate::providers::MockProvider;

    fn test_scope() -> Scope {
        Scope::Private { user_id: "goal_tester".into() }
    }

    fn mock_prov() -> Arc<dyn crate::providers::Provider> {
        let mock = MockProvider::new();
        Arc::new(mock)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_and_list() {
        let store = Arc::new(GoalStore::new("/tmp/hive_goal_test"));
        let prov = mock_prov();

        let r = execute_goal_tool("1".into(), "action:[create] title:[Test Goal] description:[A test] priority:[0.7]".into(), test_scope(), store.clone(), prov.clone(), None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Created goal"));

        let r2 = execute_goal_tool("2".into(), "action:[list]".into(), test_scope(), store.clone(), prov.clone(), None).await;
        assert!(r2.output.contains("Test Goal"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_missing_title() {
        let store = Arc::new(GoalStore::new("/tmp/hive_goal_test"));
        let prov = mock_prov();

        let r = execute_goal_tool("1".into(), "action:[create] description:[no title]".into(), test_scope(), store, prov, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_unknown_action() {
        let store = Arc::new(GoalStore::new("/tmp/hive_goal_test"));
        let prov = mock_prov();

        let r = execute_goal_tool("1".into(), "action:[explode]".into(), test_scope(), store, prov, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_status_missing_id() {
        let store = Arc::new(GoalStore::new("/tmp/hive_goal_test"));
        let prov = mock_prov();

        let r = execute_goal_tool("1".into(), "action:[status] status:[active]".into(), test_scope(), store, prov, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_prune() {
        let store = Arc::new(GoalStore::new("/tmp/hive_goal_test"));
        let prov = mock_prov();

        let r = execute_goal_tool("1".into(), "action:[prune]".into(), test_scope(), store, prov, None).await;
        assert_eq!(r.status, ToolStatus::Success);
        assert!(r.output.contains("Pruned"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_progress_missing_id() {
        let store = Arc::new(GoalStore::new("/tmp/hive_goal_test"));
        let prov = mock_prov();

        let r = execute_goal_tool("1".into(), "action:[progress] evidence:[did things]".into(), test_scope(), store, prov, None).await;
        assert!(matches!(r.status, ToolStatus::Failed(_)));
    }
}
