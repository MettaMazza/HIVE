/// Dynamic Pricing Engine — Real-time credit cost/reward adjustment.
///
/// Monitors supply and demand for compute and network resources on the mesh,
/// then adjusts credit earn rates and spend costs accordingly.
///
/// HIGH DEMAND (>80% capacity used):
///   - Providers earn MORE (incentivises sharing)
///   - Consumers pay MORE (prevents overload)
///
/// MODERATE DEMAND (50-80%):
///   - Slight premium on both sides
///
/// LOW DEMAND (<50%):
///   - Base rates for everyone
///
/// Updates every 60 seconds from PoolManager stats.
/// All pricing data is local, never shared with peers.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Resource type for pricing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    Compute,
    Network,
}

impl std::fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceKind::Compute => write!(f, "compute"),
            ResourceKind::Network => write!(f, "network"),
        }
    }
}

/// Demand level determined by capacity utilisation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DemandLevel {
    Low,       // < 50% capacity used
    Moderate,  // 50% – 80%
    High,      // > 80%
}

impl std::fmt::Display for DemandLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DemandLevel::Low => write!(f, "low"),
            DemandLevel::Moderate => write!(f, "moderate"),
            DemandLevel::High => write!(f, "high"),
        }
    }
}

/// Snapshot of demand for a single resource type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandSnapshot {
    pub resource: ResourceKind,
    pub level: DemandLevel,
    /// Ratio of active usage to available capacity (0.0 – 1.0).
    pub utilisation: f64,
    /// Current earn multiplier for providers.
    pub earn_multiplier: f64,
    /// Current cost multiplier for consumers.
    pub cost_multiplier: f64,
    /// Timestamp of this snapshot.
    pub updated_at: String,
}

/// Pricing configuration thresholds.
#[derive(Debug, Clone)]
pub struct PricingConfig {
    /// Utilisation ratio above which demand is "high".
    pub high_threshold: f64,
    /// Utilisation ratio above which demand is "moderate".
    pub moderate_threshold: f64,
    /// Multiplier applied during high demand.
    pub high_multiplier: f64,
    /// Multiplier applied during moderate demand.
    pub moderate_multiplier: f64,
}

impl PricingConfig {
    pub fn from_env() -> Self {
        Self {
            high_threshold: env_f64("REMOVED_MESH_GOVERNEDHIGH_THRESHOLD", 0.8),
            moderate_threshold: env_f64("REMOVED_MESH_GOVERNEDMODERATE_THRESHOLD", 0.5),
            high_multiplier: env_f64("REMOVED_MESH_GOVERNED", 1.5),
            moderate_multiplier: env_f64("REMOVED_MESH_GOVERNED", 1.2),
        }
    }
}

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// The Dynamic Pricing Engine.
///
/// Holds current demand snapshots for compute and network resources.
/// Updated periodically by the mesh engine from PoolManager stats.
pub struct DynamicPricing {
    config: PricingConfig,
    compute_demand: Arc<RwLock<DemandSnapshot>>,
    network_demand: Arc<RwLock<DemandSnapshot>>,
}

impl DynamicPricing {
    pub fn new() -> Self {
        let config = PricingConfig::from_env();
        let now = chrono::Utc::now().to_rfc3339();

        Self {
            compute_demand: Arc::new(RwLock::new(DemandSnapshot {
                resource: ResourceKind::Compute,
                level: DemandLevel::Low,
                utilisation: 0.0,
                earn_multiplier: 1.0,
                cost_multiplier: 1.0,
                updated_at: now.clone(),
            })),
            network_demand: Arc::new(RwLock::new(DemandSnapshot {
                resource: ResourceKind::Network,
                level: DemandLevel::Low,
                utilisation: 0.0,
                earn_multiplier: 1.0,
                cost_multiplier: 1.0,
                updated_at: now,
            })),
            config,
        }
    }

    /// Update compute demand based on pool stats.
    ///
    /// `active_jobs` — number of compute jobs currently running.
    /// `total_slots` — total available compute slots across all peers.
    pub async fn update_compute_demand(&self, active_jobs: u32, total_slots: u32) {
        let utilisation = if total_slots == 0 {
            0.0
        } else {
            active_jobs as f64 / total_slots as f64
        };

        let (level, multiplier) = self.classify(utilisation);

        let mut demand = self.compute_demand.write().await;
        demand.level = level;
        demand.utilisation = utilisation;
        demand.earn_multiplier = multiplier;
        demand.cost_multiplier = multiplier;
        demand.updated_at = chrono::Utc::now().to_rfc3339();

        if level != DemandLevel::Low {
            tracing::info!("[PRICING] 🖥️ Compute demand: {} ({:.0}% utilised, ×{:.1})",
                level, utilisation * 100.0, multiplier);
        }
    }

