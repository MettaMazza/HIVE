#![allow(clippy::collapsible_if)]
use std::sync::Arc;
use tokio::sync::RwLock;
use std::path::PathBuf;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::models::message::Event;
use crate::models::scope::Scope;

/// Tier 4: Timeline Memory
/// A complete chronological list of all actions, tool uses, and interactions.
#[derive(Debug, Clone)]
pub struct TimelineManager {
    // In-memory buffer for timeline events before flushing to disk.
    events: Arc<RwLock<Vec<Event>>>,
    base_dir: PathBuf,
}

impl Default for TimelineManager {
    fn default() -> Self {
        Self::new(None)
    }
}

impl TimelineManager {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        #[cfg(test)]
        let default_dir = std::env::temp_dir().join(format!("hive_mem_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        #[cfg(not(test))]
        let default_dir = PathBuf::from("memory");

        Self {
            events: Arc::new(RwLock::new(Vec::new())),
            base_dir: base_dir.unwrap_or(default_dir),
        }
    }

    /// Gets the base directory for memory storage
    fn get_memory_dir(&self) -> PathBuf {
        self.base_dir.clone()
    }

    /// Gets the specific timeline file path for a scope
    fn get_timeline_path(&self, scope: &Scope) -> PathBuf {
        let mut path = self.get_memory_dir();
        match scope {
            Scope::Public { channel_id, user_id } => {
                path.push(format!("public_{}", channel_id));
                path.push(user_id);
            }
            Scope::Private { user_id } => path.push(format!("private_{}", user_id)),
        }
        path.push("timeline.jsonl");
        path
    }

    pub async fn append_event(&self, event: &Event) {
        tracing::trace!("[MEMORY:Timeline] append_event: scope='{}' author='{}' content_len={}",
            event.scope.to_key(), event.author_name, event.content.len());
        // 1. Update in-memory state
        let mut w = self.events.write().await;
        w.push(event.clone());
        
        // 2. Persist to disk (Append-only)
        let path = self.get_timeline_path(&event.scope);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent).await;
        }
        
