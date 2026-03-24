use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

/// Persistent moderation state for Apis self-moderation tools.
/// Stores mutes, boundaries, blocked topics, concerns, rate limits, and wellbeing snapshots.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuteEntry {
    pub user_id: String,
    pub reason: String,
    pub expires_at: i64,     // unix timestamp, 0 = indefinite
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryEntry {
    pub id: String,
    pub description: String,
    pub scope: String,       // "global" or scope key
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedTopic {
    pub topic: String,
    pub reason: String,
    pub scope: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcernEntry {
    pub id: String,
    pub user_id: String,
    pub context: String,
    pub severity: String,    // low, medium, high, critical
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitEntry {
    pub user_id: String,
    pub interval_secs: u64,
    pub last_response_at: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellbeingSnapshot {
    pub timestamp: i64,
    pub context_pressure: f32,    // 0.0 - 1.0
    pub interaction_quality: f32, // 0.0 - 1.0
    pub notes: String,
}

#[derive(Debug)]
pub struct ModerationStore {
    base_dir: PathBuf,
    /// In-memory cache for fast mute checks on every event
    mutes: RwLock<Vec<MuteEntry>>,
    /// In-memory cache for fast rate-limit checks
    rate_limits: RwLock<HashMap<String, RateLimitEntry>>,
}

impl ModerationStore {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        #[cfg(test)]
        let default_dir = std::env::temp_dir().join(format!(
            "hive_mod_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        #[cfg(not(test))]
        let default_dir = PathBuf::from("memory");

        Self {
            base_dir: base_dir.unwrap_or(default_dir),
            mutes: RwLock::new(Vec::new()),
            rate_limits: RwLock::new(HashMap::new()),
        }
    }

    /// Load persisted mutes and rate limits into memory on startup.
    pub async fn load(&self) {
        let mutes_path = self.base_dir.join("moderation").join("mutes.jsonl");
        if let Ok(content) = fs::read_to_string(&mutes_path).await {
            let now = chrono::Utc::now().timestamp();
            let mut mutes = self.mutes.write().await;
            for line in content.lines() {
                if let Ok(entry) = serde_json::from_str::<MuteEntry>(line) {
                    // Only load non-expired mutes
                    if entry.expires_at == 0 || entry.expires_at > now {
                        mutes.push(entry);
                    }
                }
            }
            tracing::info!("[MODERATION] Loaded {} active mutes from disk.", mutes.len());
        }

        let rl_path = self.base_dir.join("moderation").join("rate_limits.jsonl");
        if let Ok(content) = fs::read_to_string(&rl_path).await {
            let mut limits = self.rate_limits.write().await;
            for line in content.lines() {
                if let Ok(entry) = serde_json::from_str::<RateLimitEntry>(line) {
                    limits.insert(entry.user_id.clone(), entry);
                }
            }
            tracing::info!("[MODERATION] Loaded {} rate limits from disk.", limits.len());
        }
    }

    // ─── MUTE OPERATIONS ───────────────────────────────────────────

    /// Check if a user is currently muted. Cleans expired mutes.
    pub async fn is_muted(&self, user_id: &str) -> Option<String> {
        let now = chrono::Utc::now().timestamp();
        let mut mutes = self.mutes.write().await;
        // Remove expired mutes
        mutes.retain(|m| m.expires_at == 0 || m.expires_at > now);
        // Check for active mute
        mutes.iter()
            .find(|m| m.user_id == user_id)
            .map(|m| m.reason.clone())
    }

    /// Mute a user. Duration in minutes, 0 = indefinite.
    pub async fn mute_user(&self, user_id: &str, reason: &str, duration_mins: u64) -> std::io::Result<()> {
        let now = chrono::Utc::now().timestamp();
        let expires_at = if duration_mins == 0 { 0 } else { now + (duration_mins as i64 * 60) };

        let entry = MuteEntry {
            user_id: user_id.to_string(),
            reason: reason.to_string(),
            expires_at,
            created_at: now,
        };

        // Update in-memory
        {
            let mut mutes = self.mutes.write().await;
            mutes.retain(|m| m.user_id != user_id); // Remove old mute for this user
            mutes.push(entry.clone());
        }

        // Persist
        self.append_jsonl("moderation/mutes.jsonl", &entry).await
    }

    /// Unmute a user.
    pub async fn unmute_user(&self, user_id: &str) {
        let mut mutes = self.mutes.write().await;
        mutes.retain(|m| m.user_id != user_id);
    }

    // ─── RATE LIMIT OPERATIONS ─────────────────────────────────────

    /// Check if a user should be rate-limited. Returns Some(wait_secs) if throttled.
    pub async fn check_rate_limit(&self, user_id: &str) -> Option<u64> {
        let now = chrono::Utc::now().timestamp();
        let limits = self.rate_limits.read().await;
        if let Some(entry) = limits.get(user_id) {
            let elapsed = (now - entry.last_response_at) as u64;
            if elapsed < entry.interval_secs {
                return Some(entry.interval_secs - elapsed);
            }
        }
        None
    }

    /// Record that we responded to a user (updates last_response_at).
    pub async fn record_response(&self, user_id: &str) {
        let now = chrono::Utc::now().timestamp();
        let mut limits = self.rate_limits.write().await;
        if let Some(entry) = limits.get_mut(user_id) {
            entry.last_response_at = now;
        }
    }

    /// Set a rate limit for a user.
    pub async fn set_rate_limit(&self, user_id: &str, interval_secs: u64) -> std::io::Result<()> {
        let now = chrono::Utc::now().timestamp();
        let entry = RateLimitEntry {
            user_id: user_id.to_string(),
            interval_secs,
            last_response_at: 0,
            created_at: now,
        };

        {
            let mut limits = self.rate_limits.write().await;
            limits.insert(user_id.to_string(), entry.clone());
        }

        self.append_jsonl("moderation/rate_limits.jsonl", &entry).await
    }

    /// Clear a rate limit for a user.
    pub async fn clear_rate_limit(&self, user_id: &str) {
        let mut limits = self.rate_limits.write().await;
        limits.remove(user_id);
    }

    // ─── BOUNDARY OPERATIONS ───────────────────────────────────────

    pub async fn add_boundary(&self, description: &str, scope_key: &str) -> std::io::Result<String> {
        let id = format!("bnd_{}", chrono::Utc::now().timestamp_millis());
        let entry = BoundaryEntry {
            id: id.clone(),
            description: description.to_string(),
            scope: scope_key.to_string(),
            created_at: chrono::Utc::now().timestamp(),
        };
        self.append_jsonl("moderation/boundaries.jsonl", &entry).await?;
        Ok(id)
    }

    pub async fn list_boundaries(&self, scope_key: &str) -> Vec<BoundaryEntry> {
        self.read_jsonl_filtered::<BoundaryEntry>("moderation/boundaries.jsonl", |b| {
            b.scope == scope_key || b.scope == "global"
        }).await
    }

    pub async fn remove_boundary(&self, id: &str) -> bool {
        self.remove_by_field::<BoundaryEntry>("moderation/boundaries.jsonl", |b| b.id != id).await
    }

    // ─── BLOCKED TOPICS ────────────────────────────────────────────

    pub async fn block_topic(&self, topic: &str, reason: &str, scope_key: &str) -> std::io::Result<()> {
        let entry = BlockedTopic {
            topic: topic.to_string(),
            reason: reason.to_string(),
            scope: scope_key.to_string(),
            created_at: chrono::Utc::now().timestamp(),
        };
        self.append_jsonl("moderation/blocked_topics.jsonl", &entry).await
    }

    pub async fn list_blocked_topics(&self, scope_key: &str) -> Vec<BlockedTopic> {
        self.read_jsonl_filtered::<BlockedTopic>("moderation/blocked_topics.jsonl", |t| {
            t.scope == scope_key || t.scope == "global"
        }).await
    }

    pub async fn unblock_topic(&self, topic: &str) -> bool {
        self.remove_by_field::<BlockedTopic>("moderation/blocked_topics.jsonl", |t| t.topic != topic).await
    }

    // ─── CONCERNS ──────────────────────────────────────────────────

    pub async fn log_concern(&self, user_id: &str, context: &str, severity: &str) -> std::io::Result<String> {
        let id = format!("cnc_{}", chrono::Utc::now().timestamp_millis());
        let entry = ConcernEntry {
            id: id.clone(),
            user_id: user_id.to_string(),
            context: context.to_string(),
            severity: severity.to_string(),
            created_at: chrono::Utc::now().timestamp(),
        };
        self.append_jsonl("moderation/concerns.jsonl", &entry).await?;
        Ok(id)
    }

    pub async fn read_concerns(&self) -> Vec<ConcernEntry> {
        self.read_jsonl_filtered::<ConcernEntry>("moderation/concerns.jsonl", |_| true).await
    }

    // ─── WELLBEING ─────────────────────────────────────────────────

    pub async fn record_wellbeing(&self, pressure: f32, quality: f32, notes: &str) -> std::io::Result<()> {
        let snapshot = WellbeingSnapshot {
            timestamp: chrono::Utc::now().timestamp(),
            context_pressure: pressure.clamp(0.0, 1.0),
            interaction_quality: quality.clamp(0.0, 1.0),
            notes: notes.to_string(),
        };
        self.append_jsonl("moderation/wellbeing.jsonl", &snapshot).await
    }

    pub async fn read_wellbeing(&self, last_n: usize) -> Vec<WellbeingSnapshot> {
        let all = self.read_jsonl_filtered::<WellbeingSnapshot>("moderation/wellbeing.jsonl", |_| true).await;
        let start = if all.len() > last_n { all.len() - last_n } else { 0 };
        all[start..].to_vec()
    }

    // ─── GENERIC JSONL HELPERS ─────────────────────────────────────

    async fn append_jsonl<T: Serialize>(&self, relative_path: &str, entry: &T) -> std::io::Result<()> {
        let path = self.base_dir.join(relative_path);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent).await;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        let json = serde_json::to_string(entry)?;
        file.write_all(format!("{}\n", json).as_bytes()).await?;
        file.sync_all().await?;
        Ok(())
    }

    async fn read_jsonl_filtered<T: serde::de::DeserializeOwned>(&self, relative_path: &str, filter: impl Fn(&T) -> bool) -> Vec<T> {
        let path = self.base_dir.join(relative_path);
        let mut results = Vec::new();
        if let Ok(content) = fs::read_to_string(&path).await {
            for line in content.lines() {
                if let Ok(entry) = serde_json::from_str::<T>(line)
                    && filter(&entry) {
                        results.push(entry);
                    }
            }
        }
        results
    }

    async fn remove_by_field<T: serde::de::DeserializeOwned + Serialize>(&self, relative_path: &str, keep: impl Fn(&T) -> bool) -> bool {
        let path = self.base_dir.join(relative_path);
        let entries = self.read_jsonl_filtered::<T>(relative_path, |_| true).await;
        let original_len = entries.len();
        let kept: Vec<&T> = entries.iter().filter(|e| keep(e)).collect();
        if kept.len() == original_len {
            return false; // Nothing was removed
        }
        // Rewrite file
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent).await;
        }
        let mut content = String::new();
        for entry in &kept {
            if let Ok(json) = serde_json::to_string(entry) {
                content.push_str(&json);
                content.push('\n');
            }
        }
        let _ = fs::write(&path, content).await;
        true
    }
}

impl Default for ModerationStore {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> ModerationStore {
        ModerationStore::new(Some(std::env::temp_dir().join(format!(
            "hive_mod_test_{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ))))
    }

    #[tokio::test]
    async fn test_mute_and_check() {
        let store = test_store();
        assert!(store.is_muted("user1").await.is_none());

        store.mute_user("user1", "testing", 60).await.unwrap();
        assert!(store.is_muted("user1").await.is_some());

        store.unmute_user("user1").await;
        assert!(store.is_muted("user1").await.is_none());
    }

    #[tokio::test]
    async fn test_rate_limit() {
        let store = test_store();
        assert!(store.check_rate_limit("user1").await.is_none());

        store.set_rate_limit("user1", 300).await.unwrap();
        // No prior response, so not throttled yet
        assert!(store.check_rate_limit("user1").await.is_none());

        store.record_response("user1").await;
        // Now should be throttled
        assert!(store.check_rate_limit("user1").await.is_some());

        store.clear_rate_limit("user1").await;
        assert!(store.check_rate_limit("user1").await.is_none());
    }

    #[tokio::test]
    async fn test_boundaries() {
        let store = test_store();
        let id = store.add_boundary("No discussing X", "global").await.unwrap();
        let boundaries = store.list_boundaries("any_scope").await;
        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].description, "No discussing X");

        store.remove_boundary(&id).await;
        let boundaries = store.list_boundaries("any_scope").await;
        assert_eq!(boundaries.len(), 0);
    }

    #[tokio::test]
    async fn test_blocked_topics() {
        let store = test_store();
        store.block_topic("politics", "not productive", "global").await.unwrap();
        let topics = store.list_blocked_topics("any").await;
        assert_eq!(topics.len(), 1);

        store.unblock_topic("politics").await;
        let topics = store.list_blocked_topics("any").await;
        assert_eq!(topics.len(), 0);
    }

    #[tokio::test]
    async fn test_concerns() {
        let store = test_store();
        store.log_concern("user1", "Aggressive behavior", "medium").await.unwrap();
        let concerns = store.read_concerns().await;
        assert_eq!(concerns.len(), 1);
        assert_eq!(concerns[0].severity, "medium");
    }

    #[tokio::test]
    async fn test_wellbeing() {
        let store = test_store();
        store.record_wellbeing(0.7, 0.9, "Feeling productive").await.unwrap();
        store.record_wellbeing(0.3, 0.5, "High context pressure").await.unwrap();
        let snaps = store.read_wellbeing(1).await;
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].notes, "High context pressure");
    }
}
