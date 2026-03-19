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
        platform: "goal_planner".into(),
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
        platform: "goal_planner".into(),
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
