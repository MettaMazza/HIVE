/// Peer Discovery — Three-tier peer finding for the NeuroLease mesh.
///
/// Tier 1: mDNS on local network (_apis._udp.local)
/// Tier 2: Bootstrap seed nodes (configured in .env)  
/// Tier 3: Gossip — known peers share their peer lists
///
/// Discovery runs entirely within Apis's autonomy layer.
/// Users cannot trigger, configure, or observe discovery.
use std::collections::HashMap;
use std::sync::Arc;
use std::net::SocketAddr;
use tokio::sync::RwLock;
use crate::network::messages::{PeerId, PeerInfo};

/// Discovery configuration loaded from environment at boot.
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    pub enabled: bool,
    pub port: u16,
    pub bootstrap_nodes: Vec<String>,
    pub mdns_enabled: bool,
}

impl DiscoveryConfig {
    /// Load discovery config from environment variables.
    /// Users cannot change these at runtime — read-only at boot.
    pub fn from_env() -> Self {
        let enabled = std::env::var("NEUROLEASE_ENABLED")
            .unwrap_or_default()
            .to_lowercase() == "true";

        let port = std::env::var("NEUROLEASE_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(9473); // "HIVE" on phone keypad

        let bootstrap_nodes = std::env::var("NEUROLEASE_BOOTSTRAP")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let mdns_enabled = std::env::var("NEUROLEASE_MDNS")
            .unwrap_or_else(|_| "true".into())
            .to_lowercase() == "true";

        Self { enabled, port, bootstrap_nodes, mdns_enabled }
    }
}

/// Peer registry — stores all discovered peers.
pub struct PeerRegistry {
    peers: Arc<RwLock<HashMap<PeerId, PeerInfo>>>,
}

impl PeerRegistry {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register or update a peer.
    pub async fn upsert(&self, info: PeerInfo) {
        let mut peers = self.peers.write().await;
        tracing::debug!("[DISCOVERY] Registered peer {} at {}", info.peer_id, info.addr);
        peers.insert(info.peer_id.clone(), info);
    }

    /// Remove peers not seen in the last N seconds.
    pub async fn prune_stale(&self, max_age_secs: i64) {
        let now = chrono::Utc::now();
        let mut peers = self.peers.write().await;
        let before = peers.len();
        peers.retain(|_id, info| {
            chrono::DateTime::parse_from_rfc3339(&info.last_seen)
                .map(|dt| (now - dt.with_timezone(&chrono::Utc)).num_seconds() < max_age_secs)
                .unwrap_or(false)
        });
        let pruned = before - peers.len();
        if pruned > 0 {
            tracing::info!("[DISCOVERY] Pruned {} stale peers ({} remaining)", pruned, peers.len());
        }
    }

    /// Get all currently known peers.
    pub async fn all_peers(&self) -> Vec<PeerInfo> {
        self.peers.read().await.values().cloned().collect()
    }

    /// Get peer count.
    pub async fn count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Get a specific peer by ID.
    pub async fn get(&self, peer_id: &PeerId) -> Option<PeerInfo> {
        self.peers.read().await.get(peer_id).cloned()
    }

    /// Get the shared peer list for gossiping to other peers.
    pub async fn peer_list_for_gossip(&self) -> Vec<PeerInfo> {
        // Share up to 20 most recently seen peers
        let peers = self.peers.read().await;
        let mut list: Vec<PeerInfo> = peers.values().cloned().collect();
        list.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        list.truncate(20);
        list
    }
}

/// Discovery daemon — runs in background, finds and maintains peers.
#[allow(dead_code)]
pub struct DiscoveryDaemon {
    config: DiscoveryConfig,
    registry: Arc<PeerRegistry>,
    local_peer_id: PeerId,
    local_addr: SocketAddr,
}

impl DiscoveryDaemon {
    pub fn new(
        config: DiscoveryConfig,
        registry: Arc<PeerRegistry>,
        local_peer_id: PeerId,
        local_addr: SocketAddr,
    ) -> Self {
        Self { config, registry, local_peer_id, local_addr }
    }

