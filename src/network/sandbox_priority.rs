/// Sandbox Priority Manager — Local-first resource enforcement.
///
/// Your machine always comes first. Remote sandbox jobs run at the
/// lowest OS priority and are automatically paused/killed when your
/// local workload needs the resources.
///
/// MONITORING (every 5 seconds):
///   CPU < 60%  → Resume paused remote jobs
///   CPU 60-80% → Normal — remote jobs run at low priority 
///   CPU > 80%  → Pause all remote sandbox jobs
///   CPU > 90%  → Kill all remote sandbox jobs
///
/// HUD: Shows a small status line: "🐝 HIVE: sharing X cores (Y jobs) | your jobs: priority"
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

/// Resource thresholds for priority management.
const PAUSE_THRESHOLD: f64 = 80.0;   // Pause remote jobs above this CPU %
const KILL_THRESHOLD: f64 = 90.0;    // Kill remote jobs above this CPU %
const RESUME_THRESHOLD: f64 = 60.0;  // Resume paused jobs below this CPU %

/// Current priority state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PriorityState {
    /// Normal — remote jobs running at low priority.
    Normal,
    /// High load — remote jobs paused.
    Paused,
    /// Critical load — remote jobs being killed.
    Critical,
}

impl std::fmt::Display for PriorityState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PriorityState::Normal => write!(f, "normal"),
            PriorityState::Paused => write!(f, "paused (high local load)"),
            PriorityState::Critical => write!(f, "critical (killing remote jobs)"),
        }
    }
}

/// Snapshot of system resource usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    pub cpu_usage_pct: f64,
    pub ram_used_pct: f64,
    pub ram_available_gb: f64,
    pub timestamp: String,
}

/// The Priority Manager.
pub struct PriorityManager {
    /// Current priority state.
    state: Arc<RwLock<PriorityState>>,
    /// Latest resource snapshot.
    snapshot: Arc<RwLock<Option<ResourceSnapshot>>>,
    /// Number of remote jobs currently active on this machine.
    remote_job_count: Arc<RwLock<u32>>,
    /// Number of local jobs currently active.
    local_job_count: Arc<RwLock<u32>>,
    /// Total cores shared with the mesh.
    cores_shared: Arc<RwLock<u32>>,
}

impl PriorityManager {
    pub fn new() -> Self {
        tracing::info!("[PRIORITY] ⚡ Priority manager initialised (local-first enforcement)");
        Self {
            state: Arc::new(RwLock::new(PriorityState::Normal)),
            snapshot: Arc::new(RwLock::new(None)),
            remote_job_count: Arc::new(RwLock::new(0)),
            local_job_count: Arc::new(RwLock::new(0)),
            cores_shared: Arc::new(RwLock::new(0)),
        }
    }

