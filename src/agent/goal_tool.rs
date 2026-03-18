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

            let id = tree.add_root_goal(title.clone(), desc_text, priority, GoalSource::User, tags).await;
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

            if tree.update_status(&goal_id, new_status.clone()).await {
                format!("✅ Updated goal {} to {:?}", goal_id, new_status)
            } else {
                return ToolResult {
                    task_id, output: format!("Goal '{}' not found.", goal_id),
                    tokens_used: 0, status: ToolStatus::Failed("Goal not found".into()),
                };
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
