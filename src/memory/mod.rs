use std::sync::Arc;
use std::path::PathBuf;
use crate::models::message::Event;
use crate::models::scope::Scope;

pub mod working;
pub mod autosave;
pub mod synaptic;
pub mod timeline;
pub mod timelines;
pub mod temporal;
pub mod scratch;
pub mod lessons;
pub mod moderation;

pub use working::*;
pub use autosave::*;
pub use synaptic::*;
pub use timeline::*;
pub use timelines::*;
pub use temporal::*;
pub use scratch::*;
pub mod preferences;
pub use preferences::*;
pub use lessons::*;
pub use moderation::*;

use std::collections::{HashMap, VecDeque};
use tokio::sync::{Mutex, RwLock};
use chrono::Utc;
use crate::computer::alu::ALU;
use crate::computer::turing_grid::TuringGrid;
/// The Unified 5-Tier Memory Store.
#[derive(Debug)]
pub struct MemoryStore {
    pub working: WorkingMemory,
    pub timeline: TimelineManager,
    pub synaptic: Neo4jGraph,
    pub scratch: Scratchpad,
    pub autosave: AutosaveManager,
    pub preferences: PreferenceStore,
    pub temporal: Arc<RwLock<TemporalTracker>>,
    pub timelines: Arc<TimelineStore>,
    pub activity_stream: Arc<RwLock<VecDeque<String>>>,
    pub lessons: LessonsManager,
    pub turing_grid: Arc<Mutex<TuringGrid>>,
    pub alu: Arc<ALU>,
    pub moderation: ModerationStore,
    /// Tracks recent active participants in Public channels. Maps channel_id -> Vec<author_name>
    rosters: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new(None)
    }
}

