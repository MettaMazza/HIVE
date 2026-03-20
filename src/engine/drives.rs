use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

/// Decay rates per hour (matching Ernos 3.0 DriveSystem)
const SOCIAL_DECAY_PER_HOUR: f64 = 5.0;   // social_connection loses 5% per hour of silence
const UNCERTAINTY_GAIN_PER_HOUR: f64 = 2.0; // uncertainty rises 2% per hour (entropy)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveState {
    pub social_connection: f64, // 0.0–100.0, starts at 100.0
    pub uncertainty: f64,       // 0.0–100.0, starts at 0.0
    pub system_health: f64,     // 0.0–100.0, starts at 100.0
    pub last_updated: f64,      // unix timestamp
}

impl Default for DriveState {
    fn default() -> Self {
        Self {
            social_connection: 100.0,
            uncertainty: 0.0,
            system_health: 100.0,
            last_updated: now_ts(),
        }
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn clamp(v: f64) -> f64 {
    v.clamp(0.0, 100.0)
}

fn drives_path(base: &str) -> PathBuf {
    PathBuf::from(base).join("memory/core/drives.json")
}

pub struct DriveSystem {
    state: Mutex<DriveState>,
    persist_path: PathBuf,
}

impl DriveSystem {
    pub fn new(project_root: &str) -> Self {
        let path = drives_path(project_root);
        let state = Self::load(&path);
        Self {
            state: Mutex::new(state),
            persist_path: path,
        }
    }

    fn load(path: &PathBuf) -> DriveState {
        if path.exists() {
            if let Ok(raw) = std::fs::read_to_string(path) {
                if let Ok(state) = serde_json::from_str::<DriveState>(&raw) {
                    return state;
                }
            }
        }
        DriveState::default()
    }

    fn save_inner(state: &DriveState, path: &PathBuf) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(state) {
            let _ = std::fs::write(path, json);
        }
    }

    /// Apply passive decay since last_updated. Call at the start of each autonomy cycle.
    pub async fn update(&self) {
        let mut s = self.state.lock().await;
        let now = now_ts();
        let hours = (now - s.last_updated) / 3600.0;

        if hours > 0.0 {
            let old_social = s.social_connection;
            let old_uncertainty = s.uncertainty;
            s.social_connection = clamp(s.social_connection - SOCIAL_DECAY_PER_HOUR * hours);
            s.uncertainty = clamp(s.uncertainty + UNCERTAINTY_GAIN_PER_HOUR * hours);
            s.last_updated = now;
            Self::save_inner(&s, &self.persist_path);
            tracing::debug!("[ENGINE:Drives] Updated drives ({:.2}h elapsed): social {:.1} -> {:.1}, uncertainty {:.1} -> {:.1}",
                hours, old_social, s.social_connection, old_uncertainty, s.uncertainty);
        }
    }

    /// Modify a drive by `amount` (positive or negative), clamped to 0–100.
    pub async fn modify_drive(&self, drive: &str, amount: f64) {
        let mut s = self.state.lock().await;
        match drive {
            "social_connection" => {
                let old = s.social_connection;
                s.social_connection = clamp(s.social_connection + amount);
                tracing::debug!("[ENGINE:Drives] Modified social_connection: {:.1} -> {:.1} (delta={:+.1})", old, s.social_connection, amount);
            }
            "uncertainty" => {
                let old = s.uncertainty;
                s.uncertainty = clamp(s.uncertainty + amount);
                tracing::debug!("[ENGINE:Drives] Modified uncertainty: {:.1} -> {:.1} (delta={:+.1})", old, s.uncertainty, amount);
            }
            "system_health" => {
                let old = s.system_health;
                s.system_health = clamp(s.system_health + amount);
                tracing::debug!("[ENGINE:Drives] Modified system_health: {:.1} -> {:.1} (delta={:+.1})", old, s.system_health, amount);
            }
            other => {
                tracing::warn!("[DriveSystem] Unknown drive: {}", other);
                return;
            }
        }
        s.last_updated = now_ts();
        Self::save_inner(&s, &self.persist_path);
    }

    /// Returns current drive state (calls update first for freshness).
    pub async fn get_state(&self) -> DriveState {
        self.update().await;
        self.state.lock().await.clone()
    }

    /// Human-readable context block for injection into prompts.
    pub async fn format_for_prompt(&self) -> String {
        let s = self.get_state().await;
        format!(
            "HOMEOSTATIC DRIVE STATE:\n\
             - Social Connection: {:.1}% (decays 5%/hr — low signals desire to reach out)\n\
             - Uncertainty: {:.1}% (rises 2%/hr — high signals desire to explore/learn)\n\
             - System Health: {:.1}%\n\
             \n\
             Drive state is informational. You decide how to act on it.\n\
             outreach is available in your drone registry whenever you choose to use it.",
            s.social_connection, s.uncertainty, s.system_health
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_drive_defaults() {
        let ds = DriveSystem::new("/tmp/hive_test_drives");
        // Fresh load — if no file exists, starts at default
        let s = ds.state.lock().await.clone();
        assert!(s.social_connection >= 0.0 && s.social_connection <= 100.0);
        assert!(s.uncertainty >= 0.0 && s.uncertainty <= 100.0);
        assert!(s.system_health >= 0.0 && s.system_health <= 100.0);
    }

    #[tokio::test]
    async fn test_modify_drive_clamp() {
        let ds = DriveSystem::new("/tmp/hive_test_drives2");
        // Set to known state
        {
            let mut s = ds.state.lock().await;
            s.social_connection = 5.0;
        }
        // Subtract more than available — should clamp at 0.0
        ds.modify_drive("social_connection", -50.0).await;
        let s = ds.state.lock().await.clone();
        assert_eq!(s.social_connection, 0.0);

        // Add more than max — should clamp at 100.0
        ds.modify_drive("social_connection", 200.0).await;
        let s2 = ds.state.lock().await.clone();
        assert_eq!(s2.social_connection, 100.0);
    }

    #[tokio::test]
    async fn test_modify_unknown_drive() {
        let ds = DriveSystem::new("/tmp/hive_test_drives3");
        // Should not panic
        ds.modify_drive("nonexistent_drive", 10.0).await;
    }

    #[tokio::test]
    async fn test_format_for_prompt() {
        let ds = DriveSystem::new("/tmp/hive_test_drives4");
        let s = ds.format_for_prompt().await;
        assert!(s.contains("Social Connection"));
        assert!(s.contains("Uncertainty"));
        assert!(s.contains("System Health"));
        assert!(s.contains("outreach"));
    }
}
