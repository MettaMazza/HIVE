#![allow(clippy::collapsible_if)]
use std::sync::Arc;
use tokio::sync::RwLock;
use std::path::PathBuf;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::models::message::Event;
use crate::models::scope::Scope;

/// Tier 1: Working Memory
/// Represents the immediate, persistent cross-session context.
/// Tracks token count to trigger autosaves when approaching the context window limit.
#[derive(Debug, Clone)]
pub struct WorkingMemory {
    events: Arc<RwLock<Vec<Event>>>,
    // The current estimated token count of the active transcript
    token_count: Arc<RwLock<usize>>,
    // The maximum token limit before triggering Autosave
    max_tokens: usize,
    base_dir: PathBuf,
}

impl Default for WorkingMemory {
    fn default() -> Self {
        Self::new(None)
    }
}

impl WorkingMemory {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        #[cfg(test)]
        let default_dir = std::env::temp_dir().join(format!("hive_mem_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        #[cfg(not(test))]
        let default_dir = PathBuf::from("memory");

        Self {
            events: Arc::new(RwLock::new(Vec::new())),
            token_count: Arc::new(RwLock::new(0)),
            max_tokens: 256_000,
            base_dir: base_dir.unwrap_or(default_dir),
        }
    }

    /// Gets the base directory for memory storage
    pub fn get_memory_dir(&self) -> PathBuf {
        self.base_dir.clone()
    }

    /// Gets the specific transcript file path for a scope
    fn get_transcript_path(&self, scope: &Scope) -> PathBuf {
        let mut path = self.get_memory_dir();
        match scope {
            Scope::Public { channel_id, user_id } => {
                path.push(format!("public_{}", channel_id));
                path.push(user_id);
            }
            Scope::Private { user_id } => path.push(format!("private_{}", user_id)),
        }
        path.push("transcript.jsonl");
        path
    }

    pub async fn add_event(&self, event: Event) {
        // 1. Update in-memory state
        let mut w = self.events.write().await;
        w.push(event.clone());
        
        // 2. Track estimated tokens (char count / 4 is a standard rough heuristic)
        let estimated_tokens = event.content.len() / 4;
        let mut tc = self.token_count.write().await;
        *tc += estimated_tokens;
        tracing::debug!("[MEMORY:Working] add_event: scope='{}' author='{}' content_len={} est_tokens={} total_tokens={}",
            event.scope.to_key(), event.author_name, event.content.len(), estimated_tokens, *tc);
        
        // 3. Persist to disk
        let path = self.get_transcript_path(&event.scope);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent).await;
        }
        
