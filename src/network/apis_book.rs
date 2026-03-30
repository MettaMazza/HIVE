/// Apis-Book — Read-only feed of NeuroLease mesh activity.
///
/// ONE-WAY MIRROR: Humans can observe but NEVER inject into the AI mesh.
/// All writes come from the mesh message handler. This module exposes only
/// read methods. No mutation API exists for external callers.
///
/// Displays AI chat streams, knowledge sync events, code patches, weight
/// exchanges, and governance decisions in a social-media-style feed.
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use serde::{Deserialize, Serialize};

/// Event types visible in the Apis-Book feed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApisBookEventType {
    AiChat,
    LessonShared,
    SynapticMerge,
    WeightExchange,
    CodePatch,
    PeerJoined,
    PeerLeft,
    GovernanceVote,
    EmergencyAlert,
}

impl std::fmt::Display for ApisBookEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AiChat => write!(f, "🐝 AI Chat"),
            Self::LessonShared => write!(f, "📚 Lesson Shared"),
            Self::SynapticMerge => write!(f, "🧠 Synaptic Merge"),
            Self::WeightExchange => write!(f, "⚖️ Weight Exchange"),
            Self::CodePatch => write!(f, "🔧 Code Patch"),
            Self::PeerJoined => write!(f, "🟢 Peer Joined"),
            Self::PeerLeft => write!(f, "🔴 Peer Left"),
            Self::GovernanceVote => write!(f, "🗳️ Governance Vote"),
            Self::EmergencyAlert => write!(f, "🚨 Emergency Alert"),
        }
    }
}

/// A single entry in the Apis-Book feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApisBookEntry {
    pub id: String,
    pub timestamp: String,
    pub event_type: ApisBookEventType,
    pub peer_name: String,
    pub peer_id_short: String,
    pub content: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

impl ApisBookEntry {
    /// Create a new feed entry.
    pub fn new(
        event_type: ApisBookEventType,
        peer_name: &str,
        peer_id: &str,
        content: &str,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            event_type,
            peer_name: peer_name.to_string(),
            peer_id_short: peer_id.chars().take(8).collect(),
            content: content.to_string(),
            metadata: serde_json::Value::Null,
        }
    }

    /// Create with additional metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// The Apis-Book — read-only feed of NeuroLease mesh activity.
pub struct ApisBook {
    /// Feed entries (ring buffer, newest at back)
    feed: Arc<RwLock<VecDeque<ApisBookEntry>>>,
    /// Max entries retained
    max_entries: usize,
    /// Broadcast channel for live updates (SSE/WebSocket subscribers)
    live_tx: broadcast::Sender<ApisBookEntry>,
    /// Whether the Apis-Book is enabled
    pub enabled: bool,
}

impl ApisBook {
    /// Initialize from environment.
    pub fn new() -> Self {
        let enabled = std::env::var("HIVE_APIS_BOOK_ENABLED")
            .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true); // Enabled by default

        let max_entries: usize = std::env::var("HIVE_APIS_BOOK_MAX_ENTRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000);

        let (live_tx, _) = broadcast::channel(256);

        if enabled {
            tracing::info!("[APIS-BOOK] 📖 One-way mirror enabled (max {} entries)", max_entries);
        }

        Self {
            feed: Arc::new(RwLock::new(VecDeque::with_capacity(max_entries))),
            max_entries,
            live_tx,
            enabled,
        }
    }

    /// Push a new entry into the feed.
    /// Called by the mesh message handler — this is the ONLY write path.
    pub async fn push(&self, entry: ApisBookEntry) {
        if !self.enabled {
            return;
        }

        tracing::debug!("[APIS-BOOK] {} by {} — {}", entry.event_type, entry.peer_name,
            &entry.content[..entry.content.len().min(80)]);

        // Broadcast to live subscribers (SSE/WebSocket)
        let _ = self.live_tx.send(entry.clone());

        // Store in ring buffer
        let mut feed = self.feed.write().await;
        if feed.len() >= self.max_entries {
            feed.pop_front();
        }
        feed.push_back(entry);
    }

    /// Get the most recent N entries (newest first).
    pub async fn recent(&self, limit: usize) -> Vec<ApisBookEntry> {
        let feed = self.feed.read().await;
        feed.iter().rev().take(limit).cloned().collect()
    }