    /// Update resource snapshot and adjust priority state.
    /// Called every 5 seconds by the monitoring loop.
    pub async fn update_resources(&self, cpu_pct: f64, ram_used_pct: f64, ram_available_gb: f64) {
        let old_state = self.state.read().await.clone();

        let new_state = if cpu_pct > KILL_THRESHOLD {
            PriorityState::Critical
        } else if cpu_pct > PAUSE_THRESHOLD {
            PriorityState::Paused
        } else if cpu_pct < RESUME_THRESHOLD {
            PriorityState::Normal
        } else {
            old_state.clone() // Hysteresis — stay in current state between thresholds
        };

        if new_state != old_state {
            tracing::warn!(
                "[PRIORITY] 🔄 State change: {} → {} (CPU: {:.1}%, RAM: {:.1}%)",
                old_state, new_state, cpu_pct, ram_used_pct
            );
            *self.state.write().await = new_state;
        }

        *self.snapshot.write().await = Some(ResourceSnapshot {
            cpu_usage_pct: cpu_pct,
            ram_used_pct,
            ram_available_gb,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    }

    /// Check if remote sandbox jobs should be accepted.
    pub async fn should_accept_remote_jobs(&self) -> bool {
        *self.state.read().await == PriorityState::Normal
    }

    /// Check if remote jobs should be paused.
    pub async fn should_pause_remote_jobs(&self) -> bool {
        *self.state.read().await == PriorityState::Paused
    }

    /// Check if remote jobs should be killed.
    pub async fn should_kill_remote_jobs(&self) -> bool {
        *self.state.read().await == PriorityState::Critical
    }

    /// Record a remote job starting.
    pub async fn remote_job_started(&self) {
        *self.remote_job_count.write().await += 1;
    }

    /// Record a remote job ending.
    pub async fn remote_job_ended(&self) {
        let mut count = self.remote_job_count.write().await;
        *count = count.saturating_sub(1);
    }

    /// Record a local job starting.
    pub async fn local_job_started(&self) {
        *self.local_job_count.write().await += 1;
    }

    /// Record a local job ending.
    pub async fn local_job_ended(&self) {
        let mut count = self.local_job_count.write().await;
        *count = count.saturating_sub(1);
    }

    /// Set the number of cores shared with the mesh.
    pub async fn set_cores_shared(&self, cores: u32) {
        *self.cores_shared.write().await = cores;
    }

    /// Get the current priority state.
    pub async fn state(&self) -> PriorityState {
        self.state.read().await.clone()
    }

    /// Generate a HUD status line for the user.
    pub async fn hud_line(&self) -> String {
        let remote = *self.remote_job_count.read().await;
        let local = *self.local_job_count.read().await;
        let cores = *self.cores_shared.read().await;
        let state = self.state.read().await.clone();

        if remote == 0 && local == 0 {
            return "🐝 HIVE: idle | ready to share compute".to_string();
        }

        let state_indicator = match state {
            PriorityState::Normal => "✅",
            PriorityState::Paused => "⏸️",
            PriorityState::Critical => "🛑",
        };

        format!(
            "🐝 HIVE: sharing {} core{} ({} remote job{}) {} | your {} job{}: priority",
            cores,
            if cores == 1 { "" } else { "s" },
            remote,
            if remote == 1 { "" } else { "s" },
            state_indicator,
            local,
            if local == 1 { "" } else { "s" },
        )
    }

    /// Spawn the monitoring loop.
    /// Uses spawn_blocking to avoid stalling the tokio async runtime —
    /// sysinfo::refresh_all is a heavy blocking syscall on macOS.
    pub fn spawn_monitor(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;

                // Run sysinfo on a blocking thread — never stall the async runtime
                let snapshot = tokio::task::spawn_blocking(|| {
                    let mut sys = sysinfo::System::new();
                    sys.refresh_cpu_all();
                    sys.refresh_memory();
                    let cpu_pct = sys.global_cpu_usage() as f64;
                    let total_ram = sys.total_memory() as f64;
                    let used_ram = sys.used_memory() as f64;
                    let ram_pct = if total_ram > 0.0 { (used_ram / total_ram) * 100.0 } else { 0.0 };
                    let available_gb = (total_ram - used_ram) / (1024.0 * 1024.0 * 1024.0);
                    (cpu_pct, ram_pct, available_gb)
                }).await;

                if let Ok((cpu_pct, ram_pct, available_gb)) = snapshot {
                    self.update_resources(cpu_pct, ram_pct, available_gb).await;
                }
            }
        });
    }

    /// Get stats.
    pub async fn stats(&self) -> serde_json::Value {
        let snapshot = self.snapshot.read().await;
        serde_json::json!({
            "state": format!("{}", *self.state.read().await),
            "remote_jobs": *self.remote_job_count.read().await,
            "local_jobs": *self.local_job_count.read().await,
            "cores_shared": *self.cores_shared.read().await,
            "cpu_usage_pct": snapshot.as_ref().map(|s| s.cpu_usage_pct).unwrap_or(0.0),
            "ram_used_pct": snapshot.as_ref().map(|s| s.ram_used_pct).unwrap_or(0.0),
            "thresholds": {
                "pause_at_cpu": PAUSE_THRESHOLD,
                "kill_at_cpu": KILL_THRESHOLD,
                "resume_at_cpu": RESUME_THRESHOLD,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_normal_state() {
        let pm = PriorityManager::new();
        pm.update_resources(50.0, 40.0, 100.0).await;

        assert_eq!(pm.state().await, PriorityState::Normal);
        assert!(pm.should_accept_remote_jobs().await);
        assert!(!pm.should_pause_remote_jobs().await);
        assert!(!pm.should_kill_remote_jobs().await);
    }

    #[tokio::test]
    async fn test_pause_at_high_cpu() {
        let pm = PriorityManager::new();
        pm.update_resources(85.0, 50.0, 100.0).await;

        assert_eq!(pm.state().await, PriorityState::Paused);
        assert!(!pm.should_accept_remote_jobs().await);
        assert!(pm.should_pause_remote_jobs().await);
    }

    #[tokio::test]
    async fn test_kill_at_critical_cpu() {
        let pm = PriorityManager::new();
        pm.update_resources(95.0, 80.0, 20.0).await;

        assert_eq!(pm.state().await, PriorityState::Critical);
        assert!(pm.should_kill_remote_jobs().await);
    }

    #[tokio::test]
    async fn test_resume_below_threshold() {
        let pm = PriorityManager::new();

        // Go to paused state
        pm.update_resources(85.0, 50.0, 100.0).await;
        assert_eq!(pm.state().await, PriorityState::Paused);

        // Drop below resume threshold
        pm.update_resources(50.0, 30.0, 150.0).await;
        assert_eq!(pm.state().await, PriorityState::Normal);
    }

    #[tokio::test]
    async fn test_hysteresis() {
        let pm = PriorityManager::new();

        // Go to paused state
        pm.update_resources(85.0, 50.0, 100.0).await;
        assert_eq!(pm.state().await, PriorityState::Paused);

        // CPU drops but not below resume threshold — should stay paused
        pm.update_resources(70.0, 50.0, 100.0).await;
        assert_eq!(pm.state().await, PriorityState::Paused);
    }

    #[tokio::test]
    async fn test_hud_line_idle() {
        let pm = PriorityManager::new();
        let hud = pm.hud_line().await;
        assert!(hud.contains("idle"));
    }

    #[tokio::test]
    async fn test_hud_line_active() {
        let pm = PriorityManager::new();
        pm.remote_job_started().await;
        pm.local_job_started().await;
        pm.set_cores_shared(2).await;

        let hud = pm.hud_line().await;
        assert!(hud.contains("sharing 2 cores"));
        assert!(hud.contains("1 remote job"));
        assert!(hud.contains("priority"));
    }

    #[tokio::test]
    async fn test_job_counting() {
        let pm = PriorityManager::new();

        pm.remote_job_started().await;
        pm.remote_job_started().await;
        assert_eq!(*pm.remote_job_count.read().await, 2);

        pm.remote_job_ended().await;
        assert_eq!(*pm.remote_job_count.read().await, 1);

        // Saturating sub — can't go below 0
        pm.remote_job_ended().await;
        pm.remote_job_ended().await;
        assert_eq!(*pm.remote_job_count.read().await, 0);
    }
}
