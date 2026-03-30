use std::sync::Arc;
use crate::engine::goals::GoalNode;
use crate::providers::Provider;
use crate::models::message::Event;
use crate::models::scope::Scope;

/// Decompose a goal into 2–5 subgoals using the LLM.
/// Returns a list of (title, description, priority) tuples.
pub async fn decompose_goal(
    goal: &GoalNode,
    provider: Arc<dyn Provider>,
) -> Vec<(String, String, f64)> {
    let system_prompt = format!(
        "You are a Goal Decomposition Engine. Break the following goal into 2-5 concrete, \
         actionable subgoals. Each subgoal must be independently achievable and together \
         they must fully cover the parent goal.\n\n\
         Parent Goal: {}\nDescription: {}\nCurrent Progress: {:.0}%\n\n\
         Output ONLY a JSON array: [{{\"title\": \"...\", \"description\": \"...\", \"priority\": 0.0-1.0}}]\n\
         No preamble, no explanation, just the JSON array.",
        goal.title, goal.description, goal.progress * 100.0
    );

    let dummy_event = Event {
        platform: "system:goal_planner".into(),
        scope: Scope::Private { user_id: "system".into() },
        author_name: "GoalPlanner".into(),
        author_id: "system".into(),
        content: "Decompose goal".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
    };

    let result = match provider.generate(&system_prompt, &[], &dummy_event, "", None, None).await {
        Ok(text) => text,
        Err(e) => {
            tracing::error!("[GOAL_PLANNER] Decomposition failed: {:?}", e);
            return vec![];
        }
    };

    // Parse JSON array from the response
    parse_subgoals(&result)
}

/// Select the highest-priority actionable goal from a list.
/// Returns the ID of the goal to pursue, or None.
pub fn select_goal(actionable: &[GoalNode]) -> Option<String> {
    if actionable.is_empty() {
        return None;
    }

    // Rank by priority (desc), then by deadline proximity (asc)
    let mut ranked = actionable.to_vec();
    ranked.sort_by(|a, b| {
        // Higher priority first
        b.priority.partial_cmp(&a.priority)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                // Earlier deadline first
                match (&a.deadline, &b.deadline) {
                    (Some(da), Some(db)) => da.partial_cmp(db).unwrap_or(std::cmp::Ordering::Equal),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
            })
    });

    ranked.first().map(|g| g.id.clone())
}

/// Evaluate tool output against a goal, returning (is_complete, progress_delta, evidence_text).
pub async fn evaluate_progress(
    goal: &GoalNode,
    tool_output: &str,
    provider: Arc<dyn Provider>,
) -> (bool, f64, String) {
    let system_prompt = format!(
        "You are a Goal Progress Evaluator. Given a goal and the tool output below, assess:\n\
         1. Is the goal complete? (true/false)\n\
         2. Progress delta (0.0-1.0, how much closer to completion)\n\
         3. Evidence summary (one sentence)\n\n\
         Goal: {}\nDescription: {}\nCurrent Progress: {:.0}%\n\n\
         Tool Output:\n{}\n\n\
         Output ONLY JSON: {{\"complete\": bool, \"delta\": float, \"evidence\": \"string\"}}\n\
         No preamble.",
        goal.title, goal.description, goal.progress * 100.0,
        if tool_output.len() > 2000 { &tool_output[..2000] } else { tool_output }
    );

    let dummy_event = Event {
        platform: "system:goal_planner".into(),
        scope: Scope::Private { user_id: "system".into() },
        author_name: "GoalEvaluator".into(),
        author_id: "system".into(),
        content: "Evaluate progress".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
    };

    let result = match provider.generate(&system_prompt, &[], &dummy_event, "", None, None).await {
        Ok(text) => text,
        Err(e) => {
            tracing::error!("[GOAL_PLANNER] Evaluation failed: {:?}", e);
            return (false, 0.0, "Evaluation failed".into());
        }
    };

    parse_evaluation(&result)
}

// ─── Parsers ───────────────────────────────────────────────────────────────