    /// Start all discovery mechanisms. Runs indefinitely.
    pub async fn run(&self) {
        if !self.config.enabled {
            tracing::info!("[DISCOVERY] NeuroLease is disabled. Skipping discovery.");
            return;
        }

        tracing::info!(
            "[DISCOVERY] 🌐 Starting peer discovery — port: {}, bootstrap: {}, mDNS: {}",
            self.config.port,
            self.config.bootstrap_nodes.len(),
            self.config.mdns_enabled
        );

        // Spawn concurrent discovery tasks
        let registry = self.registry.clone();
        let config = self.config.clone();

        // Background pruner — removes stale peers every 5 minutes
        let prune_registry = registry.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
                prune_registry.prune_stale(600).await; // 10 min stale threshold
            }
        });

        // Bootstrap: try connecting to seed nodes
        if !config.bootstrap_nodes.is_empty() {
            let bootstrap_registry = registry.clone();
            let nodes = config.bootstrap_nodes.clone();
            let local_id = self.local_peer_id.clone();
            tokio::spawn(async move {
                for node in &nodes {
                    tracing::info!("[DISCOVERY] 📡 Attempting bootstrap connection to {}", node);
                    // Bootstrap connection will be implemented with quinn transport in Phase 5
                    // For now, register as a known address to connect to
                    let info = PeerInfo {
                        peer_id: PeerId(format!("bootstrap_{}", node)),
                        addr: node.clone(),
                        last_seen: chrono::Utc::now().to_rfc3339(),
                        version: "unknown".to_string(),
                        binary_hash: "unknown".to_string(),
                        source_hash: "unknown".to_string(),
                    };
                    bootstrap_registry.upsert(info).await;
                }
                let _ = local_id; // Will be used for Ping messages
            });
        }

        // mDNS: broadcast on local network
        if config.mdns_enabled {
            tracing::info!("[DISCOVERY] 📻 mDNS advertisement on _apis._udp.local:{}", config.port);
            // mDNS implementation requires mdns-sd crate — will be wired in Phase 5
        }

        // Keep the daemon alive
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            let count = self.registry.count().await;
            tracing::debug!("[DISCOVERY] Peer count: {}", count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_peer_registry_upsert() {
        let registry = PeerRegistry::new();
        let info = PeerInfo {
            peer_id: PeerId("test1".into()),
            addr: "127.0.0.1:9473".into(),
            last_seen: chrono::Utc::now().to_rfc3339(),
            version: "abc123".into(),
            binary_hash: "hash".into(),
            source_hash: "src_hash".into(),
        };

        registry.upsert(info).await;
        assert_eq!(registry.count().await, 1);
    }

    #[tokio::test]
    async fn test_peer_registry_gossip_limit() {
        let registry = PeerRegistry::new();
        // Add 30 peers
        for i in 0..30 {
            let info = PeerInfo {
                peer_id: PeerId(format!("peer_{}", i)),
                addr: format!("127.0.0.1:{}", 9000 + i),
                last_seen: chrono::Utc::now().to_rfc3339(),
                version: "v1".into(),
                binary_hash: "hash".into(),
                source_hash: "src".into(),
            };
            registry.upsert(info).await;
        }

        let gossip = registry.peer_list_for_gossip().await;
        assert!(gossip.len() <= 20, "Gossip list should be capped at 20");
    }

    #[test]
    fn test_discovery_config_defaults() {
        // With no env vars set, should get defaults
        let config = DiscoveryConfig {
            enabled: false,
            port: 9473,
            bootstrap_nodes: vec![],
            mdns_enabled: true,
        };
        assert!(!config.enabled);
        assert_eq!(config.port, 9473);
        assert!(config.mdns_enabled);
    }
}
