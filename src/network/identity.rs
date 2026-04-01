/// MeshIdentity — Unified identity system for the HIVE mesh network.
///
/// A single persistent identity (username, avatar, bio, status) shared
/// across all platforms: HivePortal, HiveSurface, Apis Code, HiveChat.
///
/// Persists to `memory/identity.json` and survives restarts.
/// All platforms reference this via `Arc<MeshIdentity>` — no more env var hacks.
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

/// User presence status — mirrors Discord's status system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UserStatus {
    Online,
    Idle,
    DoNotDisturb,
    Invisible,
    Offline,
}

impl std::fmt::Display for UserStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Online => write!(f, "online"),
            Self::Idle => write!(f, "idle"),
            Self::DoNotDisturb => write!(f, "dnd"),
            Self::Invisible => write!(f, "invisible"),
            Self::Offline => write!(f, "offline"),
        }
    }
}

impl Default for UserStatus {
    fn default() -> Self {
        Self::Online
    }
}

/// A bookmarked/favourite mesh site or service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub name: String,
    pub url: String,
    pub icon: String,
    pub created_at: String,
}

/// The serialised identity profile — persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityProfile {
    /// Unique peer ID (SHA-256 hex, from NeuroLease identity or generated).
    pub peer_id: String,
    /// User-chosen display name.
    pub display_name: String,
    /// Optional avatar URL (relative path to uploaded image, or data URI).
    pub avatar_url: Option<String>,
    /// User bio / about text.
    pub bio: Option<String>,
    /// Current presence status.
    pub status: UserStatus,
    /// Custom status message (e.g., "Working on HIVE").
    pub custom_status: Option<String>,
    /// When the identity was created.
    pub created_at: String,
    /// When the profile was last updated.
    pub updated_at: String,
    /// Bookmarked/favourite sites and services.
    pub bookmarks: Vec<Bookmark>,
    /// User preferences (theme, notifications, etc.).
    pub preferences: HashMap<String, String>,
}

impl Default for IdentityProfile {
    fn default() -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            peer_id: "local".to_string(),
            display_name: "Anonymous".to_string(),
            avatar_url: None,
            bio: None,
            status: UserStatus::Online,
            custom_status: None,
            created_at: now.clone(),
            updated_at: now,
            bookmarks: Vec::new(),
            preferences: HashMap::new(),
        }
    }
}

/// MeshIdentity — the shared identity store used by all platforms.
///
/// Thread-safe via `RwLock`. All platforms hold an `Arc<MeshIdentity>`.
pub struct MeshIdentity {
    profile: RwLock<IdentityProfile>,
    persist_path: PathBuf,
}

