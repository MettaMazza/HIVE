/// InboxManager — per-user async message queue for proactive messaging.
///
/// Users can set a priority level:
///   notify  → Apis actively DMs the user when a message arrives
///   normal  → message sits silently until user checks /inbox  
///   mute    → messages from Apis are blocked entirely
///
/// Stored at: memory/inbox/{user_id}.json
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use chrono::Utc;

const MAX_INBOX_SIZE: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum InboxPriority {
    Notify,
    #[default]
    Normal,
    Mute,
}

impl std::fmt::Display for InboxPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InboxPriority::Notify => write!(f, "notify"),
            InboxPriority::Normal => write!(f, "normal"),
            InboxPriority::Mute => write!(f, "mute"),
        }
    }
}

impl std::str::FromStr for InboxPriority {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "notify" => Ok(Self::Notify),
            "normal" => Ok(Self::Normal),
            "mute" => Ok(Self::Mute),
            other => Err(format!("Unknown priority: {}", other)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxMessage {
    pub id: String,
    pub content: String,
    pub timestamp: String,
    pub read: bool,
    pub priority: InboxPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct InboxData {
    messages: Vec<InboxMessage>,
    priority: InboxPriority,
}


fn inbox_path(project_root: &str, user_id: &str) -> PathBuf {
    PathBuf::from(project_root)
        .join("memory/inbox")
        .join(format!("{}.json", user_id))
}

fn make_id(user_id: &str) -> String {
    let input = format!("{}:{}", user_id, Utc::now().to_rfc3339());
    let hash = Sha256::digest(input.as_bytes());
    format!("{:x}", hash)[..12].to_string()
}

pub struct InboxManager {
    project_root: String,
}

impl InboxManager {
    pub fn new(project_root: &str) -> Self {
        Self { project_root: project_root.to_string() }
    }

    fn load(&self, user_id: &str) -> InboxData {
        let path = inbox_path(&self.project_root, user_id);
        if path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&path) {
                if let Ok(data) = serde_json::from_str::<InboxData>(&raw) {
                    return data;
                }
            }
        }
        InboxData::default()
    }

    fn save(&self, user_id: &str, mut data: InboxData) {
        // Cap at MAX_INBOX_SIZE
        if data.messages.len() > MAX_INBOX_SIZE {
            let drain = data.messages.len() - MAX_INBOX_SIZE;
            data.messages.drain(0..drain);
        }
        let path = inbox_path(&self.project_root, user_id);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&data) {
            let _ = std::fs::write(path, json);
        }
    }

    /// Queue a message. Returns None if user has set priority=mute.
    pub fn add_message(&self, user_id: &str, content: &str) -> Option<InboxMessage> {
        let mut data = self.load(user_id);
        if data.priority == InboxPriority::Mute {
            tracing::info!("[Inbox] Dropping message for {} (muted)", user_id);
            return None;
        }
        let msg = InboxMessage {
            id: make_id(user_id),
            content: content.chars().take(10_000).collect(),
            timestamp: Utc::now().to_rfc3339(),
            read: false,
            priority: data.priority.clone(),
        };
        data.messages.push(msg.clone());
        self.save(user_id, data);
        tracing::info!("[Inbox] Message queued for {}", user_id);
        Some(msg)
    }

    pub fn get_unread(&self, user_id: &str) -> Vec<InboxMessage> {
        let msgs: Vec<InboxMessage> = self.load(user_id)
            .messages
            .into_iter()
            .filter(|m| !m.read)
            .collect();
        tracing::debug!("[ENGINE:Inbox] get_unread for user_id={}: {} unread messages", user_id, msgs.len());
        msgs
    }

    pub fn get_all(&self, user_id: &str) -> Vec<InboxMessage> {
        self.load(user_id).messages
    }

