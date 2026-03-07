use crate::models::scope::Scope;
use crate::memory::MemoryStore;
use std::sync::Arc;

pub struct HudData {
    pub timestamp: String,
    pub active_scope: String,
    pub working_memory_load: String,
    pub scratchpad_content: Option<String>,
    pub room_roster: Option<String>,
}

impl HudData {
    pub async fn build(scope: &Scope, memory_store: Arc<MemoryStore>) -> Self {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let active_scope = match scope {
            Scope::Public { channel_id, .. } => format!("Public (Broadcast Channel: {})", channel_id),
            Scope::Private { user_id } => format!("Private (User ID: {})", user_id),
        };
        
        let working_memory_load = match scope {
            Scope::Public { .. } => format!("{} / {}", memory_store.working.current_tokens().await, memory_store.working.max_tokens()),
            Scope::Private { .. } => format!("{} / {}", memory_store.working.current_tokens().await, memory_store.working.max_tokens()),
        };

        let scratchpad_content = match memory_store.scratch.read(scope).await {
            content if content.is_empty() => None,
            content => Some(content),
        };
        
        let room_roster = if let Scope::Public { channel_id, .. } = scope {
            memory_store.get_roster(channel_id).await
        } else {
            None
        };

        HudData {
            timestamp,
            active_scope,
            working_memory_load,
            scratchpad_content,
            room_roster,
        }
    }
}

pub fn format_hud(data: &HudData) -> String {
    let mut sections = Vec::new();

    sections.push("## Apis HUD (Live System Context)".to_string());
    
    sections.push("### Temporal Awareness".to_string());
    sections.push(format!("Current System Time: {}", data.timestamp));
    sections.push("".to_string());

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
        }).await;
        
        mem.add_event(crate::models::message::Event {
            platform: "test".into(),
            scope: pub_scope.clone(),
            author_name: "Bob".into(),
            author_id: "bob1".into(),
            content: "Pong".into(),
        }).await;

        let hud = HudData::build(&pub_scope, mem).await;

        assert_eq!(hud.active_scope, format!("Public (Broadcast Channel: {})", unique_id));
        assert!(hud.working_memory_load.ends_with(" / 256000"));
        assert_eq!(hud.scratchpad_content, Some("First noteSecond note\n".to_string()));
        assert_eq!(hud.room_roster, Some("Alice, Bob".to_string()));
    }

    #[tokio::test]
    async fn test_format_hud_with_scratchpad() {
        let data = HudData {
            timestamp: "2026-03-07T12:00:00Z".to_string(),
            active_scope: "Public (Broadcast Channel)".to_string(),
            working_memory_load: "100 / 256000".to_string(),
            scratchpad_content: Some("Test note".to_string()),
            room_roster: None,
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