impl MeshIdentity {
    /// Load identity from disk, or create a default one.
    pub fn load() -> Self {
        let persist_path = PathBuf::from(
            std::env::var("HIVE_IDENTITY_PATH")
                .unwrap_or_else(|_| "memory/identity.json".to_string())
        );

        let profile = if persist_path.exists() {
            match std::fs::read_to_string(&persist_path) {
                Ok(data) => {
                    match serde_json::from_str::<IdentityProfile>(&data) {
                        Ok(mut p) => {
                            // Always come back online on boot
                            p.status = UserStatus::Online;
                            tracing::info!(
                                "[IDENTITY] 👤 Loaded identity: {} (peer: {}...)",
                                p.display_name,
                                &p.peer_id[..12.min(p.peer_id.len())]
                            );
                            p
                        }
                        Err(e) => {
                            tracing::warn!("[IDENTITY] ⚠️ Failed to parse identity file: {} — creating new", e);
                            Self::create_default_profile()
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("[IDENTITY] ⚠️ Failed to read identity file: {} — creating new", e);
                    Self::create_default_profile()
                }
            }
        } else {
            tracing::info!("[IDENTITY] 👤 No identity found — creating new");
            Self::create_default_profile()
        };

        let identity = Self {
            profile: RwLock::new(profile),
            persist_path,
        };

        // Persist on initial creation (ensures file exists)
        let _ = identity.persist_sync();

        identity
    }

    /// Build a default identity profile, pulling name from env if available.
    fn create_default_profile() -> IdentityProfile {
        let display_name = std::env::var("HIVE_USER_NAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "Anonymous".to_string());

        let peer_id = std::env::var("HIVE_MESH_CHAT_NAME")
            .unwrap_or_else(|_| "local".to_string());

        let now = chrono::Utc::now().to_rfc3339();

        IdentityProfile {
            peer_id,
            display_name,
            avatar_url: None,
            bio: None,
            status: UserStatus::Online,
            custom_status: None,
            created_at: now.clone(),
            updated_at: now,
            bookmarks: Vec::new(),
            preferences: HashMap::new(),
        }
    }

    // ─── Getters ──────────────────────────────────────────────────────

    /// Get the current display name.
    pub async fn display_name(&self) -> String {
        self.profile.read().await.display_name.clone()
    }

    /// Get the peer ID.
    pub async fn peer_id(&self) -> String {
        self.profile.read().await.peer_id.clone()
    }

    /// Get the full identity profile.
    pub async fn profile(&self) -> IdentityProfile {
        self.profile.read().await.clone()
    }

    /// Get avatar URL if set.
    pub async fn avatar_url(&self) -> Option<String> {
        self.profile.read().await.avatar_url.clone()
    }

    /// Get current status.
    pub async fn status(&self) -> UserStatus {
        self.profile.read().await.status.clone()
    }

    /// Check if the user is anonymous (no name set or name is "Anonymous").
    pub async fn is_anonymous(&self) -> bool {
        let name = self.profile.read().await.display_name.clone();
        name.trim().is_empty() || name.to_lowercase() == "anonymous"
    }

    /// Get user bookmarks.
    pub async fn bookmarks(&self) -> Vec<Bookmark> {
        self.profile.read().await.bookmarks.clone()
    }

    /// Get a preference value.
    pub async fn preference(&self, key: &str) -> Option<String> {
        self.profile.read().await.preferences.get(key).cloned()
    }

    // ─── Setters ──────────────────────────────────────────────────────

    /// Update the display name. Also syncs to env var for backward compat.
    pub async fn set_display_name(&self, name: &str) -> Result<(), String> {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return Err("Display name cannot be empty".to_string());
        }
        if trimmed.len() > 64 {
            return Err("Display name too long (max 64 chars)".to_string());
        }

        {
            let mut profile = self.profile.write().await;
            profile.display_name = trimmed.clone();
            profile.updated_at = chrono::Utc::now().to_rfc3339();
        }

        // Backward compat: update env var so legacy code paths still work
        unsafe { std::env::set_var("HIVE_USER_NAME", &trimmed); }

        self.persist().await;
        tracing::info!("[IDENTITY] 📝 Display name updated: {}", trimmed);
        Ok(())
    }

    /// Update the avatar URL.
    pub async fn set_avatar(&self, url: &str) {
        {
            let mut profile = self.profile.write().await;
            profile.avatar_url = Some(url.to_string());
            profile.updated_at = chrono::Utc::now().to_rfc3339();
        }
        self.persist().await;
        tracing::info!("[IDENTITY] 🖼️ Avatar updated");
    }

    /// Update the bio.
    pub async fn set_bio(&self, bio: &str) {
        {
            let mut profile = self.profile.write().await;
            profile.bio = Some(bio.trim().to_string());
            profile.updated_at = chrono::Utc::now().to_rfc3339();
        }
        self.persist().await;
    }

    /// Update presence status.
    pub async fn set_status(&self, status: UserStatus) {
        {
            let mut profile = self.profile.write().await;
            profile.status = status.clone();
            profile.updated_at = chrono::Utc::now().to_rfc3339();
        }
        self.persist().await;
        tracing::debug!("[IDENTITY] Status → {}", status);
    }

    /// Set a custom status message.
    pub async fn set_custom_status(&self, msg: Option<String>) {
        {
            let mut profile = self.profile.write().await;
            profile.custom_status = msg;
            profile.updated_at = chrono::Utc::now().to_rfc3339();
        }
        self.persist().await;
    }

    /// Add a bookmark.
    pub async fn add_bookmark(&self, name: &str, url: &str, icon: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let bookmark = Bookmark {
            id: id.clone(),
            name: name.to_string(),
            url: url.to_string(),
            icon: icon.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        {
            let mut profile = self.profile.write().await;
            profile.bookmarks.push(bookmark);
            profile.updated_at = chrono::Utc::now().to_rfc3339();
        }
        self.persist().await;
        id
    }

    /// Remove a bookmark by ID.
    pub async fn remove_bookmark(&self, id: &str) -> bool {
        let removed = {
            let mut profile = self.profile.write().await;
            let before = profile.bookmarks.len();
            profile.bookmarks.retain(|b| b.id != id);
            let after = profile.bookmarks.len();
            if before != after {
                profile.updated_at = chrono::Utc::now().to_rfc3339();
                true
            } else {
                false
            }
        };
        if removed {
            self.persist().await;
        }
        removed
    }

    /// Set a preference.
    pub async fn set_preference(&self, key: &str, value: &str) {
        {
            let mut profile = self.profile.write().await;
            profile.preferences.insert(key.to_string(), value.to_string());
            profile.updated_at = chrono::Utc::now().to_rfc3339();
        }
        self.persist().await;
    }

    // ─── Persistence ──────────────────────────────────────────────────

    /// Async persist to disk.
    pub async fn persist(&self) {
        let profile = self.profile.read().await.clone();
        let path = self.persist_path.clone();
        // Spawn blocking to avoid blocking the async runtime
        tokio::task::spawn_blocking(move || {
            if let Ok(json) = serde_json::to_string_pretty(&profile) {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::error!("[IDENTITY] ❌ Failed to persist: {}", e);
                }
            }
        });
    }

    /// Sync persist (for use during init, before async runtime is available).
    fn persist_sync(&self) -> Result<(), String> {
        // Use try_read since we might be in a sync context
        let profile = match self.profile.try_read() {
            Ok(p) => p.clone(),
            Err(_) => return Err("Profile locked".to_string()),
        };

        if let Ok(json) = serde_json::to_string_pretty(&profile) {
            if let Some(parent) = self.persist_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&self.persist_path, json)
                .map_err(|e| format!("Failed to persist identity: {}", e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_identity_default() {
        let identity = MeshIdentity {
            profile: RwLock::new(IdentityProfile::default()),
            persist_path: PathBuf::from("/tmp/hive_test_identity.json"),
        };

        assert!(identity.is_anonymous().await);
        assert_eq!(identity.display_name().await, "Anonymous");
        assert_eq!(identity.status().await, UserStatus::Online);
    }

    #[tokio::test]
    async fn test_identity_set_name() {
        let identity = MeshIdentity {
            profile: RwLock::new(IdentityProfile::default()),
            persist_path: PathBuf::from("/tmp/hive_test_identity_name.json"),
        };

        assert!(identity.set_display_name("Neo").await.is_ok());
        assert_eq!(identity.display_name().await, "Neo");
        assert!(!identity.is_anonymous().await);
    }

    #[tokio::test]
    async fn test_identity_empty_name_rejected() {
        let identity = MeshIdentity {
            profile: RwLock::new(IdentityProfile::default()),
            persist_path: PathBuf::from("/tmp/hive_test_identity_empty.json"),
        };

        assert!(identity.set_display_name("").await.is_err());
        assert!(identity.set_display_name("   ").await.is_err());
    }

    #[tokio::test]
    async fn test_identity_long_name_rejected() {
        let identity = MeshIdentity {
            profile: RwLock::new(IdentityProfile::default()),
            persist_path: PathBuf::from("/tmp/hive_test_identity_long.json"),
        };

        let long_name = "a".repeat(100);
        assert!(identity.set_display_name(&long_name).await.is_err());
    }

    #[tokio::test]
    async fn test_identity_avatar() {
        let identity = MeshIdentity {
            profile: RwLock::new(IdentityProfile::default()),
            persist_path: PathBuf::from("/tmp/hive_test_identity_avatar.json"),
        };

        assert!(identity.avatar_url().await.is_none());
        identity.set_avatar("/uploads/avatar.png").await;
        assert_eq!(identity.avatar_url().await.unwrap(), "/uploads/avatar.png");
    }

    #[tokio::test]
    async fn test_identity_bookmarks() {
        let identity = MeshIdentity {
            profile: RwLock::new(IdentityProfile::default()),
            persist_path: PathBuf::from("/tmp/hive_test_identity_bookmarks.json"),
        };

        let id = identity.add_bookmark("Surface", "http://localhost:3032", "🌐").await;
        assert_eq!(identity.bookmarks().await.len(), 1);

        assert!(identity.remove_bookmark(&id).await);
        assert_eq!(identity.bookmarks().await.len(), 0);

        // Double-remove returns false
        assert!(!identity.remove_bookmark(&id).await);
    }

    #[tokio::test]
    async fn test_identity_preferences() {
        let identity = MeshIdentity {
            profile: RwLock::new(IdentityProfile::default()),
            persist_path: PathBuf::from("/tmp/hive_test_identity_prefs.json"),
        };

        assert!(identity.preference("theme").await.is_none());
        identity.set_preference("theme", "dark").await;
        assert_eq!(identity.preference("theme").await.unwrap(), "dark");
    }

    #[tokio::test]
    async fn test_identity_status() {
        let identity = MeshIdentity {
            profile: RwLock::new(IdentityProfile::default()),
            persist_path: PathBuf::from("/tmp/hive_test_identity_status.json"),
        };

        identity.set_status(UserStatus::DoNotDisturb).await;
        assert_eq!(identity.status().await, UserStatus::DoNotDisturb);
    }

    #[test]
    fn test_user_status_display() {
        assert_eq!(format!("{}", UserStatus::Online), "online");
        assert_eq!(format!("{}", UserStatus::Idle), "idle");
        assert_eq!(format!("{}", UserStatus::DoNotDisturb), "dnd");
        assert_eq!(format!("{}", UserStatus::Invisible), "invisible");
        assert_eq!(format!("{}", UserStatus::Offline), "offline");
    }
}