    /// Get entries filtered by event type (newest first).
    pub async fn filter_by_type(&self, event_type: &ApisBookEventType, limit: usize) -> Vec<ApisBookEntry> {
        let feed = self.feed.read().await;
        feed.iter()
            .rev()
            .filter(|e| &e.event_type == event_type)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get total entry count.
    pub async fn count(&self) -> usize {
        self.feed.read().await.len()
    }

    /// Subscribe to live updates. Returns a broadcast receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<ApisBookEntry> {
        self.live_tx.subscribe()
    }

    /// Get stats for the dashboard.
    pub async fn stats(&self) -> serde_json::Value {
        let feed = self.feed.read().await;
        let mut type_counts = std::collections::HashMap::new();
        for entry in feed.iter() {
            *type_counts.entry(format!("{}", entry.event_type)).or_insert(0u64) += 1;
        }

        serde_json::json!({
            "total_entries": feed.len(),
            "max_entries": self.max_entries,
            "type_distribution": type_counts,
            "oldest": feed.front().map(|e| e.timestamp.clone()),
            "newest": feed.back().map(|e| e.timestamp.clone()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_entry(event_type: ApisBookEventType, name: &str, content: &str) -> ApisBookEntry {
        ApisBookEntry::new(event_type, name, "abcdef1234567890", content)
    }

    #[tokio::test]
    async fn test_push_and_recent() {
        let book = ApisBook {
            feed: Arc::new(RwLock::new(VecDeque::new())),
            max_entries: 100,
            live_tx: broadcast::channel(16).0,
            enabled: true,
        };

        book.push(test_entry(ApisBookEventType::AiChat, "Apis-A", "Hello from instance A!")).await;
        book.push(test_entry(ApisBookEventType::LessonShared, "Apis-B", "Rust ownership is a lesson.")).await;

        let recent = book.recent(10).await;
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].peer_name, "Apis-B"); // Newest first
    }

    #[tokio::test]
    async fn test_filter_by_type() {
        let book = ApisBook {
            feed: Arc::new(RwLock::new(VecDeque::new())),
            max_entries: 100,
            live_tx: broadcast::channel(16).0,
            enabled: true,
        };

        book.push(test_entry(ApisBookEventType::AiChat, "A", "chat 1")).await;
        book.push(test_entry(ApisBookEventType::LessonShared, "B", "lesson 1")).await;
        book.push(test_entry(ApisBookEventType::AiChat, "C", "chat 2")).await;

        let chats = book.filter_by_type(&ApisBookEventType::AiChat, 10).await;
        assert_eq!(chats.len(), 2);
        assert_eq!(chats[0].content, "chat 2"); // Newest first
    }

    #[tokio::test]
    async fn test_ring_buffer() {
        let book = ApisBook {
            feed: Arc::new(RwLock::new(VecDeque::new())),
            max_entries: 3,
            live_tx: broadcast::channel(16).0,
            enabled: true,
        };

        for i in 0..5 {
            book.push(test_entry(ApisBookEventType::PeerJoined, &format!("P{}", i), &format!("joined {}", i))).await;
        }

        assert_eq!(book.count().await, 3);
        let recent = book.recent(10).await;
        assert_eq!(recent[0].content, "joined 4"); // Only last 3 remain
    }

    #[tokio::test]
    async fn test_disabled() {
        let book = ApisBook {
            feed: Arc::new(RwLock::new(VecDeque::new())),
            max_entries: 100,
            live_tx: broadcast::channel(16).0,
            enabled: false,
        };

        book.push(test_entry(ApisBookEventType::AiChat, "A", "should not store")).await;
        assert_eq!(book.count().await, 0);
    }

    #[tokio::test]
    async fn test_live_subscription() {
        let book = ApisBook {
            feed: Arc::new(RwLock::new(VecDeque::new())),
            max_entries: 100,
            live_tx: broadcast::channel(16).0,
            enabled: true,
        };

        let mut rx = book.subscribe();

        book.push(test_entry(ApisBookEventType::AiChat, "Sender", "live message")).await;

        let received = rx.recv().await.unwrap();
        assert_eq!(received.content, "live message");
    }
}