    /// Update network demand based on pool stats.
    ///
    /// `active_requests` — number of pending/active relay requests.
    /// `available_relays` — number of available relay peers.
    pub async fn update_network_demand(&self, active_requests: u32, available_relays: u32) {
        // For network, "capacity" is relays × max_req/hour
        // Simplified: use relay count as rough capacity indicator
        let capacity = (available_relays as f64 * 100.0).max(1.0); // 100 req/relay/hour baseline
        let utilisation = active_requests as f64 / capacity;
        let utilisation = utilisation.min(1.0);

        let (level, multiplier) = self.classify(utilisation);

        let mut demand = self.network_demand.write().await;
        demand.level = level;
        demand.utilisation = utilisation;
        demand.earn_multiplier = multiplier;
        demand.cost_multiplier = multiplier;
        demand.updated_at = chrono::Utc::now().to_rfc3339();

        if level != DemandLevel::Low {
            tracing::info!("[PRICING] 🌐 Network demand: {} ({:.0}% utilised, ×{:.1})",
                level, utilisation * 100.0, multiplier);
        }
    }

    /// Classify utilisation into a demand level and multiplier.
    fn classify(&self, utilisation: f64) -> (DemandLevel, f64) {
        if utilisation > self.config.high_threshold {
            (DemandLevel::High, self.config.high_multiplier)
        } else if utilisation > self.config.moderate_threshold {
            (DemandLevel::Moderate, self.config.moderate_multiplier)
        } else {
            (DemandLevel::Low, 1.0)
        }
    }

    /// Get current demand multiplier for a resource type.
    /// This is what the credits engine uses when earning/spending.
    pub async fn multiplier(&self, resource: ResourceKind) -> f64 {
        match resource {
            ResourceKind::Compute => self.compute_demand.read().await.earn_multiplier,
            ResourceKind::Network => self.network_demand.read().await.earn_multiplier,
        }
    }

    /// Get full demand snapshots for display.
    pub async fn snapshots(&self) -> (DemandSnapshot, DemandSnapshot) {
        let compute = self.compute_demand.read().await.clone();
        let network = self.network_demand.read().await.clone();
        (compute, network)
    }

    /// Get demand stats as JSON for dashboards/APIs.
    pub async fn stats(&self) -> serde_json::Value {
        let (compute, network) = self.snapshots().await;
        serde_json::json!({
            "compute": {
                "level": compute.level.to_string(),
                "utilisation": format!("{:.1}%", compute.utilisation * 100.0),
                "earn_multiplier": compute.earn_multiplier,
                "cost_multiplier": compute.cost_multiplier,
                "updated_at": compute.updated_at,
            },
            "network": {
                "level": network.level.to_string(),
                "utilisation": format!("{:.1}%", network.utilisation * 100.0),
                "earn_multiplier": network.earn_multiplier,
                "cost_multiplier": network.cost_multiplier,
                "updated_at": network.updated_at,
            },
        })
    }

    /// Spawn a background task that periodically updates demand from pool stats.
    /// Returns a handle that can be used to stop the task.
    pub fn spawn_updater(
        pricing: Arc<DynamicPricing>,
        pool_stats_fn: Arc<dyn Fn() -> (u32, u32, u32, u32) + Send + Sync>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
            loop {
                interval.tick().await;

                let (active_jobs, total_slots, active_requests, available_relays) = pool_stats_fn();
                pricing.update_compute_demand(active_jobs, total_slots).await;
                pricing.update_network_demand(active_requests, available_relays).await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_low_demand() {
        let pricing = DynamicPricing::new();
        pricing.update_compute_demand(1, 10).await; // 10% utilisation

        let mult = pricing.multiplier(ResourceKind::Compute).await;
        assert_eq!(mult, 1.0);

        let (compute, _) = pricing.snapshots().await;
        assert_eq!(compute.level, DemandLevel::Low);
    }

    #[tokio::test]
    async fn test_moderate_demand() {
        let pricing = DynamicPricing::new();
        pricing.update_compute_demand(6, 10).await; // 60% utilisation

        let mult = pricing.multiplier(ResourceKind::Compute).await;
        assert_eq!(mult, 1.2);

        let (compute, _) = pricing.snapshots().await;
        assert_eq!(compute.level, DemandLevel::Moderate);
    }

    #[tokio::test]
    async fn test_high_demand() {
        let pricing = DynamicPricing::new();
        pricing.update_compute_demand(9, 10).await; // 90% utilisation

        let mult = pricing.multiplier(ResourceKind::Compute).await;
        assert_eq!(mult, 1.5);

        let (compute, _) = pricing.snapshots().await;
        assert_eq!(compute.level, DemandLevel::High);
    }

    #[tokio::test]
    async fn test_network_demand() {
        let pricing = DynamicPricing::new();
        pricing.update_network_demand(250, 3).await; // 250 / (3×100) = 83%

        let mult = pricing.multiplier(ResourceKind::Network).await;
        assert_eq!(mult, 1.5); // High demand
    }

    #[tokio::test]
    async fn test_zero_capacity() {
        let pricing = DynamicPricing::new();
        pricing.update_compute_demand(5, 0).await; // No capacity

        let mult = pricing.multiplier(ResourceKind::Compute).await;
        assert_eq!(mult, 1.0); // Defaults to low
    }

    #[tokio::test]
    async fn test_stats_json() {
        let pricing = DynamicPricing::new();
        pricing.update_compute_demand(8, 10).await;
        pricing.update_network_demand(10, 5).await;

        let stats = pricing.stats().await;
        assert!(stats["compute"]["level"].as_str().is_some());
        assert!(stats["network"]["level"].as_str().is_some());
    }
}
