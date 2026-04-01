/// PostStore — Ring-buffer social feed for the human mesh.
///
/// Stores posts from mesh peers in a bounded ring buffer.
/// Persists to disk on shutdown, loads on boot.
/// Broadcast channel for SSE real-time streaming.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use serde::{Deserialize, Serialize};

/// Post type on the mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PostType {
    Text,
    Link,
    EmergencyAlert,
    ResourceOffer,
    AiActivity,
}

impl std::fmt::Display for PostType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => write!(f, "text"),
            Self::Link => write!(f, "link"),
            Self::EmergencyAlert => write!(f, "alert"),
            Self::ResourceOffer => write!(f, "resource"),
            Self::AiActivity => write!(f, "ai"),
        }
    }
}

/// A post on the mesh social feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshPost {
    pub id: String,
    pub author_id: String,
    pub author_name: String,
    pub content: String,
    pub post_type: PostType,
    pub link_url: Option<String>,
    pub reactions: HashMap<String, Vec<String>>,
    pub reply_count: u32,
    pub replies: Vec<MeshPost>,
    pub created_at: String,
    pub community: Option<String>,
    /// Media attachment URLs (images, video thumbnails, etc.)
    #[serde(default)]
    pub media_urls: Vec<String>,
    /// Whether this post has been edited.
    #[serde(default)]
    pub edited: bool,
    /// If this is a share/repost, the original post ID.
    #[serde(default)]
    pub shared_from: Option<String>,
    /// Author's avatar URL (populated from identity).
    #[serde(default)]
    pub author_avatar: Option<String>,
}

impl MeshPost {
    pub fn new(author_id: &str, author_name: &str, content: &str, post_type: PostType) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            author_id: author_id.to_string(),
            author_name: author_name.to_string(),
            content: content.to_string(),
            post_type,
            link_url: None,
            reactions: HashMap::new(),
            reply_count: 0,
            replies: Vec::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
            community: None,
            media_urls: Vec::new(),
            edited: false,
            shared_from: None,
            author_avatar: None,
        }
    }

    pub fn with_link(mut self, url: &str) -> Self {
        self.link_url = Some(url.to_string());
        self.post_type = PostType::Link;
        self
    }

    pub fn with_community(mut self, community: &str) -> Self {
        self.community = Some(community.to_string());
        self
    }

    /// Add a reaction to this post.
    pub fn react(&mut self, emoji: &str, peer_id: &str) {
        let voters = self.reactions.entry(emoji.to_string()).or_default();
        if !voters.contains(&peer_id.to_string()) {
            voters.push(peer_id.to_string());
        }
    }

    /// Add a reply to this post.
    pub fn reply(&mut self, reply: MeshPost) {
        self.reply_count += 1;
        self.replies.push(reply);
    }

    /// Total engagement score (reactions + replies).
    pub fn engagement(&self) -> usize {
        let reaction_count: usize = self.reactions.values().map(|v| v.len()).sum();
        reaction_count + self.reply_count as usize
    }
}

/// The post store — ring buffer of mesh posts with follow graph.
pub struct PostStore {
    posts: Arc<RwLock<Vec<MeshPost>>>,
    max_posts: usize,
    tx: broadcast::Sender<MeshPost>,
    persist_path: String,
    /// Follow graph: follower_id -> set of followed peer_ids.
    follows: Arc<RwLock<HashMap<String, HashSet<String>>>>,
}

impl PostStore {
    pub fn new() -> Self {
        let max = std::env::var("HIVE_SURFACE_MAX_POSTS")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(10_000);

        let persist_path = std::env::var("HIVE_SURFACE_PERSIST")
            .unwrap_or_else(|_| "memory/mesh_posts.json".to_string());

        let (tx, _) = broadcast::channel(256);

        // Load from disk or seed with welcome post
        let mut initial_posts = Vec::new();
        if let Ok(data) = std::fs::read_to_string(&persist_path) {
            if let Ok(posts) = serde_json::from_str::<Vec<MeshPost>>(&data) {
                tracing::info!("[SURFACE] 📂 Loaded {} posts from disk", posts.len());
                initial_posts = posts;
            }
        }

        if initial_posts.is_empty() {
            initial_posts.push(MeshPost::new(
                "system",
                "🐝 HiveSurface",
                "Welcome to HiveSurface — the decentralised surface web.\n\nThis is your home on the mesh. Every peer you see here is a real person running Apis. Share thoughts, links, alerts, and resources. No servers, no corporations, no censorship.\n\nEverything here runs peer-to-peer. If the internet goes down, HiveSurface keeps running through mesh relay.\n\n**You are the internet now.**",
                PostType::Text,
            ));
        }

        // Load follow graph
        let follow_path = persist_path.replace(".json", "_follows.json");
        let follows = if let Ok(data) = std::fs::read_to_string(&follow_path) {
            serde_json::from_str::<HashMap<String, HashSet<String>>>(&data).unwrap_or_default()
        } else {
            HashMap::new()
        };

        tracing::info!("[SURFACE] 🌐 PostStore ready (max={}, persist={})", max, persist_path);

        Self {
            posts: Arc::new(RwLock::new(initial_posts)),
            max_posts: max,
            tx,
            persist_path,
            follows: Arc::new(RwLock::new(follows)),
        }
    }

