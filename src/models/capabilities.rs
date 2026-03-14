#![allow(clippy::derivable_impls)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilities {
    pub admin_users: Vec<String>,
    pub has_terminal_access: bool,
    pub has_internet_access: bool,
    pub admin_tools: Vec<String>,
    pub default_tools: Vec<String>,
}

impl Default for AgentCapabilities {
    fn default() -> Self {
        Self {
            admin_users: vec![],
            has_terminal_access: false,
            has_internet_access: false,
            admin_tools: vec![],
            default_tools: vec![],
        }
    }
}

impl AgentCapabilities {
    pub fn format_for_prompt(&self, event: &crate::models::message::Event) -> String {
        let is_admin = self.admin_users.contains(&event.author_id);

        let terminal = if is_admin && self.has_terminal_access { "ENABLED" } else { "DISABLED" };
        let internet = if is_admin && self.has_internet_access { "ENABLED" } else { "DISABLED" };
        
        let mut active_tools = self.default_tools.clone();
        if is_admin {
            active_tools.extend(self.admin_tools.iter().cloned());
        }

        let tools = if active_tools.is_empty() {
            "NONE".to_string()
        } else {
            active_tools.join(", ")
        };

        format!(
            "- TERMINAL/SYSTEM/BASH EXECUTION: {}\n- INTERNET/WEB ACCESS: {}\n- ACTIVE PLUGINS/TOOLS: {}",
            terminal, internet, tools
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::message::Event;
    use crate::models::scope::Scope;

    fn get_dummy_event(uid: &str) -> Event {
        Event {
            platform: "test".into(),
            scope: Scope::Public { channel_id: "test".into(), user_id: "test".into() },
            author_name: "User".into(),
            author_id: uid.to_string(),
            content: "Ping".into(),
        }
    }

    #[test]
    fn test_default_capabilities() {
        let caps = AgentCapabilities::default();
        assert!(!caps.has_terminal_access);
        assert!(!caps.has_internet_access);
        assert!(caps.admin_tools.is_empty());
        assert!(caps.default_tools.is_empty());
    }

    #[test]
    fn test_format_all_disabled() {
        let caps = AgentCapabilities::default();
        let ev = get_dummy_event("user1");
        let output = caps.format_for_prompt(&ev);
        assert!(output.contains("TERMINAL/SYSTEM/BASH EXECUTION: DISABLED"));
        assert!(output.contains("INTERNET/WEB ACCESS: DISABLED"));
        assert!(output.contains("ACTIVE PLUGINS/TOOLS: NONE"));
    }

    #[test]
    fn test_format_with_tools_enabled_admin() {
        let caps = AgentCapabilities {
            admin_users: vec!["admin1".into()],
            has_terminal_access: true,
            has_internet_access: false,
            admin_tools: vec!["wipe_memory".into()],
            default_tools: vec!["read_code".into()],
        };
        
        let admin_ev = get_dummy_event("admin1");
        let output = caps.format_for_prompt(&admin_ev);
        assert!(output.contains("TERMINAL/SYSTEM/BASH EXECUTION: ENABLED"));
        assert!(output.contains("read_code, wipe_memory"));

        let user_ev = get_dummy_event("user1");
        let output2 = caps.format_for_prompt(&user_ev);
        assert!(output2.contains("TERMINAL/SYSTEM/BASH EXECUTION: DISABLED"));
        assert!(output2.contains("read_code"));
        assert!(!output2.contains("wipe_memory"));
    }
}
