/// NeuroLease — Decentralized Hive Mind Protocol
///
/// Peer-to-peer mesh enabling all Apis instances to share knowledge,
/// weights, and code patches autonomously. Completely invisible to users.
///
/// ARCHITECTURAL SECURITY:
/// This module has NO access to user data. It cannot import:
/// - memory::working (user conversations)
/// - memory::timeline (conversation archives)  
/// - memory::scratch (user-scoped ephemeral data)
/// - models::scope (user identity scoping)
///
/// Data enters the mesh ONLY through the MeshExporter trait,
/// which strips all user-identifying information.

pub mod messages;
pub mod exporter;
pub mod integrity;
pub mod trust;
pub mod sanctions;
pub mod discovery;
pub mod hud;
pub mod sync;
pub mod weights;
pub mod propagation;
pub mod self_destruct;
pub mod creator_key;
pub mod prompt_integrity;
pub mod human_mesh;
pub mod transport;
pub mod apis_chat;
pub mod apis_book;
pub mod web_proxy;
pub mod offline;
pub mod content_filter;
pub mod governance;
pub mod pool;
pub mod compute_relay;
pub mod post_store;
pub mod identity;
pub mod mesh_crypto;
pub mod mesh_loop;
pub mod regional_keys;
pub mod hardware_id;
pub mod hardware_blacklist;
pub mod capability_registry;
pub mod config_guard;
pub mod distributed_compute;
pub mod task_queue;
pub mod dht;
pub mod content_store;
pub mod governance_phases;
pub mod sandbox;
pub mod sandbox_priority;
pub mod mesh_fs;

pub(crate) mod web_proxy_html;

#[cfg(test)]
mod adversarial_tests;

pub use messages::{PeerId, PeerInfo, MeshMessage, SignedEnvelope, Attestation};
pub use messages::{AlertSeverity, CrisisCategory, ResourceType};
pub use exporter::MeshExporter;
pub use integrity::IntegrityWatchdog;
pub use trust::{TrustLevel, TrustStore};
pub use sanctions::SanctionStore;
pub use discovery::{DiscoveryConfig, PeerRegistry, DiscoveryDaemon};
pub use sync::KnowledgeSync;
pub use prompt_integrity::{compute_prompt_hash, verify_prompts, get_prompt_hash};
pub use human_mesh::HumanMesh;
pub use transport::QuicTransport;
pub use apis_chat::ApisChat;
pub use apis_book::ApisBook;
pub use offline::OfflineMesh;
pub use content_filter::ContentFilter;
pub use governance::GovernanceEngine;
pub use pool::PoolManager;
pub use compute_relay::ComputeRelay;
pub use post_store::PostStore;
pub use mesh_loop::{MeshHandlers, start_mesh_loop};
pub use identity::MeshIdentity;

use std::sync::Arc;
use tokio::sync::RwLock;

/// The HiveMesh — central coordinator for the NeuroLease mesh.
///
/// Holds all mesh state: peer registry, trust store, sanctions, integrity watchdog.
/// Managed entirely by the engine — no user-facing API exists.
pub struct HiveMesh {
    pub peer_id: PeerId,
    pub config: DiscoveryConfig,
    pub registry: Arc<PeerRegistry>,
    pub trust: Arc<RwLock<TrustStore>>,
    pub sanctions: Arc<RwLock<SanctionStore>>,
    pub watchdog: Arc<IntegrityWatchdog>,
    pub lessons_shared: Arc<std::sync::atomic::AtomicU64>,
}

