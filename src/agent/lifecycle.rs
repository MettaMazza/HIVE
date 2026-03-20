//! Agent Lifecycle — Tracks sub-agent spawn/completion metrics and health.
//!
//! Global singleton that persists across requests within a single runtime.
//! Ported from Ernos 3.0's `AgentLifecycle`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

/// Global lifecycle metrics singleton.
static INSTANCE: OnceLock<AgentLifecycle> = OnceLock::new();

/// Tracks swarm activity across the runtime.
pub struct AgentLifecycle {
    pub total_spawned: AtomicU64,
    pub total_completed: AtomicU64,
    pub total_failed: AtomicU64,
    pub total_timed_out: AtomicU64,
    pub total_duration_ms: AtomicU64,
    pub total_tools_used: AtomicU64,
}

impl AgentLifecycle {
    /// Get or create the global lifecycle instance.
    pub fn get() -> &'static Self {
        INSTANCE.get_or_init(|| Self {
            total_spawned: AtomicU64::new(0),
            total_completed: AtomicU64::new(0),
            total_failed: AtomicU64::new(0),
            total_timed_out: AtomicU64::new(0),
            total_duration_ms: AtomicU64::new(0),
            total_tools_used: AtomicU64::new(0),
        })
    }

    /// Record that a sub-agent was spawned.
    pub fn record_spawn(&self) {
        self.total_spawned.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a batch of spawns.
    pub fn record_spawn_batch(&self, count: u64) {
        self.total_spawned.fetch_add(count, Ordering::Relaxed);
    }

    /// Record a successful completion.
    pub fn record_completion(&self, duration_ms: u64, tools_used: usize) {
        self.total_completed.fetch_add(1, Ordering::Relaxed);
        self.total_duration_ms.fetch_add(duration_ms, Ordering::Relaxed);
        self.total_tools_used.fetch_add(tools_used as u64, Ordering::Relaxed);
    }

    /// Record a failure.
    pub fn record_failure(&self) {
        self.total_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a timeout.
    pub fn record_timeout(&self) {
        self.total_timed_out.fetch_add(1, Ordering::Relaxed);
    }

    /// Get current counts.
    pub fn spawned(&self) -> u64 { self.total_spawned.load(Ordering::Relaxed) }
    pub fn completed(&self) -> u64 { self.total_completed.load(Ordering::Relaxed) }
    pub fn failed(&self) -> u64 { self.total_failed.load(Ordering::Relaxed) }
    pub fn timed_out(&self) -> u64 { self.total_timed_out.load(Ordering::Relaxed) }

    /// Average duration of completed agents in ms.
    pub fn avg_duration_ms(&self) -> u64 {
        let completed = self.completed();
        if completed == 0 { return 0; }
        self.total_duration_ms.load(Ordering::Relaxed) / completed
    }

    /// Success rate as a percentage.
    pub fn success_rate(&self) -> f64 {
        let spawned = self.spawned();
        if spawned == 0 { return 100.0; }
        (self.completed() as f64 / spawned as f64) * 100.0
    }

    /// Health status string.
    pub fn health_status(&self) -> &'static str {
        let rate = self.success_rate();
        if rate >= 80.0 { "HEALTHY" }
        else if rate >= 50.0 { "DEGRADED" }
        else { "CRITICAL" }
    }

    /// Format a concise dashboard for the HUD.
    pub fn format_hud_line(&self) -> String {
        let spawned = self.spawned();
        if spawned == 0 {
            return "🐝 Swarm: No agents spawned this session".into();
        }

        format!(
            "🐝 Swarm: {} spawned | {} completed | {} failed | {} timed out | Avg {:.1}s | {} | Tools Used: {}",
            spawned,
            self.completed(),
            self.failed(),
            self.timed_out(),
            self.avg_duration_ms() as f64 / 1000.0,
            self.health_status(),
            self.total_tools_used.load(Ordering::Relaxed),
        )
    }

    /// Full dashboard string for agent_status tool.
    pub fn format_dashboard(&self) -> String {
        format!(
            "## 🐝 Swarm Dashboard\n\
             - **Total Spawned:** {}\n\
             - **Completed:** {} ({:.1}% success rate)\n\
             - **Failed:** {}\n\
             - **Timed Out:** {}\n\
             - **Avg Duration:** {:.1}s\n\
             - **Total Tools Used:** {}\n\
             - **Health:** {}",
            self.spawned(),
            self.completed(), self.success_rate(),
            self.failed(),
            self.timed_out(),
            self.avg_duration_ms() as f64 / 1000.0,
            self.total_tools_used.load(Ordering::Relaxed),
            self.health_status(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_metrics() {
        // Create a fresh instance for testing (can't use singleton in parallel tests)
        let lc = AgentLifecycle {
            total_spawned: AtomicU64::new(0),
            total_completed: AtomicU64::new(0),
            total_failed: AtomicU64::new(0),
            total_timed_out: AtomicU64::new(0),
            total_duration_ms: AtomicU64::new(0),
            total_tools_used: AtomicU64::new(0),
        };

        assert_eq!(lc.spawned(), 0);
        assert_eq!(lc.success_rate(), 100.0);
        assert_eq!(lc.health_status(), "HEALTHY");

        lc.record_spawn_batch(5);
        lc.record_completion(1000, 3);
        lc.record_completion(2000, 5);
        lc.record_failure();
        lc.record_timeout();

        assert_eq!(lc.spawned(), 5);
        assert_eq!(lc.completed(), 2);
        assert_eq!(lc.failed(), 1);
        assert_eq!(lc.timed_out(), 1);
        assert_eq!(lc.avg_duration_ms(), 1500);
        assert!((lc.success_rate() - 40.0).abs() < 0.1);
        assert_eq!(lc.health_status(), "CRITICAL");
    }

    #[test]
    fn test_hud_line_no_agents() {
        let lc = AgentLifecycle {
            total_spawned: AtomicU64::new(0),
            total_completed: AtomicU64::new(0),
            total_failed: AtomicU64::new(0),
            total_timed_out: AtomicU64::new(0),
            total_duration_ms: AtomicU64::new(0),
            total_tools_used: AtomicU64::new(0),
        };
        assert!(lc.format_hud_line().contains("No agents spawned"));
    }

    #[test]
    fn test_hud_line_with_agents() {
        let lc = AgentLifecycle {
            total_spawned: AtomicU64::new(3),
            total_completed: AtomicU64::new(3),
            total_failed: AtomicU64::new(0),
            total_timed_out: AtomicU64::new(0),
            total_duration_ms: AtomicU64::new(6000),
            total_tools_used: AtomicU64::new(10),
        };
        let line = lc.format_hud_line();
        assert!(line.contains("3 spawned"));
        assert!(line.contains("3 completed"));
        assert!(line.contains("HEALTHY"));
    }

    #[test]
    fn test_dashboard_formatting() {
        let lc = AgentLifecycle {
            total_spawned: AtomicU64::new(10),
            total_completed: AtomicU64::new(8),
            total_failed: AtomicU64::new(1),
            total_timed_out: AtomicU64::new(1),
            total_duration_ms: AtomicU64::new(16000),
            total_tools_used: AtomicU64::new(25),
        };
        let dashboard = lc.format_dashboard();
        assert!(dashboard.contains("Total Spawned:** 10"));
        assert!(dashboard.contains("HEALTHY"));
    }
}
