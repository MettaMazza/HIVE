//! Account Linking — Persistent, disk-backed linking between glasses/app and Discord identity.
//!
//! Flow:
//! 1. First connect: glasses get a 6-digit link code + a device_token (UUID)
//! 2. User types `/link <code>` in Discord → HIVE binds device_token to Discord user ID
//! 3. Link is persisted to disk (`memory/glasses_links.json`)
//! 4. On reconnect: app sends device_token → HIVE auto-identifies the user
//! 5. Link persists until explicit `/unlink` or logout from app
//!
//! Codes expire after 5 minutes and are single-use.
//! Device tokens are permanent until explicitly revoked.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// How long a link code remains valid.
const CODE_EXPIRY: Duration = Duration::from_secs(300); // 5 minutes

/// Path to the persistent links file.
fn links_file_path() -> PathBuf {
    PathBuf::from("memory/glasses_links.json")
}

// ──────────────────────────────────────────────────────────────────
// Data Structures
// ──────────────────────────────────────────────────────────────────

/// A persistent link between a device and a Discord identity.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct DeviceLink {
    /// Unique device token (UUID) — stored on the app, sent on reconnect.
    pub device_token: String,
    /// Discord user ID this device is linked to.
    pub discord_user_id: String,
    /// Discord username for display.
    pub discord_username: String,
    /// When this link was created (Unix timestamp).
    pub linked_at: u64,
}

/// Pending link code entry (ephemeral, in-memory only).
struct PendingLink {
    /// The device_token that will be bound when claimed.
    device_token: String,
    /// The platform_id of the active connection.
    platform_id: String,
    /// When this code was created.
    created_at: Instant,
}

// ──────────────────────────────────────────────────────────────────
// Global State
// ──────────────────────────────────────────────────────────────────

/// Pending link codes (ephemeral, in-memory).
static PENDING_LINKS: std::sync::LazyLock<RwLock<HashMap<String, PendingLink>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

/// Active session mapping: platform_id → (discord_user_id, discord_username).
/// This is the runtime cache — populated from disk on reconnect or from claim.
static ACTIVE_SESSIONS: std::sync::LazyLock<RwLock<HashMap<String, (String, String)>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

// ──────────────────────────────────────────────────────────────────
// Persistent Storage
// ──────────────────────────────────────────────────────────────────

/// Load all device links from disk.
async fn load_links() -> HashMap<String, DeviceLink> {
    let path = links_file_path();
    match tokio::fs::read_to_string(&path).await {
        Ok(contents) => {
            serde_json::from_str(&contents).unwrap_or_default()
        }
        Err(_) => HashMap::new(),
    }
}

/// Save all device links to disk.
async fn save_links(links: &HashMap<String, DeviceLink>) {
    let path = links_file_path();
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    match serde_json::to_string_pretty(links) {
        Ok(json) => {
            if let Err(e) = tokio::fs::write(&path, json).await {
                tracing::error!("[LINK] Failed to persist links: {}", e);
            }
        }
        Err(e) => {
            tracing::error!("[LINK] Failed to serialize links: {}", e);
        }
    }
}

// ──────────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────────


/// Attempt to authenticate with an existing device token on reconnect.
///
/// Returns Some((discord_user_id, discord_username)) if the token is linked.
pub async fn authenticate_device(device_token: &str, platform_id: &str) -> Option<(String, String)> {
    let links = load_links().await;
    if let Some(link) = links.get(device_token) {
        // Cache in active sessions for this connection
        ACTIVE_SESSIONS.write().await.insert(
            platform_id.to_string(),
            (link.discord_user_id.clone(), link.discord_username.clone()),
        );
        tracing::info!(
            "[LINK] 🔓 Device auto-authenticated as {} ({})",
            link.discord_username, link.discord_user_id
        );
        Some((link.discord_user_id.clone(), link.discord_username.clone()))
    } else {
        None
    }
}

/// Send a DM to a Discord user with their verification code.
pub async fn send_dm(discord_user_id: u64, code: &str) -> Result<(), String> {
    let token = std::env::var("DISCORD_TOKEN").map_err(|_| "No DISCORD_TOKEN set".to_string())?;
    let http = serenity::http::Http::new(&token);
    let dm_channel = serenity::model::id::UserId::new(discord_user_id)
        .create_dm_channel(&http)
        .await
        .map_err(|e| format!("Failed to create DM: {}", e))?;
    
    let msg_text = format!("🐝 **HIVE Device Linking**\nYour 6-digit verification code is: **{}**\nEnter this in the HIVE app to connect your identity.", code);
    let builder = serenity::builder::CreateMessage::new().content(msg_text);
    
    dm_channel.send_message(&http, builder)
        .await
        .map_err(|e| format!("Failed to send DM: {}", e))?;
    Ok(())
}