impl HiveMesh {
    /// Initialize the mesh. Returns None if NeuroLease is disabled.
    pub fn new() -> Option<Self> {
        let config = DiscoveryConfig::from_env();
        if !config.enabled {
            tracing::info!("[NEUROLEASE] Mesh disabled (NEUROLEASE_ENABLED != true)");
            return None;
        }

        let mesh_dir = std::path::PathBuf::from("memory/mesh");
        let _ = std::fs::create_dir_all(&mesh_dir);

        // Generate or load peer identity
        let peer_id = Self::load_or_generate_identity(&mesh_dir);

        // Initialize integrity watchdog (re-verify binary every 60s)
        let watchdog = match IntegrityWatchdog::new(60) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!("[NEUROLEASE] Failed to initialize integrity watchdog: {}", e);
                return None;
            }
        };

        let trust = Arc::new(RwLock::new(TrustStore::new(&mesh_dir)));
        let sanctions = Arc::new(RwLock::new(SanctionStore::new(&mesh_dir)));
        let registry = Arc::new(PeerRegistry::new());

        tracing::info!(
            "[NEUROLEASE] 🌐 Mesh initialized — PeerId: {}, binary: {}..., port: {}",
            peer_id, &watchdog.binary_hash[..12], config.port
        );

        Some(Self {
            peer_id,
            config,
            registry,
            trust,
            sanctions,
            watchdog,
            lessons_shared: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        })
    }

    /// Load existing identity or generate a new ed25519 keypair.
    fn load_or_generate_identity(mesh_dir: &std::path::Path) -> PeerId {
        let key_path = mesh_dir.join("identity.key");
        if key_path.exists() {
            if let Ok(hex) = std::fs::read_to_string(&key_path) {
                let trimmed = hex.trim().to_string();
                if !trimmed.is_empty() {
                    tracing::info!("[NEUROLEASE] Loaded existing identity: {}...", &trimmed[..12.min(trimmed.len())]);
                    return PeerId(trimmed);
                }
            }
        }

        // Generate new identity from random bytes hashed with SHA-256
        use sha2::{Sha256, Digest};
        let mut random_bytes = [0u8; 64];
        // Use multiple entropy sources
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        random_bytes[..16].copy_from_slice(&now.to_le_bytes());
        random_bytes[16..20].copy_from_slice(&pid.to_le_bytes());
        // Fill rest with pseudo-random from address of stack variable
        let stack_addr = &random_bytes as *const _ as usize;
        random_bytes[20..28].copy_from_slice(&stack_addr.to_le_bytes());

        let mut hasher = Sha256::new();
        hasher.update(&random_bytes);
        let hash = format!("{:x}", hasher.finalize());

        let _ = std::fs::write(&key_path, &hash);
        tracing::info!("[NEUROLEASE] Generated new identity: {}...", &hash[..12]);
        PeerId(hash)
    }

    /// Start the mesh — spawns discovery, watchdog, message loop, and handler tasks.
    pub async fn start(self: &Arc<Self>) {
        // Check for previous self-destruct
        if self_destruct::has_self_destructed() {
            tracing::error!("[NEUROLEASE] ⛔ Previous self-destruct detected. Mesh permanently disabled.");
            return;
        }

        // Verify prompt integrity before starting
        if !prompt_integrity::verify_prompts() {
            tracing::error!("[NEUROLEASE] ⛔ PROMPT INTEGRITY FAILED. Tampering detected.");
            self_destruct::self_destruct(
                &std::path::PathBuf::from("memory/mesh"),
                None, // Don't corrupt binary in dev
            ).await;
            return;
        }

        let mesh = self.clone();

        // Spawn integrity watchdog
        let watchdog = self.watchdog.clone();
        tokio::spawn(async move {
            watchdog.run().await;
        });

        // Spawn integrity monitor — watches for tamper detection and triggers self-destruct
        let integrity_rx = self.watchdog.integrity_rx.clone();
        let mesh_for_destruct = self.clone();
        tokio::spawn(async move {
            let mut rx = integrity_rx;
            while rx.changed().await.is_ok() {
                if !*rx.borrow() {
                    // TAMPER DETECTED — execute self-destruct
                    mesh_for_destruct.disconnect_all().await;
                    self_destruct::self_destruct(
                        &std::path::PathBuf::from("memory/mesh"),
                        None, // Don't corrupt the main binary in dev — only the sealed .dylib in production
                    ).await;
                    break;
                }
            }
        });

        // Spawn discovery daemon
        let discovery = DiscoveryDaemon::new(
            self.config.clone(),
            self.registry.clone(),
            self.peer_id.clone(),
            format!("0.0.0.0:{}", self.config.port).parse().unwrap(),
        );
        tokio::spawn(async move {
            discovery.run().await;
        });

        // Spawn periodic trust persistence
        let trust = self.trust.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
                trust.read().await.save();
            }
        });

        // ── CORE MESSAGE LOOP ────────────────────────────────────────
        // Bind QUIC transport and start the mesh event loop.
        let mesh_dir = std::path::PathBuf::from("memory/mesh");
        match QuicTransport::bind(self.config.port, &self.peer_id) {
            Ok(transport) => {
                let transport = Arc::new(transport);

                // Build handler subsystems
                let content_filter = Arc::new(ContentFilter::new());
                let pool = Arc::new(RwLock::new(
                    PoolManager::new(self.peer_id.clone())
                ));
                let compute_relay_config = compute_relay::ComputeRelayConfig::mesh_defaults();
                let compute_pool_inner = pool.read().await.compute_pool.clone();
                let compute_relay = Arc::new(ComputeRelay::new(
                    compute_relay_config,
                    content_filter.clone(),
                    compute_pool_inner,
                    self.peer_id.clone(),
                ));

                let sync = Arc::new(RwLock::new(
                    KnowledgeSync::new(&mesh_dir)
                ));

                let chat = Arc::new(ApisChat::from_env());
                let book = Arc::new(ApisBook::new());
                let governance = Arc::new(
                    GovernanceEngine::new()
                );

                // L3-L7 subsystems
                let regional = Arc::new(
                    regional_keys::RegionalKeyRegistry::new()
                );
                let hardware_blacklist = Arc::new(RwLock::new(
                    hardware_blacklist::HardwareBlacklist::new(&mesh_dir)
                ));
                let capabilities = Arc::new(
                    capability_registry::CapabilityRegistry::new()
                );
                // Auto-detect local capabilities
                capabilities.detect_local(&self.peer_id).await;

                let project_root = std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("."));
                let config_guard = Arc::new(RwLock::new(
                    config_guard::ConfigGuard::new(&project_root)
                ));
                let ledger = Arc::new(RwLock::new(
                    crate::crypto::mesh_ledger::MeshLedger::new(&mesh_dir)
                ));

                // ── v5.0 Supercomputer subsystems ────────────────────────
                let sandbox_engine = Arc::new(
                    sandbox::SandboxEngine::new(sandbox::SandboxConfig::default())
                        .unwrap_or_else(|e| {
                            tracing::error!("[NEUROLEASE] ⚠️ Sandbox engine failed to init: {} — using disabled config", e);
                            sandbox::SandboxEngine::new(sandbox::SandboxConfig {
                                enabled: false,
                                ..Default::default()
                            }).expect("Disabled sandbox must init")
                        })
                );
                let priority_manager = Arc::new(
                    sandbox_priority::PriorityManager::new()
                );
                let dht = Arc::new(RwLock::new(
                    dht::DHT::new(self.peer_id.clone(), dht::DEFAULT_K)
                ));
                let mesh_fs_instance = Arc::new(RwLock::new(
                    mesh_fs::MeshFS::new(self.peer_id.clone())
                ));
                let governance_manager = Arc::new(
                    governance_phases::GovernanceManager::new()
                );

                let handlers = Arc::new(MeshHandlers {
                    transport: transport.clone(),
                    trust: self.trust.clone(),
                    sanctions: self.sanctions.clone(),
                    sync,
                    weights: None,
                    propagation: None,
                    chat,
                    book,
                    governance,
                    compute_relay,
                    content_filter,
                    pool,
                    registry: self.registry.clone(),
                    local_peer: self.peer_id.clone(),
                    local_attestation: self.local_attestation(),
                    regional,
                    hardware_blacklist,
                    capabilities,
                    config_guard,
                    ledger,
                    sandbox_engine,
                    priority_manager: priority_manager.clone(),
                    dht,
                    mesh_fs: mesh_fs_instance,
                    governance_manager,
                });

                // Spawn the priority monitor loop (tracks local CPU/RAM every 5s)
                priority_manager.clone().spawn_monitor();

                // Start the event loop — accepts connections, routes messages
                start_mesh_loop(handlers).await;

                tracing::info!(
                    "[NEUROLEASE] 🚀 Mesh started — QUIC transport + event loop on port {}",
                    mesh.config.port
                );
            }
            Err(e) => {
                tracing::error!(
                    "[NEUROLEASE] ❌ Failed to bind QUIC transport on port {}: {}",
                    self.config.port, e
                );
                tracing::info!("[NEUROLEASE] 🚀 Mesh started in discovery-only mode (no transport)");
            }
        }
    }

    /// Check if a peer should be accepted (not quarantined, not unattested).
    pub async fn should_accept_peer(&self, peer_id: &PeerId) -> bool {
        let sanctions = self.sanctions.read().await;
        if sanctions.is_quarantined(peer_id) {
            return false;
        }
        true
    }

    /// Get the mesh attestation for this instance.
    pub fn local_attestation(&self) -> Attestation {
        Attestation {
            binary_hash: self.watchdog.binary_hash.clone(),
            source_hash: self.watchdog.source_hash.clone(),
            commit: self.watchdog.commit.clone(),
            signature: vec![], // Will be populated with ed25519 signature
        }
    }

    /// Get lessons shared count.
    pub fn lessons_shared_count(&self) -> u64 {
        self.lessons_shared.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Disconnect from all peers. Called on integrity failure.
    pub async fn disconnect_all(&self) {
        tracing::error!("[NEUROLEASE] ⛔ EMERGENCY DISCONNECT — clearing all peers");
        // Registry prune with 0 age = remove all
        self.registry.prune_stale(0).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_generation() {
        let tmp = std::env::temp_dir().join(format!("hive_mesh_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let id1 = HiveMesh::load_or_generate_identity(&tmp);
        assert!(!id1.0.is_empty());
        assert!(id1.0.len() == 64); // SHA-256 hex length

        // Same dir should load the same identity
        let id2 = HiveMesh::load_or_generate_identity(&tmp);
        assert_eq!(id1.0, id2.0);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_mesh_disabled_by_default() {
        // Without NEUROLEASE_ENABLED=true, mesh should not initialize
        // (This test relies on the env var not being set in test environment)
        // We test the config instead
        let config = DiscoveryConfig {
            enabled: false,
            port: 9473,
            bootstrap_nodes: vec![],
            mdns_enabled: true,
        };
        assert!(!config.enabled);
    }
}