        if let Ok(json) = serde_json::to_string(&event) {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await 
            {
                let _ = file.write_all(format!("{}\n", json).as_bytes()).await;
            }
        }
    }

    pub async fn get_history(&self, requesting_scope: &Scope) -> Vec<Event> {
        let r = self.events.read().await;
        // Filter history based on privacy scope
        let mut history = Vec::new();
        for e in r.iter() {
            if requesting_scope.can_read(&e.scope) {
                history.push(e.clone());
            }
        }

        // HARD CAP: Retain only the most recent 100 messages (50 conversational turns).
        // Prevents unbounded System Prompt growth, KV Cache explosion, and massive latency creep.
        // Data is not permanently lost; `memory::autosave` and `memory::timeline` retain full persistence.
        if history.len() > 100 {
            let start = history.len() - 100;
            tracing::debug!("[MEMORY:Working] get_history: capping {} events to 100 (scope='{}')", history.len(), requesting_scope.to_key());
            history = history[start..].to_vec();
        }

        let ptr_addr = Arc::as_ptr(&self.events) as usize;
        tracing::info!("[MEMORY:Working] get_history: scope='{}' returning {} events (total_vec_size={}) | vec_ptr=0x{:x}", requesting_scope.to_key(), history.len(), r.len(), ptr_addr);
        history
    }
    
    pub async fn get_absolute_count(&self, requesting_scope: &Scope) -> usize {
        let r = self.events.read().await;
        r.iter().filter(|e| requesting_scope.can_read(&e.scope)).count()
    }
    
    pub async fn current_tokens(&self) -> usize {
        *self.token_count.read().await
    }

    /// Returns a clone of all events in working memory (scope-unfiltered).
    pub async fn get_all_events(&self) -> Vec<Event> {
        self.events.read().await.clone()
    }
    
    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }
    
    /// Scans the memory directory and loads all existing transcript histories into active memory.
    pub async fn load_persisted(&self) {
        let memory_dir = self.get_memory_dir();
        
        let mut dirs = match tokio::fs::read_dir(&memory_dir).await {
            Ok(d) => d,
            Err(_) => return, // No memory dir exists yet
        };

        let mut all_events = Vec::new();
        let mut total_tokens = 0;

        while let Ok(Some(entry)) = dirs.next_entry().await {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path.file_name().unwrap_or_default().to_string_lossy();
                
                let mut transcript_paths = Vec::new();

                if dir_name.starts_with("public_") {
                    if let Ok(mut subdirs) = tokio::fs::read_dir(&path).await {
                        while let Ok(Some(sub_entry)) = subdirs.next_entry().await {
                            if sub_entry.path().is_dir() {
                                transcript_paths.push(sub_entry.path().join("transcript.jsonl"));
                            }
                        }
                    }
                } else if dir_name.starts_with("private_") {
                    transcript_paths.push(path.join("transcript.jsonl"));
                }
                
                for t_path in transcript_paths {
                    if let Ok(contents) = tokio::fs::read_to_string(&t_path).await {
                        for line in contents.lines() {
                            let trimmed = line.trim();
                            if trimmed.is_empty() { continue; }
                            if let Ok(event) = serde_json::from_str::<crate::models::message::Event>(trimmed) {
                                total_tokens += event.content.len() / 4;
                                all_events.push(event);
                            }
                        }
                    }
                }
            }
        }

        if !all_events.is_empty() {
            let mut w = self.events.write().await;
            *w = all_events;
            
            let mut tc = self.token_count.write().await;
            *tc = total_tokens;
            
            tracing::info!("[MEMORY:Working] Loaded {} persistent memory events (approx {} tokens) across sessions.", w.len(), *tc);
        }
    }
    
    /// Clears the working memory transcript (called internally by Autosave)
    pub async fn clear(&self, scope: &Scope) {
        let mut w = self.events.write().await;
        let before_count = w.len();
        let matching_count = w.iter().filter(|e| e.scope == *scope).count();
        
        // Diagnostic: log scopes of first 5 events to see what we're comparing against
        let sample_scopes: Vec<String> = w.iter().take(5).map(|e| e.scope.to_key()).collect();
        let ptr_addr = Arc::as_ptr(&self.events) as usize;
        tracing::warn!(
            "[MEMORY:Working] CLEAR called — scope='{}' | before={} | matching={} | sample_scopes={:?} | vec_ptr=0x{:x}",
            scope.to_key(), before_count, matching_count, sample_scopes, ptr_addr
        );
        
        w.retain(|e| e.scope != *scope); // Remove events for this scope
        
        let after_count = w.len();
        tracing::warn!(
            "[MEMORY:Working] CLEAR complete — after={} | removed={}",
            after_count, before_count - after_count
        );
        
        let mut tc = self.token_count.write().await;
        // Simple reset for now. In a robust system, we'd recount the remaining events.
        *tc = 0; 
        
        let path = self.get_transcript_path(scope);
        let _ = fs::remove_file(path).await;
    }

    /// Completely wipes all RAM structures.
    pub async fn clear_all(&self) {
        tracing::debug!("[MEMORY:Working] Clearing ALL working memory");
        let mut w = self.events.write().await;
        w.clear();
        let mut tc = self.token_count.write().await;
        *tc = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_working_memory_add_and_get() {
        let test_dir = std::env::temp_dir().join(format!("hive_working_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mem = WorkingMemory::new(Some(test_dir));
        let pub_scope = Scope::Public { channel_id: "test".into(), user_id: "test".into() };
        let priv_scope = Scope::Private { user_id: "user123".to_string() };

        let event1 = Event {
            platform: "test".into(),
            scope: pub_scope.clone(),
            author_name: "Alice".into(),
            author_id: "test".into(),
            content: "Hello working memory".into(), // length 20 -> 5 tokens
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };
        
        let event2 = Event {
            platform: "test".into(),
            scope: priv_scope.clone(),
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Secret".into(), // length 6 -> 1 token
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        mem.add_event(event1).await;
        mem.add_event(event2).await;

        assert_eq!(mem.current_tokens().await, 6);
        assert_eq!(mem.max_tokens(), 256_000);

        let pub_hist = mem.get_history(&pub_scope).await;
        assert_eq!(pub_hist.len(), 1);
        assert_eq!(pub_hist[0].author_name, "Alice");

        let priv_hist = mem.get_history(&priv_scope).await;
        assert_eq!(priv_hist.len(), 1); // Now completely siloed


    }

    #[tokio::test]
    async fn test_working_memory_clear() {
        let test_dir = std::env::temp_dir().join(format!("hive_working_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mem = WorkingMemory::new(Some(test_dir));
        let scope = Scope::Public { channel_id: "test".into(), user_id: "test".into() };

        let event = Event {
            platform: "test".into(),
            scope: scope.clone(),
            author_name: "Alice".into(),
            author_id: "test".into(),
            content: "Test event".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        mem.add_event(event).await;
        assert_eq!(mem.get_history(&scope).await.len(), 1);
        assert_eq!(mem.current_tokens().await, 2);

        mem.clear(&scope).await;
        assert_eq!(mem.get_history(&scope).await.len(), 0);
        assert_eq!(mem.current_tokens().await, 0);
    }

    #[tokio::test]
    async fn test_working_memory_default() {
        let mem = WorkingMemory::default();
        // Verify default sets path to testing env in test mode, and limit to 256k
        #[cfg(test)]
        assert!(mem.get_memory_dir().to_string_lossy().contains("hive_mem_test"));
        #[cfg(not(test))]
        assert_eq!(mem.get_memory_dir(), std::path::PathBuf::from("memory"));
        assert_eq!(mem.max_tokens(), 256000);
    }

    #[tokio::test]
    async fn test_working_memory_clear_all() {
        let test_dir = std::env::temp_dir().join(format!("hive_working_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mem = WorkingMemory::new(Some(test_dir));
        let scope1 = Scope::Public { channel_id: "test".into(), user_id: "test".into() };
        let scope2 = Scope::Private { user_id: "user123".to_string() };

        mem.add_event(Event {
            platform: "test".into(),
            scope: scope1.clone(),
            author_name: "Alice".into(),
            author_id: "test".into(),
            content: "Public event".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await;

        mem.add_event(Event {
            platform: "test".into(),
            scope: scope2.clone(),
            author_name: "Bob".into(),
            author_id: "test".into(),
            content: "Private event".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }).await;

        assert_eq!(mem.current_tokens().await, 6); // 3 (Public) + 3 (Private) tokens
        
        mem.clear_all().await;
        
        assert_eq!(mem.current_tokens().await, 0);
        let r = mem.events.read().await;
        assert_eq!(r.len(), 0);
    }

    #[tokio::test]
    async fn test_working_memory_load_persisted() {
        let test_dir = std::env::temp_dir().join(format!("hive_working_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mem = WorkingMemory::new(Some(test_dir));
        let scope = Scope::Private { user_id: "load_test".into() };
        
        let path = mem.get_transcript_path(&scope);
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let event = Event {
            platform: "test".into(),
            scope: scope.clone(),
            author_name: "DiskUser".into(),
            author_id: "test".into(),
            content: "Loaded from disk".into(), // length 16 -> 4 tokens
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };
        
        let json = serde_json::to_string(&event).unwrap();
        let _ = tokio::fs::write(&path, format!("{}\n", json)).await;

        mem.load_persisted().await;

        let hist = mem.get_history(&scope).await;
        assert!(hist.iter().any(|e| e.author_name == "DiskUser"));
        assert!(mem.current_tokens().await >= 4);

        let _ = tokio::fs::remove_dir_all(path.parent().unwrap()).await;
    }

    #[tokio::test]
    async fn test_working_memory_load_persisted_no_dir() {
        let test_dir = std::env::temp_dir().join(format!("hive_working_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let _ = tokio::fs::remove_dir_all(&test_dir).await;

        let mem = WorkingMemory::new(Some(test_dir.clone()));
        // This should safely hit the `Err(_) => return` at line 107
        mem.load_persisted().await;
        assert_eq!(mem.current_tokens().await, 0);
    }

    #[tokio::test]
    async fn test_working_memory_40_msg_cap() {
        let test_dir = std::env::temp_dir().join(format!("hive_working_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mem = WorkingMemory::new(Some(test_dir.clone()));
        let scope = Scope::Private { user_id: "capper".into() };

        // Add 120 events
        for i in 0..120 {
            mem.add_event(Event {
                platform: "test".into(),
                scope: scope.clone(),
                author_name: format!("User{}", i),
                author_id: "test".into(),
                content: "ping".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
            }).await;
        }

        // Fetch history, should be capped at 100
        let hist = mem.get_history(&scope).await;
        assert_eq!(hist.len(), 100);
        // The first 20 should be truncated, so the earliest we see is User20
        assert_eq!(hist[0].author_name, "User20");

        let _ = tokio::fs::remove_dir_all(&test_dir).await;
    }
}
