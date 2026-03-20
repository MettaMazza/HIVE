use crate::models::tool::{ToolResult, ToolStatus};
use crate::memory::MemoryStore;
use crate::models::scope::Scope;
use crate::agent::preferences::extract_tag;
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn execute_manage_lessons(
    task_id: String,
    description: String,
    memory: Arc<MemoryStore>,
    telemetry_tx: Option<mpsc::Sender<String>>,
    current_scope: &Scope,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("🧠 Lessons Drone executing...\n".to_string()).await;
    }
    tracing::debug!("[AGENT:lessons] ▶ task_id={}", task_id);

    let action = extract_tag(&description, "action:").unwrap_or_default().to_lowercase();
    
    if action.is_empty() {
        return ToolResult { 
            task_id, 
            output: "Error: Missing 'action:' field.".to_string(), 
            tokens_used: 0, 
            status: ToolStatus::Failed("Missing action field".into()) 
        };
    }

    match action.as_str() {
        "store" => {
            let lesson_text = extract_tag(&description, "lesson:").unwrap_or("".to_string());
            let keywords_str = extract_tag(&description, "keywords:").unwrap_or("".to_string());
            let confidence_str = extract_tag(&description, "confidence:").unwrap_or("1.0".to_string());

            if lesson_text.is_empty() {
                return ToolResult { 
                    task_id, 
                    output: "Error: Missing 'lesson:' field for store action.".to_string(), 
                    tokens_used: 0, 
                    status: ToolStatus::Failed("Missing field".into()) 
                };
            }

            let confidence: f32 = confidence_str.parse().unwrap_or(1.0f32).clamp(0.0f32, 1.0f32);
            let keywords: Vec<String> = keywords_str.split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();

            // Scope can be explicitly targeted, otherwise defaults to current context scope
            let target_scope = extract_tag(&description, "scope:").map_or(current_scope.clone(), |scope_str| {
                if scope_str.starts_with("public_") {
                    Scope::Public { channel_id: scope_str.replace("public_", ""), user_id: "system".into() }
                } else {
                    Scope::Private { user_id: scope_str.replace("private_", "") }
                }
            });

            let lesson = crate::memory::lessons::Lesson {
                text: lesson_text,
                keywords,
                confidence,
            };

            match memory.lessons.add_lesson(&target_scope, &lesson).await {
                Ok(_) => ToolResult { task_id, output: "Lesson stored successfully.".to_string(), tokens_used: 0, status: ToolStatus::Success },
                Err(e) => ToolResult { task_id, output: format!("Failed to store lesson: {}", e), tokens_used: 0, status: ToolStatus::Failed(e.to_string()) },
            }
        }
        "read" => {
            let lessons = memory.lessons.read_lessons(current_scope).await;
            if lessons.is_empty() {
                ToolResult { task_id, output: "No lessons recorded for this scope.".to_string(), tokens_used: 0, status: ToolStatus::Success }
            } else {
                let formatted: Vec<String> = lessons.iter().map(|l| format!("- {} (Confidence: {:.2})", l.text, l.confidence)).collect();
                ToolResult { task_id, output: format!("Recorded Lessons:\n{}", formatted.join("\n")), tokens_used: 0, status: ToolStatus::Success }
            }
        }
        "search" => {
            let query = extract_tag(&description, "query:").unwrap_or_default().to_lowercase();
            if query.is_empty() {
                return ToolResult { task_id, output: "Error: Missing 'query:' field for search action.".to_string(), tokens_used: 0, status: ToolStatus::Failed("Missing field".into()) };
            }

            let lessons = memory.lessons.read_lessons(current_scope).await;
            let mut matches = Vec::new();
            
            for l in lessons {
                if l.text.to_lowercase().contains(&query) || l.keywords.iter().any(|k| k.contains(&query)) {
                    matches.push(format!("- {} (Confidence: {:.2})", l.text, l.confidence));
                }
            }

            if matches.is_empty() {
                ToolResult { task_id, output: format!("No lessons matched '{}'", query), tokens_used: 0, status: ToolStatus::Success }
            } else {
                ToolResult { task_id, output: format!("Lessons matching '{}':\n{}", query, matches.join("\n")), tokens_used: 0, status: ToolStatus::Success }
            }
        }
        _ => ToolResult {
            task_id,
            output: format!("Unknown action '{}'. Valid actions: store, read, search.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_manage_lessons_tool_execute() {
        let mem = Arc::new(MemoryStore::default());
        let scope = Scope::Private { user_id: "test_les_user".into() };

        // Test missing action
        let res = execute_manage_lessons("1".into(), "lesson:[fire]".into(), mem.clone(), None, &scope).await;
        assert_eq!(res.status, ToolStatus::Failed("Missing action field".into()));

        // Test empty read
        let res = execute_manage_lessons("2".into(), "action:[read]".into(), mem.clone(), None, &scope).await;
        assert!(res.output.contains("No lessons"));

        // Test store missing lesson
        let res = execute_manage_lessons("3".into(), "action:[store] keywords:[fire]".into(), mem.clone(), None, &scope).await;
        assert_eq!(res.status, ToolStatus::Failed("Missing field".into()));

        // Test successful store
        let res = execute_manage_lessons(
            "4".into(),
            "action:[store] lesson:[Fire is hot] keywords:[fire, hot] confidence:[0.9]".into(),
            mem.clone(),
            None,
            &scope
        ).await;
        assert_eq!(res.status, ToolStatus::Success);

        // Test read after store
        let res = execute_manage_lessons("5".into(), "action:[read]".into(), mem.clone(), None, &scope).await;
        assert!(res.output.contains("Fire is hot"));
        assert!(res.output.contains("0.90"));

        // Test search hit
        let res = execute_manage_lessons("6".into(), "action:[search] query:[hot]".into(), mem.clone(), None, &scope).await;
        assert!(res.output.contains("Fire is hot"));

        // Test search miss
        let res = execute_manage_lessons("7".into(), "action:[search] query:[ice]".into(), mem.clone(), None, &scope).await;
        assert!(res.output.contains("No lessons matched"));
    }
}