        if let Ok(json) = serde_json::to_string(&event) {
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path).await {
                let _ = file.write_all(format!("{}\n", json).as_bytes()).await;
            }
        }
    }

    /// Reads the raw JSONL timeline data from disk
    pub async fn read_timeline(&self, scope: &Scope) -> std::io::Result<Vec<u8>> {
        let path = self.get_timeline_path(scope);
        if !path.exists() {
            tracing::trace!("[MEMORY:Timeline] read_timeline: no file for scope='{}'", scope.to_key());
            return Ok(Vec::new());
        }
        let data = tokio::fs::read(path).await?;
        tracing::debug!("[MEMORY:Timeline] read_timeline: scope='{}' bytes={}", scope.to_key(), data.len());
        Ok(data)
    }

    pub async fn get_formatted_hud(&self) -> String {
        let len = self.events.read().await.len();
        format!("### Temporal Awareness\nCurrent System Time: {}\nTimeline Event Depth: {}", chrono::Utc::now().to_rfc3339(), len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::message::Event; // Assuming Event is in crate::models::message
    use crate::models::scope::Scope;   // Assuming Scope is in crate::models::scope
    use tokio::fs;

    #[tokio::test]
    async fn test_timeline_append_and_read() {
        let test_dir = std::env::temp_dir().join(format!("hive_timeline_multi_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let timeline_manager = TimelineManager::new(Some(test_dir));

        let unique_pub = format!("timeline_pub_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let unique_priv = format!("timeline_priv_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

        let pub_scope = Scope::Private { user_id: unique_pub.clone() };
        let priv_scope = Scope::Private { user_id: unique_priv.clone() };

        let event1 = Event {
            platform: "cli".into(),
            scope: pub_scope.clone(),
            author_name: "Alice".into(),
            author_id: "test".into(),
            content: "Hello".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        let event2 = Event {
            platform: "discord".into(),
            scope: priv_scope.clone(),
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Secret".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        timeline_manager.append_event(&event1).await;
        timeline_manager.append_event(&event2).await;

        // Verify in-memory state
        let in_memory_events = timeline_manager.events.read().await;
        assert_eq!(in_memory_events.len(), 2);
        assert_eq!(in_memory_events[0].content, "Hello");
        assert_eq!(in_memory_events[1].content, "Secret");

        // Verify persistence for public scope
        let public_path = timeline_manager.get_timeline_path(&Scope::Private { user_id: unique_pub.clone() });
        assert!(public_path.exists());
        let public_content = tokio::fs::read_to_string(&public_path).await.unwrap();
        let lines: Vec<&str> = public_content.trim().split('\n').collect();
        assert_eq!(lines.len(), 1);
        let parsed_event1: Event = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed_event1.content, "Hello");
        assert_eq!(parsed_event1.scope, pub_scope);

        // Verify persistence for private scope
        let private_path = timeline_manager.get_timeline_path(&Scope::Private { user_id: unique_priv.clone() });
        assert!(private_path.exists());
        let private_content = tokio::fs::read_to_string(&private_path).await.unwrap();
        let lines: Vec<&str> = private_content.trim().split('\n').collect();
        assert_eq!(lines.len(), 1);
        let parsed_event2: Event = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed_event2.content, "Secret");
    }

    #[tokio::test]
    async fn test_timeline_empty_on_new() {
        let test_dir = std::env::temp_dir().join(format!("hive_timeline_multi_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let timeline_manager = TimelineManager::new(Some(test_dir));
        let timeline_default = TimelineManager::default();
        
        let in_memory_events = timeline_manager.events.read().await;
        let in_default_events = timeline_default.events.read().await;
        
        assert!(in_memory_events.is_empty());
        assert!(in_default_events.is_empty());
        
    }

    #[tokio::test]
    async fn test_timeline_multiple_appends_to_same_file() {
        let test_dir = std::env::temp_dir().join(format!("hive_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let timeline_manager = TimelineManager::new(Some(test_dir.clone()));
        let scope = Scope::Private { user_id: "userX".to_string() };

        let event1 = Event {
            platform: "cli".into(),
            scope: scope.clone(),
            author_name: "Alice".into(),
            author_id: "test".into(),
            content: "Msg1".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        let event2 = Event {
            platform: "cli".into(),
            scope: scope.clone(),
            author_name: "Alice".into(),
            author_id: "test".into(),
            content: "Msg2".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        timeline_manager.append_event(&event1).await;
        timeline_manager.append_event(&event2).await;

        // Verify in-memory
        let in_memory_events = timeline_manager.events.read().await;
        assert_eq!(in_memory_events.len(), 2);
        assert_eq!(in_memory_events[0].content, "Msg1");
        assert_eq!(in_memory_events[1].content, "Msg2");

        // Verify file content
        let path = timeline_manager.get_timeline_path(&scope);
        assert!(path.exists());
        let content = fs::read_to_string(&path).await.unwrap();
        let lines: Vec<&str> = content.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);

        let parsed_event1: Event = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed_event1.content, "Msg1");

        let parsed_event2: Event = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(parsed_event2.content, "Msg2");
    }

    #[tokio::test]
    async fn test_timeline_read_missing_file_and_hud() {
        let test_dir = std::env::temp_dir().join(format!("hive_timeline_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let manager = TimelineManager::new(Some(test_dir.clone()));
        let scope = Scope::Private { user_id: "missing".into() };

        let data = manager.read_timeline(&scope).await.unwrap();
        assert!(data.is_empty());

        let hud = manager.get_formatted_hud().await;
        assert!(hud.contains("Timeline Event Depth: 0"));

        manager.append_event(&Event {
            platform: "test".into(),
            scope: scope.clone(),
            author_name: "test".into(),
            author_id: "test".into(),
            content: "test".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await;

        let hud2 = manager.get_formatted_hud().await;
        assert!(hud2.contains("Timeline Event Depth: 1"));
    }
}