fn parse_subgoals(text: &str) -> Vec<(String, String, f64)> {
    // Find JSON array in the response
    let trimmed = text.trim();
    let json_str = if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            &trimmed[start..=end]
        } else {
            return vec![];
        }
    } else {
        return vec![];
    };

    match serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
        Ok(arr) => {
            arr.iter().filter_map(|item| {
                let title = item.get("title")?.as_str()?.to_string();
                let desc = item.get("description")?.as_str()?.to_string();
                let priority = item.get("priority").and_then(|v| v.as_f64()).unwrap_or(0.5);
                Some((title, desc, priority))
            }).collect()
        }
        Err(e) => {
            tracing::warn!("[GOAL_PLANNER] Failed to parse subgoals JSON: {}", e);
            vec![]
        }
    }
}

fn parse_evaluation(text: &str) -> (bool, f64, String) {
    let trimmed = text.trim();
    let json_str = if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            &trimmed[start..=end]
        } else {
            return (false, 0.0, "Parse error".into());
        }
    } else {
        return (false, 0.0, "Parse error".into());
    };

    match serde_json::from_str::<serde_json::Value>(json_str) {
        Ok(obj) => {
            let complete = obj.get("complete").and_then(|v| v.as_bool()).unwrap_or(false);
            let delta = obj.get("delta").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let evidence = obj.get("evidence").and_then(|v| v.as_str()).unwrap_or("").to_string();
            (complete, delta.clamp(0.0, 1.0), evidence)
        }
        Err(_) => (false, 0.0, "Parse error".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::goals::{GoalNode, GoalSource, GoalStatus};

    #[test]
    fn test_parse_subgoals_valid() {
        let json = r#"[{"title":"Sub A","description":"Do A","priority":0.8},{"title":"Sub B","description":"Do B","priority":0.3}]"#;
        let result = parse_subgoals(json);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "Sub A");
        assert_eq!(result[1].2, 0.3);
    }

    #[test]
    fn test_parse_subgoals_with_preamble() {
        let text = "Here are the subgoals:\n[{\"title\":\"X\",\"description\":\"Y\",\"priority\":0.5}]";
        let result = parse_subgoals(text);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_parse_subgoals_empty() {
        assert!(parse_subgoals("no json here").is_empty());
        assert!(parse_subgoals("").is_empty());
    }

    #[test]
    fn test_parse_subgoals_malformed() {
        assert!(parse_subgoals("[{broken json}]").is_empty());
    }

    #[test]
    fn test_parse_evaluation_valid() {
        let json = r#"{"complete": true, "delta": 0.5, "evidence": "Task done"}"#;
        let (complete, delta, evidence) = parse_evaluation(json);
        assert!(complete);
        assert_eq!(delta, 0.5);
        assert_eq!(evidence, "Task done");
    }

    #[test]
    fn test_parse_evaluation_with_preamble() {
        let text = "Analysis:\n{\"complete\": false, \"delta\": 0.2, \"evidence\": \"partial\"}";
        let (complete, delta, _) = parse_evaluation(text);
        assert!(!complete);
        assert_eq!(delta, 0.2);
    }

    #[test]
    fn test_parse_evaluation_empty() {
        let (c, d, e) = parse_evaluation("no json");
        assert!(!c);
        assert_eq!(d, 0.0);
        assert_eq!(e, "Parse error");
    }

    #[test]
    fn test_parse_evaluation_clamp() {
        let json = r#"{"complete": false, "delta": 5.0, "evidence": "way too high"}"#;
        let (_, delta, _) = parse_evaluation(json);
        assert_eq!(delta, 1.0); // clamped
    }

    #[test]
    fn test_select_goal_empty() {
        assert!(select_goal(&[]).is_none());
    }

    #[test]
    fn test_select_goal_priority_ordering() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        let goals = vec![
            GoalNode {
                id: "low".into(), title: "Low".into(), description: "".into(),
                priority: 0.2, progress: 0.0, status: GoalStatus::Active,
                source: GoalSource::User, tags: vec![], evidence: vec![],
                parent_id: None, children: vec![], dependencies: vec![],
                deadline: None, depth: 0, created_at: now, updated_at: now,
            },
            GoalNode {
                id: "high".into(), title: "High".into(), description: "".into(),
                priority: 0.9, progress: 0.0, status: GoalStatus::Active,
                source: GoalSource::User, tags: vec![], evidence: vec![],
                parent_id: None, children: vec![], dependencies: vec![],
                deadline: None, depth: 0, created_at: now, updated_at: now,
            },
        ];
        assert_eq!(select_goal(&goals), Some("high".into()));
    }
}