impl MemoryStore {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        #[cfg(test)]
        let default_dir = std::env::temp_dir().join(format!("hive_mem_test_store_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        #[cfg(not(test))]
        let default_dir = PathBuf::from("memory");

        let memory_dir = base_dir.unwrap_or(default_dir);
        let working = WorkingMemory::new(Some(memory_dir.clone()));
        let autosave = AutosaveManager::new();
        let synaptic = Neo4jGraph::new();
        let timeline = TimelineManager::new(Some(memory_dir.clone()));
        let scratch = Scratchpad::new(Some(memory_dir.clone()));
        let preferences = PreferenceStore::new(Some(memory_dir.clone()));
        let lessons = LessonsManager::new(Some(memory_dir.clone()));
        
        let temporal = Arc::new(RwLock::new(TemporalTracker::new(&memory_dir.join("core"))));
        let timelines = Arc::new(TimelineStore::new(&memory_dir.join("core")));
        let turing_grid = Arc::new(Mutex::new(TuringGrid::new(memory_dir.join("computer_grid.json"))));
        let alu = Arc::new(ALU::new(Some(memory_dir.join("computer_runtime"))));
        let moderation = ModerationStore::new();

        Self {
            working,
            timeline,
            synaptic,
            scratch,
            autosave,
            preferences,
            lessons,
            temporal,
            timelines,
            turing_grid,
            alu,
            moderation,
            activity_stream: Arc::new(RwLock::new(VecDeque::new())),
            rosters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn init(&self) {
        self.working.load_persisted().await;
        // Init grid logic:
        let grid_path = self.turing_grid.lock().await.persistence_path.clone();
        if let Ok(loaded) = TuringGrid::load(grid_path).await {
            *self.turing_grid.lock().await = loaded;
        } else {
            tracing::debug!("[Memory] 🔲 Turing Grid starting fresh (no persisted state)");
        }
        let _ = self.alu.init().await;
        self.temporal.write().await.init_and_register_boot().await;
    }

    /// Stores a new event into the primary working memory and appends to the timeline.
    pub async fn add_event(&self, event: Event) {
        // Track roster for Public scopes
        if let Scope::Public { channel_id, .. } = &event.scope {
            let mut rosters: tokio::sync::RwLockWriteGuard<'_, HashMap<String, Vec<String>>> = self.rosters.write().await;
            let channel_roster = rosters.entry(channel_id.clone()).or_default();
            
            // Add if not already present; push to end to signify recency
            // Keep the last 10 unique speakers.
            if let Some(pos) = channel_roster.iter().position(|name| name == &event.author_name) {
                // If they exist, move them to the end (most recently spoken)
                let name = channel_roster.remove(pos);
                channel_roster.push(name);
            } else {
                channel_roster.push(event.author_name.clone());
                if channel_roster.len() > 10 {
                    channel_roster.remove(0); // Pop oldest
                }
            }
        }

        let time = Utc::now().format("%H:%M:%S").to_string();

        // 0. Inject into transient Global Activity Stream (HUD Telemetry)
        if event.author_name != "System" && !event.content.starts_with("***") {
            let mut stream: tokio::sync::RwLockWriteGuard<'_, VecDeque<String>> = self.activity_stream.write().await;
            let target_line = match &event.scope {
                Scope::Public { channel_id: _, user_id: _ } => {
                    format!("[{}] [PUBLIC] {}: {}...", time, event.author_name, &event.content.chars().take(50).collect::<String>().trim().replace('\n', " "))
                }
                Scope::Private { user_id } => {
                    format!("[{}] [(Encrypted PM Header)] UID:{}", time, user_id)
                }
            };
            stream.push_back(target_line);
            if stream.len() > 10 {
                stream.pop_front();
            }
        }

        self.working.add_event(event.clone()).await;
        self.timeline.append_event(&event).await;
    }

    /// Fetches a comma-separated list of active participants in a channel
    pub async fn get_roster(&self, channel_id: &str) -> Option<String> {
        let rosters: tokio::sync::RwLockReadGuard<'_, HashMap<String, Vec<String>>> = self.rosters.read().await;
        if let Some(roster) = rosters.get(channel_id) {
            if roster.is_empty() {
                None
            } else {
                Some(roster.join(", "))
            }
        } else {
            None
        }
    }    
    /// Retrieves the short-term working history for the requesting scope.
    pub async fn get_working_history(&self, requesting_scope: &Scope) -> Vec<Event> {
        self.working.get_history(requesting_scope).await
    }

    /// Checks if the active working memory has breached the token limit.
    /// If so, it triggers an archive event and injects a Continuity Summary.
    pub async fn check_and_trigger_autosave(&self, scope: &Scope) -> Option<Event> {
        let current_tokens = self.working.current_tokens().await;
        if current_tokens >= self.working.max_tokens() {
            println!("Context window limit reached ({} tokens). Triggering Autosave...", current_tokens);
            
            // In a full implementation, AutosaveManager reads the file and calls the LLM.
            // For now, we simulate the LLM summarization.
            let dummy_path = std::path::PathBuf::from("dummy");
            if let Ok((title, summary)) = self.autosave.archive_transcript(scope, dummy_path).await {
                
                // Clear the working memory for this scope
                self.working.clear(scope).await;

                // Create a Continuity Event to seed the fresh transcript
                let continuity_event = Event {
                    platform: "system:memory".to_string(),
                    scope: scope.clone(),
                    author_name: "System".to_string(),
                    author_id: "test".into(),
                    content: format!(
                        "*** CONTINUITY SUMMARY ***\n\n\
                        The previous session hit the maximum context limit and was archived under the title: [{}].\n\n\
                        Summary of recent events:\n{}\n\n\
                        Directions: If you need to recall older details, use your memory search tools to retrieve '{}'.\n\n\
                        You are now operating in a fresh context window. Continue seamlessly.",
                        title, summary, title
                    ),
                };
                
                // Inject the continuity event into the fresh working memory AND timeline
                self.timeline.append_event(&continuity_event).await;
                self.working.add_event(continuity_event.clone()).await;
                
                return Some(continuity_event);
            }
        }
        None
    }

    /// Triggers a total factory reset of the memory system (Private, Public, Timelines, etc.)
    pub async fn wipe_all(&self) {
        // 1. Wipe Active RAM State
        self.working.clear_all().await;
        self.rosters.write().await.clear();

        // 2. Wipe Physical Hard Drive Backups
        let dir = self.working.get_memory_dir();
        if let Err(e) = tokio::fs::remove_dir_all(&dir).await {
            eprintln!("⚠️ Failed standard delete of {:?}: {}. Attempting force wipe...", dir, e);
            let _ = std::process::Command::new("rm").arg("-rf").arg(&dir).status();
            // Re-create the dir immediately so it's fresh without hitting Not Found errs
            let _ = tokio::fs::create_dir_all(&dir).await;
        }
        
        println!("⚠️ FACTORY RESET EXECUTED: All persistent memory has been wiped.");
    }    
    /// Builds a compact narrative of all public interactions currently in working memory.
    /// Used to give Apis context about her day's work when entering autonomy mode.
    pub async fn get_public_narrative(&self) -> String {
        let events = self.working.get_all_events().await;
        let mut users_seen = std::collections::HashSet::new();
        let mut conversations = Vec::new();

        for e in events.iter() {
            if let Scope::Public { .. } = &e.scope {
                // Skip internal events
                if e.author_name.contains("Internal") || e.author_name == "System" {
                    continue;
                }
                if e.author_name != "Apis" {
                    users_seen.insert(e.author_name.clone());
                }
                conversations.push(format!("[{}]: {}", e.author_name, e.content.trim()));
            }
        }

        if conversations.is_empty() {
            return "No public interactions recorded in current session.".to_string();
        }

        let mut narrative = String::new();
        narrative.push_str("📋 **Public Engagement Log**\n");
        narrative.push_str(&format!("• Users engaged: {}\n\n", users_seen.into_iter().collect::<Vec<_>>().join(", ")));
        narrative.push_str("**Full Conversation History:**\n");
        for line in &conversations {
            narrative.push_str(&format!("{}\n", line));
        }

        narrative
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    #[tokio::test]
    async fn test_memorystore_default() {
        let store = MemoryStore::default();
        let store2 = MemoryStore::default();
        // Just verify it doesn't panic
        assert_eq!(store.working.max_tokens(), 256000);
        assert_eq!(store2.working.max_tokens(), 256000);
    }

    #[tokio::test]
    async fn test_memorystore_add_and_get() {
        let store = MemoryStore::default();
        let pub_scope = Scope::Public { channel_id: "test".into(), user_id: "test".into() };

        let event = Event {
            platform: "test".into(),
            scope: pub_scope.clone(),
            author_name: "User".into(),
            author_id: "test".into(),
            content: "Ping".into(),
        };

        store.add_event(event).await;
        
        // Verify working memory handles it
        let hist = store.get_working_history(&pub_scope).await;
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].content, "Ping");

        // Timeline handles it internally (covered by timeline.rs tests)


    }

    #[tokio::test]
    async fn test_check_and_trigger_autosave() {

        let store = MemoryStore::default();
        let scope = Scope::Public { channel_id: "test".into(), user_id: "test".into() };

        // Force working memory to breach 256k limit by setting max to 0 directly or adding a giant event
        // Since max_tokens is private, we will just add a big fake token count or loop
        // Alternatively, add a massive event that exceeds 256k chars / 4
        let giant_content = "A".repeat(1_025_000); // > 256,000 tokens

        let event = Event {
            platform: "test".into(),
            scope: scope.clone(),
            author_name: "User".into(),
            author_id: "test".into(),
            content: giant_content,
        };

        store.add_event(event).await;

        // Trigger autosave
        let continuity = store.check_and_trigger_autosave(&scope).await;
        assert!(continuity.is_some());

        let ce = continuity.unwrap();
        assert_eq!(ce.author_name, "System");
        assert!(ce.content.contains("*** CONTINUITY SUMMARY ***"));

        // Verify working memory was cleared except for the continuity event
        let hist = store.get_working_history(&scope).await;
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].content, ce.content);


    }