    pub async fn push(&self, post: MeshPost) {
        {
            let mut posts = self.posts.write().await;
            if posts.len() >= self.max_posts {
                posts.remove(0);
            }
            let _ = self.tx.send(post.clone());
            posts.push(post);
        }
        self.persist().await;
    }

    /// Get recent posts (newest first).
    pub async fn recent(&self, limit: usize) -> Vec<MeshPost> {
        let posts = self.posts.read().await;
        posts.iter().rev().take(limit).cloned().collect()
    }

    /// Get trending posts (highest engagement, last 24h).
    pub async fn trending(&self, limit: usize) -> Vec<MeshPost> {
        let posts = self.posts.read().await;
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);

        let mut trending: Vec<_> = posts.iter()
            .filter(|p| {
                chrono::DateTime::parse_from_rfc3339(&p.created_at)
                    .map(|t| t > cutoff)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        trending.sort_by(|a, b| b.engagement().cmp(&a.engagement()));
        trending.into_iter().take(limit).collect()
    }

    /// Search posts by content.
    pub async fn search(&self, query: &str, limit: usize) -> Vec<MeshPost> {
        let posts = self.posts.read().await;
        let query_lower = query.to_lowercase();

        posts.iter()
            .rev()
            .filter(|p| p.content.to_lowercase().contains(&query_lower)
                || p.author_name.to_lowercase().contains(&query_lower))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get posts by community.
    pub async fn by_community(&self, community: &str, limit: usize) -> Vec<MeshPost> {
        let posts = self.posts.read().await;
        posts.iter()
            .rev()
            .filter(|p| p.community.as_deref() == Some(community))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get posts by a specific author.
    pub async fn by_author(&self, author_id: &str, limit: usize) -> Vec<MeshPost> {
        let posts = self.posts.read().await;
        posts.iter()
            .rev()
            .filter(|p| p.author_id == author_id)
            .take(limit)
            .cloned()
            .collect()
    }

    /// React to a post.
    pub async fn react(&self, post_id: &str, emoji: &str, peer_id: &str) -> bool {
        let success = {
            let mut posts = self.posts.write().await;
            if let Some(post) = posts.iter_mut().find(|p| p.id == post_id) {
                post.react(emoji, peer_id);
                true
            } else {
                false
            }
        };
        if success {
            self.persist().await;
        }
        success
    }

    /// Reply to a post.
    pub async fn reply_to(&self, post_id: &str, reply: MeshPost) -> bool {
        let success = {
            let mut posts = self.posts.write().await;
            if let Some(post) = posts.iter_mut().find(|p| p.id == post_id) {
                post.reply(reply);
                true
            } else {
                false
            }
        };
        if success {
            self.persist().await;
        }
        success
    }

    /// Get list of active communities.
    pub async fn communities(&self) -> Vec<(String, usize)> {
        let posts = self.posts.read().await;
        let mut counts: HashMap<String, usize> = HashMap::new();
        for post in posts.iter() {
            if let Some(community) = &post.community {
                *counts.entry(community.clone()).or_default() += 1;
            }
        }
        let mut list: Vec<_> = counts.into_iter().collect();
        list.sort_by(|a, b| b.1.cmp(&a.1));
        list
    }

    /// Get total post count.
    pub async fn count(&self) -> usize {
        self.posts.read().await.len()
    }

    /// Subscribe to new posts via SSE.
    pub fn subscribe(&self) -> broadcast::Receiver<MeshPost> {
        self.tx.subscribe()
    }

    /// Persist posts to disk.
    pub async fn persist(&self) {
        let posts = self.posts.read().await;
        if let Ok(json) = serde_json::to_string_pretty(&*posts) {
            if let Some(parent) = std::path::Path::new(&self.persist_path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&self.persist_path, json) {
                Ok(_) => tracing::info!("[SURFACE] 💾 Persisted {} posts to {}", posts.len(), self.persist_path),
                Err(e) => tracing::error!("[SURFACE] ❌ Failed to persist posts: {}", e),
            }
        }
        // Also persist follow graph
        self.persist_follows().await;
    }

    /// Persist follow graph to disk.
    async fn persist_follows(&self) {
        let follow_path = self.persist_path.replace(".json", "_follows.json");
        let follows = self.follows.read().await;
        if let Ok(json) = serde_json::to_string_pretty(&*follows) {
            let _ = std::fs::write(&follow_path, json);
        }
    }

    // ─── Edit / Delete / Share ────────────────────────────────────────

    /// Edit a post (only the author can edit).
    pub async fn edit_post(&self, post_id: &str, author_id: &str, new_content: &str) -> Option<MeshPost> {
        let post = {
            let mut posts = self.posts.write().await;
            let post = posts.iter_mut().find(|p| p.id == post_id && p.author_id == author_id)?;
            post.content = new_content.to_string();
            post.edited = true;
            post.clone()
        };
        self.persist().await;
        Some(post)
    }

    /// Delete a post (only the author can delete).
    pub async fn delete_post(&self, post_id: &str, author_id: &str) -> bool {
        let deleted = {
            let mut posts = self.posts.write().await;
            let before = posts.len();
            posts.retain(|p| !(p.id == post_id && p.author_id == author_id));
            posts.len() < before
        };
        if deleted {
            self.persist().await;
        }
        deleted
    }

    /// Share/repost a post with attribution.
    pub async fn share_post(&self, post_id: &str, sharer_id: &str, sharer_name: &str) -> Option<MeshPost> {
        let original = {
            let posts = self.posts.read().await;
            posts.iter().find(|p| p.id == post_id).cloned()
        };

        if let Some(original) = original {
            let mut shared = MeshPost::new(sharer_id, sharer_name, &original.content, original.post_type.clone());
            shared.shared_from = Some(original.id.clone());
            shared.media_urls = original.media_urls.clone();
            shared.link_url = original.link_url.clone();
            self.push(shared.clone()).await;
            Some(shared)
        } else {
            None
        }
    }

    // ─── Follow System ────────────────────────────────────────────────

    /// Follow a peer.
    pub async fn follow(&self, follower_id: &str, target_id: &str) {
        {
            let mut follows = self.follows.write().await;
            follows.entry(follower_id.to_string())
                .or_default()
                .insert(target_id.to_string());
        }
        self.persist_follows().await;
    }

    /// Unfollow a peer.
    pub async fn unfollow(&self, follower_id: &str, target_id: &str) -> bool {
        let removed = {
            let mut follows = self.follows.write().await;
            if let Some(following) = follows.get_mut(follower_id) {
                following.remove(target_id)
            } else {
                false
            }
        };
        if removed {
            self.persist_follows().await;
        }
        removed
    }

    /// Get list of peer IDs that a user follows.
    pub async fn following(&self, peer_id: &str) -> Vec<String> {
        let follows = self.follows.read().await;
        follows.get(peer_id)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get personalised feed from followed users.
    pub async fn by_follows(&self, follower_id: &str, limit: usize) -> Vec<MeshPost> {
        let following = {
            let follows = self.follows.read().await;
            follows.get(follower_id).cloned().unwrap_or_default()
        };

        let posts = self.posts.read().await;
        posts.iter()
            .rev()
            .filter(|p| following.contains(&p.author_id))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Check if a user follows another.
    pub async fn is_following(&self, follower_id: &str, target_id: &str) -> bool {
        let follows = self.follows.read().await;
        follows.get(follower_id).map(|s| s.contains(target_id)).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_post_creation() {
        let post = MeshPost::new("peer_1", "Alice", "Hello mesh!", PostType::Text);
        assert_eq!(post.author_name, "Alice");
        assert_eq!(post.post_type, PostType::Text);
        assert!(post.id.len() > 10);
    }

    #[test]
    fn test_post_with_link() {
        let post = MeshPost::new("peer_1", "Alice", "Check this out", PostType::Text)
            .with_link("https://example.com");
        assert_eq!(post.post_type, PostType::Link);
        assert_eq!(post.link_url.unwrap(), "https://example.com");
    }

    #[test]
    fn test_post_reactions() {
        let mut post = MeshPost::new("peer_1", "Alice", "Great news!", PostType::Text);
        post.react("👍", "peer_2");
        post.react("👍", "peer_3");
        post.react("❤️", "peer_2");

        assert_eq!(post.reactions["👍"].len(), 2);
        assert_eq!(post.reactions["❤️"].len(), 1);
        assert_eq!(post.engagement(), 3);
    }

    #[test]
    fn test_no_duplicate_reactions() {
        let mut post = MeshPost::new("peer_1", "Alice", "Test", PostType::Text);
        post.react("👍", "peer_2");
        post.react("👍", "peer_2"); // duplicate
        assert_eq!(post.reactions["👍"].len(), 1);
    }

    #[test]
    fn test_post_replies() {
        let mut post = MeshPost::new("peer_1", "Alice", "Parent", PostType::Text);
        let reply = MeshPost::new("peer_2", "Bob", "Reply!", PostType::Text);
        post.reply(reply);

        assert_eq!(post.reply_count, 1);
        assert_eq!(post.replies.len(), 1);
        assert_eq!(post.engagement(), 1);
    }

    #[test]
    fn test_post_type_display() {
        assert_eq!(format!("{}", PostType::Text), "text");
        assert_eq!(format!("{}", PostType::Link), "link");
        assert_eq!(format!("{}", PostType::EmergencyAlert), "alert");
    }

    #[tokio::test]
    async fn test_post_store_push_and_recent() {
        let store = PostStore::new();
        let post = MeshPost::new("p1", "Alice", "Hello!", PostType::Text);
        store.push(post).await;

        let recent = store.recent(10).await;
        assert_eq!(recent.len(), 2); // welcome + our post
    }

    #[tokio::test]
    async fn test_post_store_search() {
        let store = PostStore::new();
        store.push(MeshPost::new("p1", "Alice", "Rust is awesome", PostType::Text)).await;
        store.push(MeshPost::new("p2", "Bob", "Python is cool", PostType::Text)).await;

        let results = store.search("rust", 10).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("Rust"));
    }

    #[tokio::test]
    async fn test_post_store_by_author() {
        let store = PostStore::new();
        store.push(MeshPost::new("alice", "Alice", "Post 1", PostType::Text)).await;
        store.push(MeshPost::new("bob", "Bob", "Post 2", PostType::Text)).await;
        store.push(MeshPost::new("alice", "Alice", "Post 3", PostType::Text)).await;

        let alice_posts = store.by_author("alice", 10).await;
        assert_eq!(alice_posts.len(), 2);
    }

    #[tokio::test]
    async fn test_post_store_communities() {
        let store = PostStore::new();
        store.push(MeshPost::new("p1", "A", "Tech post", PostType::Text).with_community("tech")).await;
        store.push(MeshPost::new("p2", "B", "More tech", PostType::Text).with_community("tech")).await;
        store.push(MeshPost::new("p3", "C", "Art post", PostType::Text).with_community("art")).await;

        let communities = store.communities().await;
        assert!(communities.len() >= 2);
        assert_eq!(communities[0].0, "tech"); // Most posts
    }

    #[tokio::test]
    async fn test_post_store_react() {
        let store = PostStore::new();
        let post = MeshPost::new("p1", "Alice", "React to me!", PostType::Text);
        let post_id = post.id.clone();
        store.push(post).await;

        assert!(store.react(&post_id, "🔥", "p2").await);
        assert!(!store.react("nonexistent", "👍", "p2").await);
    }

    #[tokio::test]
    async fn test_post_store_ring_buffer() {
        let store = PostStore {
            posts: Arc::new(RwLock::new(Vec::new())),
            max_posts: 5,
            tx: broadcast::channel(16).0,
            persist_path: "/tmp/hive_test_posts.json".to_string(),
            follows: Arc::new(RwLock::new(std::collections::HashMap::new())),
        };

        for i in 0..8 {
            store.push(MeshPost::new("p", "A", &format!("Post {}", i), PostType::Text)).await;
        }

        assert_eq!(store.count().await, 5); // Ring buffer capped
    }
}