/// App-first linking: App requests a code for a specific Discord ID.
/// HIVE generates the code, saves it as pending, and DMs the user.
pub async fn request_code_for_user(discord_user_id: &str, platform_id: &str) -> Result<(), String> {
    let uid: u64 = discord_user_id.parse().map_err(|_| "Invalid Discord ID".to_string())?;
    let device_token = uuid::Uuid::new_v4().to_string();
    let code = format!("{:06}", rand_u32() % 1_000_000);

    sweep_expired().await;

    // Save pending link with the target discord_id attached
    // We'll reuse PendingLink but we need a way to store the discord_id.
    // Wait, PendingLink doesn't have a discord_id field! Let's just append it to platform_id for now as "glasses:UUID|DISCORD_ID"
    let stored_platform_id = format!("{}|{}", platform_id, discord_user_id);

    PENDING_LINKS.write().await.insert(code.clone(), PendingLink {
        device_token,
        platform_id: stored_platform_id,
        created_at: Instant::now(),
    });

    send_dm(uid, &code).await?;
    tracing::info!("[LINK] 🔗 Sent requested link code {} to Discord ID {}", code, uid);
    Ok(())
}

/// App-first linking: App verifies the code it received in DM.
pub async fn verify_code_from_app(code: &str) -> Result<String, String> {
    let mut pending = PENDING_LINKS.write().await;
    
    // Find the link
    let link = pending.remove(code).ok_or("Invalid or expired link code.")?;
    
    if link.created_at.elapsed() > CODE_EXPIRY {
        return Err("Link code has expired. Request a new one.".to_string());
    }

    // Extract discord_id from our hacked platform_id
    let parts: Vec<&str> = link.platform_id.split('|').collect();
    if parts.len() < 2 {
        return Err("Internal error: missing discord ID in pending link".to_string());
    }
    let discord_id = parts[1].to_string();

    // Fetch username (simplified: just use ID or fetch from Discord)
    let discord_username = format!("User_{}", &discord_id[..4]);

    // Persist
    let new_link = DeviceLink {
        device_token: link.device_token.clone(),
        discord_user_id: discord_id.clone(),
        discord_username: discord_username.clone(),
        linked_at: Default::default(),
    };

    let mut links = load_links().await;
    links.insert(link.device_token.clone(), new_link);
    save_links(&links).await;

    tracing::info!("[LINK] ✅ App verified code {} and linked device token {}", code, &link.device_token[..8]);
    
    Ok(link.device_token)
}

/// Claim a link code from Discord. Persists the binding to disk.
///
/// Returns Ok((device_token, platform_id)) on success.
pub async fn claim_link_code(
    code: &str,
    discord_user_id: &str,
    discord_username: &str,
) -> Result<(String, String), String> {
    let mut pending = PENDING_LINKS.write().await;

    if let Some(link) = pending.remove(code) {
        if link.created_at.elapsed() > CODE_EXPIRY {
            return Err("Link code has expired. Generate a new one from your glasses.".to_string());
        }

        // Persist to disk
        let mut links = load_links().await;
        links.insert(link.device_token.clone(), DeviceLink {
            device_token: link.device_token.clone(),
            discord_user_id: discord_user_id.to_string(),
            discord_username: discord_username.to_string(),
            linked_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
        save_links(&links).await;

        // Cache in active sessions
        ACTIVE_SESSIONS.write().await.insert(
            link.platform_id.clone(),
            (discord_user_id.to_string(), discord_username.to_string()),
        );

        tracing::info!(
            "[LINK] ✅ Device {} linked to Discord {} ({}) — persisted to disk",
            &link.device_token[..8], discord_username, discord_user_id
        );
        Ok((link.device_token, link.platform_id))
    } else {
        Err("Invalid or expired link code.".to_string())
    }
}

/// Look up the Discord identity for an active glasses session.
pub async fn get_linked_identity(platform_id: &str) -> Option<(String, String)> {
    ACTIVE_SESSIONS.read().await.get(platform_id).cloned()
}

/// Remove a device link entirely (explicit unlink/logout). Removes from disk.
pub async fn unlink_device(device_token: &str) {
    let mut links = load_links().await;
    if links.remove(device_token).is_some() {
        save_links(&links).await;
        tracing::info!("[LINK] 🗑️ Device {} unlinked and removed from disk", &device_token[..8.min(device_token.len())]);
    }
}

/// Clear the active session cache for a disconnecting connection.
/// Does NOT remove the persistent link — the device can reconnect later.
pub async fn clear_session(platform_id: &str) {
    ACTIVE_SESSIONS.write().await.remove(platform_id);
}

/// Sweep expired pending link codes.
async fn sweep_expired() {
    let mut pending = PENDING_LINKS.write().await;
    pending.retain(|_, link| link.created_at.elapsed() <= CODE_EXPIRY);
}

/// Simple random u32 from system time nanos.
fn rand_u32() -> u32 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let thread_id = std::thread::current().id();
    let thread_hash = format!("{:?}", thread_id).len() as u32;
    nanos.wrapping_mul(2654435761).wrapping_add(thread_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_authenticate_device_not_linked() {
        let result = authenticate_device("nonexistent-token", "glasses:fake:0:0").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_device_link_serialization() {
        let link = DeviceLink {
            device_token: "test-token-123".to_string(),
            discord_user_id: "disc_456".to_string(),
            discord_username: "TestUser".to_string(),
            linked_at: 1710000000,
        };
        let json = serde_json::to_string(&link).unwrap();
        let deserialized: DeviceLink = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.device_token, "test-token-123");
        assert_eq!(deserialized.discord_user_id, "disc_456");
    }
}