    #[tokio::test]
    async fn test_memorystore_wipe_all() {
        let test_dir = std::env::temp_dir().join(format!("hive_store_wipe_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = MemoryStore::new(Some(test_dir.clone()));
        // Since get_memory_dir is hardcoded to "memory" here, we'll let it create it then wipe it.
        let pub_scope = Scope::Public { channel_id: "test".into(), user_id: "test".into() };
        let event = Event {
            platform: "test".into(),
            scope: pub_scope.clone(),
            author_name: "User".into(),
            author_id: "test".into(),
            content: "Test factory wipe".into(),
        };

        store.add_event(event).await;
        
        // Ensure it has data
        assert_eq!(store.get_working_history(&pub_scope).await.len(), 1);
        
        // Wipe it all
        store.wipe_all().await;
        
        // Verify WorkingMemory RAM is cleared
        assert_eq!(store.get_working_history(&pub_scope).await.len(), 0);
        
        // Verify working directory was unlinked
        assert!(!test_dir.exists());
    }

    #[tokio::test]
    async fn test_roster_speaker_reorder() {
        let test_dir = std::env::temp_dir().join(format!("hive_mem_test_roster_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = MemoryStore::new(Some(test_dir.clone()));
        let s = Scope::Public { channel_id: "reorder".into(), user_id: "u".into() };
        store.add_event(Event { platform: "t".into(), scope: s.clone(), author_name: "Alice".into(), author_id: "a".into(), content: "1".into() }).await;
        store.add_event(Event { platform: "t".into(), scope: s.clone(), author_name: "Bob".into(), author_id: "b".into(), content: "2".into() }).await;
        store.add_event(Event { platform: "t".into(), scope: s.clone(), author_name: "Alice".into(), author_id: "a".into(), content: "3".into() }).await;
        assert_eq!(store.get_roster("reorder").await.unwrap(), "Bob, Alice");
    }

    #[tokio::test]
    async fn test_roster_overflow() {
        let test_dir = std::env::temp_dir().join(format!("hive_mem_test_of_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = MemoryStore::new(Some(test_dir.clone()));
        let s = Scope::Public { channel_id: "of".into(), user_id: "u".into() };
        for i in 0..11 {
            store.add_event(Event { platform: "t".into(), scope: s.clone(), author_name: format!("U{}", i), author_id: format!("{}", i), content: "m".into() }).await;
        }
        let r = store.get_roster("of").await.unwrap();
        assert!(!r.contains("U0"));
        assert!(r.contains("U10"));
    }

    #[tokio::test]
    async fn test_roster_none_for_missing() {
        let test_dir = std::env::temp_dir().join(format!("hive_mem_test_mnfm_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = MemoryStore::new(Some(test_dir.clone()));
        assert_eq!(store.get_roster("nope").await, None);
    }

    #[tokio::test]
    async fn test_private_no_roster() {
        let test_dir = std::env::temp_dir().join(format!("hive_mem_test_pnr_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = MemoryStore::new(Some(test_dir.clone()));
        let s = Scope::Private { user_id: "dm".into() };
        store.add_event(Event { platform: "t".into(), scope: s, author_name: "A".into(), author_id: "a".into(), content: "m".into() }).await;
        assert_eq!(store.get_roster("dm").await, None);
    }

    #[tokio::test]
    async fn test_init() {
        let test_dir = std::env::temp_dir().join(format!("hive_mem_test_init_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = MemoryStore::new(Some(test_dir.clone()));
        store.init().await;
    }

    #[tokio::test]
    async fn test_autosave_under_limit() {
        let test_dir = std::env::temp_dir().join(format!("hive_mem_test_asul_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = MemoryStore::new(Some(test_dir.clone()));
        let s = Scope::Public { channel_id: "t".into(), user_id: "u".into() };
        store.add_event(Event { platform: "t".into(), scope: s.clone(), author_name: "U".into(), author_id: "u".into(), content: "Small".into() }).await;
        assert!(store.check_and_trigger_autosave(&s).await.is_none());
    }
}
