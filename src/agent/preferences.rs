use crate::models::tool::{ToolResult as DroneResult, ToolStatus as DroneStatus};
use crate::models::scope::Scope;
use crate::memory::MemoryStore;
use std::sync::Arc;
use tokio::sync::mpsc;

pub fn extract_tag(text: &str, tag: &str) -> Option<String> {
    if let Some(idx) = text.find(tag) {
        let start = idx + tag.len();
        let end = text[start..].find(']').unwrap_or(text[start..].len());
        let val = text[start..start+end].trim().trim_matches(|c| c == '[' || c == ']');
        return Some(val.to_string());
    }
    None
}

pub async fn execute_manage_user_preferences(
    task_id: String,
    description: String,
    scope: Scope,
    memory: Arc<MemoryStore>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> DroneResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send(format!("🧠 Psychological Profiling Drone actively updating Theory of Mind...\n")).await;
    }

    let action = extract_tag(&description, "action:").unwrap_or_default();
    tracing::debug!("[AGENT:preferences] ▶ task_id={} action='{}' scope='{}'", task_id, action, scope.to_key());
    
    if action.is_empty() {
        return DroneResult {
            task_id,
            output: "Error: Missing `action:` tag. Valid actions are: read, update_name, add_hobby, add_topic, update_narrative, update_psychoanalysis".into(),
            tokens_used: 0,
            status: DroneStatus::Failed("Missing action".into()),
        };
    }

    // READ action: return current stored preferences (no value: needed)
    if action == "read" || action == "list" || action == "view" {
        let prefs = memory.preferences.read(&scope).await;
        let name = prefs.name.as_deref().unwrap_or("(not set)");
        let hobbies = if prefs.hobbies.is_empty() { "(none)".to_string() } else { prefs.hobbies.join(", ") };
        let topics = if prefs.topics_of_interest.is_empty() { "(none)".to_string() } else { prefs.topics_of_interest.join(", ") };
        let narrative = if prefs.narrative_history.is_empty() { "(none)".to_string() } else { prefs.narrative_history.clone() };
        let psycho = if prefs.psychoanalysis.is_empty() { "(none)".to_string() } else { prefs.psychoanalysis.clone() };
        
        let output = format!(
            "**Stored User Preferences:**\n• Name: {}\n• Hobbies: {}\n• Topics of Interest: {}\n• Narrative History: {}\n• Psychoanalysis: {}",
            name, hobbies, topics, narrative, psycho
        );
        
        return DroneResult {
            task_id,
            output,
            tokens_used: 0,
            status: DroneStatus::Success,
        };
    }

    let value = if let Some(idx) = description.find("value:") {
        description[idx + 6..].trim().trim_matches(|c| c == '[' || c == ']').to_string()
    } else {
        return DroneResult {
            task_id,
            output: "Error: Missing `value:` tag payload.".into(),
            tokens_used: 0,
            status: DroneStatus::Failed("Missing value".into()),
        };
    };

    let mut prefs = memory.preferences.read(&scope).await;
    let success_msg;

    match action.as_str() {
        "update_name" => {
            prefs.name = Some(value.clone());
            success_msg = format!("User name updated to: {}", value);
        }
        "add_hobby" => {
            if !prefs.hobbies.contains(&value) {
                prefs.hobbies.push(value.clone());
                success_msg = format!("Hobby added: {}", value);
            } else {
                success_msg = format!("Hobby already exists: {}", value);
            }
        }
        "add_topic" => {
            if !prefs.topics_of_interest.contains(&value) {
                prefs.topics_of_interest.push(value.clone());
                success_msg = format!("Topic of interest added: {}", value);
            } else {
                success_msg = format!("Topic already exists: {}", value);
            }
        }
        "update_narrative" => {
            prefs.narrative_history = value.clone();
            success_msg = "Narrative history overwritten/updated.".into();
        }
        "update_psychoanalysis" => {
            prefs.psychoanalysis = value.clone();
            success_msg = "Psychological profile updated.".into();
        }
        _ => {
            return DroneResult {
                task_id,
                output: format!("Error: Unknown action '{}'. Valid: read, update_name, add_hobby, add_topic, update_narrative, update_psychoanalysis", action),
                tokens_used: 0,
                status: DroneStatus::Failed("Invalid action".into()),
            };
        }
    }

    if let Err(e) = memory.preferences.write(&scope, &prefs).await {
        return DroneResult {
            task_id,
            output: format!("Failed to save preferences to disk: {}", e),
            tokens_used: 0,
            status: DroneStatus::Failed("I/O Error".into()),
        };
    }

    DroneResult {
        task_id: task_id.clone(),
        output: success_msg.clone(),
        tokens_used: 0,
        status: DroneStatus::Success,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_memory() -> (Arc<MemoryStore>, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let memory = Arc::new(MemoryStore::new(Some(temp.path().to_path_buf())));
        (memory, temp)
    }

    #[tokio::test]
    async fn test_manage_user_preferences_missing_action() {
        let (memory, _temp) = setup_memory().await;
        let scope = Scope::Private { user_id: "u1".into() };
        
        let res = execute_manage_user_preferences(
            "1".into(),
            "value:[John]".into(),
            scope,
            memory,
            None,
        ).await;

        assert_eq!(res.status, DroneStatus::Failed("Missing action".into()));
        assert!(res.output.contains("Missing `action:` tag"));
    }

    #[tokio::test]
    async fn test_manage_user_preferences_missing_value() {
        let (memory, _temp) = setup_memory().await;
        let scope = Scope::Private { user_id: "u1".into() };
        
        let res = execute_manage_user_preferences(
            "1".into(),
            "action:[update_name]".into(),
            scope,
            memory,
            None,
        ).await;

        assert_eq!(res.status, DroneStatus::Failed("Missing value".into()));
        assert!(res.output.contains("Missing `value:` tag"));
    }

    #[tokio::test]
    async fn test_manage_user_preferences_empty_value() {
        let (memory, _temp) = setup_memory().await;
        let scope = Scope::Private { user_id: "u1".into() };
        
        let res = execute_manage_user_preferences(
            "1".into(),
            "action:[update_name] value:[]".into(),
            scope,
            memory,
            None,
        ).await;

        assert_eq!(res.status, DroneStatus::Success);
        assert!(res.output.contains("User name updated to:"));
    }

    #[tokio::test]
    async fn test_manage_user_preferences_unknown_action() {
        let (memory, _temp) = setup_memory().await;
        let scope = Scope::Private { user_id: "u1".into() };
        
        let res = execute_manage_user_preferences(
            "1".into(),
            "action:[fly] value:[high]".into(),
            scope,
            memory,
            None,
        ).await;

        assert_eq!(res.status, DroneStatus::Failed("Invalid action".into()));
    }

    #[tokio::test]
    async fn test_manage_user_preferences_crud_operations() {
        let (memory, _temp) = setup_memory().await;
        let scope = Scope::Private { user_id: "u1".into() };
        
        // Update Name
        let res = execute_manage_user_preferences(
            "1".into(),
            "action:[update_name] value:[Alice]".into(),
            scope.clone(),
            memory.clone(),
            None,
        ).await;
        assert_eq!(res.status, DroneStatus::Success);
        
        let mut prefs = memory.preferences.read(&scope).await;
        assert_eq!(prefs.name.unwrap(), "Alice");

        // Add Hobby
        let res = execute_manage_user_preferences(
            "2".into(),
            "action:[add_hobby] value:[Archery]".into(),
            scope.clone(),
            memory.clone(),
            None,
        ).await;
        assert_eq!(res.status, DroneStatus::Success);
        prefs = memory.preferences.read(&scope).await;
        assert_eq!(prefs.hobbies[0], "Archery");

        // Add Duplicate Hobby
        let res = execute_manage_user_preferences(
            "3".into(),
            "action:[add_hobby] value:[Archery]".into(),
            scope.clone(),
            memory.clone(),
            None,
        ).await;
        assert_eq!(res.status, DroneStatus::Success);
        assert!(res.output.contains("already exists"));

        // Add Topic
        execute_manage_user_preferences("4".into(), "action:[add_topic] value:[Rust]".into(), scope.clone(), memory.clone(), None).await;
        prefs = memory.preferences.read(&scope).await;
        assert_eq!(prefs.topics_of_interest[0], "Rust");

        // Add Duplicate Topic
        let res = execute_manage_user_preferences("5".into(), "action:[add_topic] value:[Rust]".into(), scope.clone(), memory.clone(), None).await;
        assert!(res.output.contains("already exists"));

        // Update Narrative
        execute_manage_user_preferences("6".into(), "action:[update_narrative] value:[Met long ago.]".into(), scope.clone(), memory.clone(), None).await;
        prefs = memory.preferences.read(&scope).await;
        assert_eq!(prefs.narrative_history, "Met long ago.");

        // Update Psychoanalysis
        execute_manage_user_preferences("7".into(), "action:[update_psychoanalysis] value:[Very analytical user.]".into(), scope.clone(), memory.clone(), None).await;
        prefs = memory.preferences.read(&scope).await;
        assert_eq!(prefs.psychoanalysis, "Very analytical user.");
    }

    #[tokio::test]
    async fn test_manage_user_preferences_io_error() {
        // Use a blatantly invalid path to force fs::write underlying to fail.
        let broken_memory = Arc::new(MemoryStore::new(Some(std::path::PathBuf::from("/dev/null/illegal_dir"))));
        let scope = Scope::Private { user_id: "u1".into() };
        
        let res = execute_manage_user_preferences(
            "1".into(),
            "action:[update_name] value:[Alice]".into(),
            scope,
            broken_memory,
            None,
        ).await;

        assert_eq!(res.status, DroneStatus::Failed("I/O Error".into()));
    }
}
