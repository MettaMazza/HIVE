pub mod kernel;
pub mod identity;
pub mod hud;
pub mod observer;

use crate::models::scope::Scope;
use crate::memory::MemoryStore;
use std::sync::Arc;
use crate::prompts::hud::HudData;

pub struct SystemPromptBuilder;

impl SystemPromptBuilder {
    pub async fn assemble(scope: &Scope, memory_store: Arc<MemoryStore>) -> String {
        // Build live HUD data
        let hud_data = HudData::build(scope, memory_store).await;
        let hud_string = hud::format_hud(&hud_data);

        let kernel_string = kernel::get_laws();
        let identity_string = identity::get_persona();

        // Observer is NOT concatenated here; it runs as a separate 1:1 interceptor hook.
        // The core system prompt is just HUD + KERNEL + IDENTITY.
        format!("{}\n\n{}\n\n{}", hud_string, kernel_string, identity_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_system_prompt_builder_assemble() {
        let mem = Arc::new(MemoryStore::default());
        let scope = Scope::Public { channel_id: "test".into(), user_id: "test".into() };
        let prompt = SystemPromptBuilder::assemble(&scope, mem).await;
        
        // Should contain HUD, Kernel, and Identity sections
        assert!(prompt.contains("Apis HUD"));
        assert!(prompt.contains("Kernel Laws"));
        assert!(prompt.contains("Identity Core"));
    }

    #[tokio::test]
    async fn test_system_prompt_builder_private_scope() {
        let mem = Arc::new(MemoryStore::default());
        let scope = Scope::Private { user_id: "dm_user".into() };
        let prompt = SystemPromptBuilder::assemble(&scope, mem).await;
        
        assert!(prompt.contains("Private (User ID: dm_user)"));
        assert!(prompt.contains("Kernel Laws"));
    }

    #[test]
    fn test_format_hud_with_roster() {
        let data = HudData {
            temporal_metrics: "Current System Time: 2026-03-07T12:00:00Z".to_string(),
            timeline_narratives: String::new(),
            active_scope: "Public (Broadcast Channel: main)".to_string(),
            working_memory_load: "500 / 256000".to_string(),
            scratchpad_content: None,
            room_roster: Some("Alice, Bob, Charlie".to_string()),
            relevant_lessons: None,
            relevant_routines: None,
            global_activity: String::new(),
            kg_snapshot: String::new(),
            tape_focus: String::new(),
            user_preferences: String::new(),
            system_logs: String::new(),
            recent_reasoning_traces: String::new(),
            swarm_status: String::new(),
        };
        
        let output = hud::format_hud(&data);
        assert!(output.contains("Room Roster"));
        assert!(output.contains("Alice, Bob, Charlie"));
        assert!(!output.contains("Scratchpad"));
    }

    #[test]
    fn test_format_hud_no_scratchpad_no_roster() {
        let data = HudData {
            temporal_metrics: "Current System Time: 2026-03-07T12:00:00Z".to_string(),
            timeline_narratives: String::new(),
            active_scope: "Private (User ID: u1)".to_string(),
            working_memory_load: "0 / 256000".to_string(),
            scratchpad_content: None,
            room_roster: None,
            relevant_lessons: None,
            relevant_routines: None,
            global_activity: String::new(),
            kg_snapshot: String::new(),
            tape_focus: String::new(),
            user_preferences: String::new(),
            system_logs: String::new(),
            recent_reasoning_traces: String::new(),
            swarm_status: String::new(),
        };
        
        let output = hud::format_hud(&data);
        assert!(output.contains("Private (User ID: u1)"));
        assert!(!output.contains("Scratchpad"));
        assert!(!output.contains("Current participants:"));
    }
}