    pub fn mark_read(&self, user_id: &str, msg_id: &str) -> bool {
        let mut data = self.load(user_id);
        let mut found = false;
        for m in &mut data.messages {
            if m.id == msg_id {
                m.read = true;
                found = true;
                break;
            }
        }
        if found {
            tracing::debug!("[ENGINE:Inbox] Marked message {} as read for user_id={}", msg_id, user_id);
            self.save(user_id, data);
        } else {
            tracing::debug!("[ENGINE:Inbox] mark_read: message {} not found for user_id={}", msg_id, user_id);
        }
        found
    }

    pub fn mark_all_read(&self, user_id: &str) -> usize {
        let mut data = self.load(user_id);
        let mut count = 0;
        for m in &mut data.messages {
            if !m.read { m.read = true; count += 1; }
        }
        self.save(user_id, data);
        tracing::debug!("[ENGINE:Inbox] Marked {} messages as read for user_id={}", count, user_id);
        count
    }

    pub fn set_priority(&self, user_id: &str, level: InboxPriority) -> String {
        let mut data = self.load(user_id);
        let emoji = match &level {
            InboxPriority::Notify => "🔔",
            InboxPriority::Normal => "📬",
            InboxPriority::Mute => "🔇",
        };
        let priority_label = level.to_string();
        tracing::debug!("[ENGINE:Inbox] Setting priority for user_id={} to '{}'", user_id, priority_label);
        data.priority = level;
        self.save(user_id, data);
        format!("{} Inbox priority set to **{}**.", emoji, priority_label)
    }

    pub fn get_priority(&self, user_id: &str) -> InboxPriority {
        self.load(user_id).priority
    }

    /// Human-readable unread summary.
    pub fn get_summary(&self, user_id: &str) -> String {
        let unread = self.get_unread(user_id);
        if unread.is_empty() {
            return "📭 Inbox empty — no unread messages.".to_string();
        }
        let total = unread.len();
        let preview: Vec<String> = unread.iter().take(5).map(|m| {
            let short = if m.content.len() > 80 {
                format!("{}…", &m.content[..80])
            } else {
                m.content.clone()
            };
            format!("• [{}] {}", m.timestamp.get(..16).unwrap_or(""), short)
        }).collect();
        format!(
            "📬 **{} unread message{}:**\n{}\n{}",
            total,
            if total == 1 { "" } else { "s" },
            preview.join("\n"),
            if total > 5 { format!("*…and {} more.*", total - 5) } else { String::new() }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mgr() -> InboxManager {
        let dir = format!("/tmp/hive_inbox_test_{}", std::process::id());
        InboxManager::new(&dir)
    }

    #[test]
    fn test_add_and_get_unread() {
        let m = mgr();
        m.add_message("u1", "Hello from Apis");
        let unread = m.get_unread("u1");
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].content, "Hello from Apis");
    }

    #[test]
    fn test_mute_drops_message() {
        let m = mgr();
        m.set_priority("u2", InboxPriority::Mute);
        let result = m.add_message("u2", "should be dropped");
        assert!(result.is_none());
        assert_eq!(m.get_unread("u2").len(), 0);
    }

    #[test]
    fn test_mark_read() {
        let m = mgr();
        m.add_message("u3", "test");
        let unread = m.get_unread("u3");
        assert_eq!(unread.len(), 1);
        m.mark_read("u3", &unread[0].id);
        assert_eq!(m.get_unread("u3").len(), 0);
    }

    #[test]
    fn test_mark_all_read() {
        let m = mgr();
        m.add_message("u4", "msg1");
        m.add_message("u4", "msg2");
        let count = m.mark_all_read("u4");
        assert_eq!(count, 2);
        assert!(m.get_unread("u4").is_empty());
    }

    #[test]
    fn test_priority_set_and_get() {
        let m = mgr();
        m.set_priority("u5", InboxPriority::Notify);
        assert_eq!(m.get_priority("u5"), InboxPriority::Notify);
    }

    #[test]
    fn test_summary_empty() {
        let m = mgr();
        let s = m.get_summary("u_empty");
        assert!(s.contains("empty"));
    }

    #[test]
    fn test_summary_with_messages() {
        let m = mgr();
        m.add_message("u6", "Hello!");
        let s = m.get_summary("u6");
        assert!(s.contains("unread"));
    }
}
