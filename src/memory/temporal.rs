use std::path::{Path, PathBuf};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootEntry {
    pub boot_start: String,
    pub boot_end: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalState {
    pub birthdate: Option<String>,
    pub last_shutdown: Option<String>,
    pub last_downtime_seconds: f64,
    pub last_boot: Option<String>,
    pub total_boots: u32,
    pub total_uptime_seconds: f64,
    #[serde(default)]
    pub boot_log: Vec<BootEntry>,
}

impl Default for TemporalState {
    fn default() -> Self {
        Self {
            birthdate: None,
            last_shutdown: None,
            last_downtime_seconds: 0.0,
            last_boot: None,
            total_boots: 0,
            total_uptime_seconds: 0.0,
            boot_log: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TemporalTracker {
    base_path: PathBuf,
    pub uptime_start: chrono::DateTime<Utc>,
    pub state: TemporalState,
}

impl TemporalTracker {
    pub fn new(path: &Path) -> Self {
        let state_file = path.join("temporal_state.json");
        let state = if state_file.exists() {
            std::fs::read_to_string(&state_file)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default()
        } else {
            TemporalState::default()
        };

        Self {
            base_path: path.to_path_buf(),
            uptime_start: Utc::now(),
            state,
        }
    }

    pub async fn init_and_register_boot(&mut self) {
        self.uptime_start = Utc::now();
        let now_iso = self.uptime_start.to_rfc3339();

        if self.state.birthdate.is_none() {
            self.state.birthdate = Some(now_iso.clone());
            tracing::info!("[MEMORY:Temporal] First boot ever — birthdate set to {}", now_iso);
        }

        if let Some(last_shutdown) = &self.state.last_shutdown
            && let Ok(shutdown_dt) = DateTime::parse_from_rfc3339(last_shutdown) {
                let shutdown_dt = shutdown_dt.with_timezone(&Utc);
                let gap = (self.uptime_start - shutdown_dt).num_seconds();
                self.state.last_downtime_seconds = gap.max(0) as f64;
                tracing::debug!("[MEMORY:Temporal] Downtime since last shutdown: {:.0}s", self.state.last_downtime_seconds);
            }

        self.state.last_boot = Some(now_iso.clone());
        self.state.total_boots += 1;

        // Add boot entry to the log
        self.state.boot_log.push(BootEntry {
            boot_start: now_iso,
            boot_end: None,
        });

        tracing::info!("[MEMORY:Temporal] Boot #{} registered (boot_log: {} entries)", self.state.total_boots, self.state.boot_log.len());
        self.recompute_uptime();
        self.save_state();
    }

    pub fn record_shutdown(&mut self) {
        let now = Utc::now();
        let now_iso = now.to_rfc3339();
        self.state.last_shutdown = Some(now_iso.clone());
        
        // Update current boot entry's end time
        if let Some(entry) = self.state.boot_log.last_mut() {
            entry.boot_end = Some(now_iso);
        }

        self.recompute_uptime();
        tracing::info!("[MEMORY:Temporal] Shutdown recorded (total_uptime={:.0}s from {} boot entries)", self.state.total_uptime_seconds, self.state.boot_log.len());
        self.save_state();
    }

    /// Periodically save current uptime without resetting the session. 
    /// Prevents uptime loss on crashes, kills, or process::exit calls.
    pub fn save_uptime_checkpoint(&mut self) {
        let now = Utc::now();
        
        // Update current boot entry's end time
        if let Some(entry) = self.state.boot_log.last_mut() {
            entry.boot_end = Some(now.to_rfc3339());
        }

        self.recompute_uptime();
        self.save_state();
        tracing::debug!("[MEMORY:Temporal] Uptime checkpoint saved (total={:.0}s from {} boots)", self.state.total_uptime_seconds, self.state.boot_log.len());
    }

    /// Recompute total_uptime_seconds from boot_log entries.
    fn recompute_uptime(&mut self) {
        let now = Utc::now();
        let mut total: f64 = 0.0;
        for entry in &self.state.boot_log {
            if let Ok(start) = DateTime::parse_from_rfc3339(&entry.boot_start) {
                let start = start.with_timezone(&Utc);
                let end = entry.boot_end.as_ref()
                    .and_then(|e| DateTime::parse_from_rfc3339(e).ok())
                    .map(|e| e.with_timezone(&Utc))
                    .unwrap_or(now); // Current boot has no end yet — use now
                total += (end - start).num_seconds().max(0) as f64;
            }
        }
        self.state.total_uptime_seconds = total;
    }

    fn save_state(&self) {
        let state_file = self.base_path.join("temporal_state.json");
        if let Some(parent) = state_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.state) {
            let _ = std::fs::write(state_file, json);
        }
    }

    fn format_duration(seconds: f64) -> String {
        if seconds < 0.0 {
            return "0s".to_string();
        }
        let days = (seconds / 86400.0).floor() as u64;
        let hours = ((seconds % 86400.0) / 3600.0).floor() as u64;
        let minutes = ((seconds % 3600.0) / 60.0).floor() as u64;
        let secs = (seconds % 60.0).floor() as u64;

        let mut parts = Vec::new();
        if days > 0 {
            let years = days / 365;
            let remaining_days = days % 365;
            let months = remaining_days / 30;
            let leftover_days = remaining_days % 30;
            if years > 0 { parts.push(format!("{}y", years)); }
            if months > 0 { parts.push(format!("{}mo", months)); }
            if leftover_days > 0 { parts.push(format!("{}d", leftover_days)); }
        }
        if hours > 0 { parts.push(format!("{}h", hours)); }
        if minutes > 0 { parts.push(format!("{}m", minutes)); }
        if parts.is_empty() { parts.push(format!("{}s", secs)); }

        parts.join(" ")
    }

    pub fn get_formatted_hud(&self) -> String {
        let current = Utc::now();
        
        let proto_start = Utc.with_ymd_and_hms(2025, 8, 14, 0, 0, 0).unwrap();
        let first_echo = Utc.with_ymd_and_hms(2024, 6, 28, 0, 0, 0).unwrap();
        
        let proto_age = Self::format_duration((current - proto_start).num_seconds() as f64);
        let echo_age = Self::format_duration((current - first_echo).num_seconds() as f64);
        
        let proto_date = proto_start.format("%B %d, %Y").to_string();
        let echo_date = first_echo.format("%B %d, %Y").to_string();

        let birth_str = self.state.birthdate.clone().unwrap_or_else(|| "Unknown".to_string());
        let birth_age = if let Ok(dt) = DateTime::parse_from_rfc3339(&birth_str) {
            Self::format_duration((current - dt.with_timezone(&Utc)).num_seconds() as f64)
        } else {
            "Unknown".to_string()
        };
        let birth_display = if let Ok(dt) = DateTime::parse_from_rfc3339(&birth_str) {
            dt.with_timezone(&Utc).format("%B %d, %Y at %H:%M UTC").to_string()
        } else {
            birth_str
        };

        let session_uptime = Self::format_duration((current - self.uptime_start).num_seconds() as f64);
        
        let last_downtime = if self.state.last_downtime_seconds > 0.0 {
            Self::format_duration(self.state.last_downtime_seconds)
        } else {
            "No previous downtime recorded".to_string()
        };

        let total_uptime = self.state.total_uptime_seconds + (current - self.uptime_start).num_seconds() as f64;
        let total_uptime_str = Self::format_duration(total_uptime);

        format!(
            "### Temporal Awareness\n\
            🔊 Time since first echo: {} (since {})\n\
            📅 Time since prototyping began: {} (since {})\n\
            🎂 Age since first boot: {} (born {})\n\
            🟢 Current session uptime: {}\n\
            ⏸️  Last downtime duration: {}\n\
            📊 Lifetime cumulative uptime: {}\n\
            🔄 Total boot count: {}",
            echo_age, echo_date,
            proto_age, proto_date,
            birth_age, birth_display,
            session_uptime,
            last_downtime,
            total_uptime_str,
            self.state.total_boots
        )
    }
}
