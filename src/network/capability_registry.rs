/// Capability Registry — Peer hardware/software capability tracking.
///
/// Every peer advertises its capabilities (OS, arch, models, VRAM, RAM).
/// The registry enables intelligent matching:
/// - Code patches only sent to compatible OS/arch peers
/// - LoRA adapters only offered to peers with the matching base model
/// - Compute requests routed to peers with sufficient VRAM
///
/// Capabilities are auto-detected on boot and broadcast via Ping/Pong.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::network::messages::PeerId;

/// Describes a model available on a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,         // e.g. "qwen3.5:32b"
    pub family: String,       // e.g. "qwen"
    pub parameter_size: String, // e.g. "32B"
    pub quantization: String, // e.g. "Q4_K_M"
    pub size_bytes: u64,
}

/// Full capabilities of a mesh peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerCapabilities {
    pub peer_id: PeerId,
    pub os: String,               // "macos", "linux", "windows"
    pub arch: String,             // "aarch64", "x86_64"
    pub cpu_model: String,        // "Apple M3 Ultra"
    pub cpu_cores: usize,
    pub ram_gb: f64,
    pub vram_gb: f64,             // GPU VRAM (0.0 if no discrete GPU)
    pub models: Vec<ModelInfo>,   // Available Ollama models
    pub disk_free_gb: f64,
    pub last_updated: String,
}

impl PeerCapabilities {
    /// Auto-detect local capabilities.
    pub fn detect_local(peer_id: &PeerId) -> Self {
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_all();
        sys.refresh_memory();

        let cpu_model = sys.cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let ram_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);

        // VRAM detection: on Apple Silicon, unified memory is shared
        // On discrete GPUs, this would query nvidia-smi/rocm-smi
        let vram_gb = if cfg!(target_os = "macos") && std::env::consts::ARCH == "aarch64" {
            ram_gb // Unified memory — all RAM is available to GPU
        } else {
            0.0 // Conservative — no way to detect without root
        };

        // Detect available disk space
        let disk_free_gb = sysinfo::Disks::new_with_refreshed_list()
            .list()
            .first()
            .map(|d| d.available_space() as f64 / (1024.0 * 1024.0 * 1024.0))
            .unwrap_or(0.0);

        Self {
            peer_id: peer_id.clone(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            cpu_model,
            cpu_cores: sys.cpus().len(),
            ram_gb,
            vram_gb,
            models: Vec::new(), // Populated by detect_models()
            disk_free_gb,
            last_updated: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Detect available Ollama models (async, calls `ollama list`).
    pub async fn detect_models(&mut self) {
        let ollama_url = std::env::var("HIVE_OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());

        match reqwest::get(format!("{}/api/tags", ollama_url)).await {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(models) = json.get("models").and_then(|m| m.as_array()) {
                        self.models = models.iter()
                            .filter_map(|m| {
                                let name = m.get("name")?.as_str()?.to_string();
                                let details = m.get("details")?;
                                Some(ModelInfo {
                                    name: name.clone(),
                                    family: details.get("family")
                                        .and_then(|f| f.as_str())
                                        .unwrap_or("unknown")
                                        .to_string(),
                                    parameter_size: details.get("parameter_size")
                                        .and_then(|p| p.as_str())
                                        .unwrap_or("unknown")
                                        .to_string(),
                                    quantization: details.get("quantization_level")
                                        .and_then(|q| q.as_str())
                                        .unwrap_or("unknown")
                                        .to_string(),
                                    size_bytes: m.get("size")
                                        .and_then(|s| s.as_u64())
                                        .unwrap_or(0),
                                })
                            })
                            .collect();
                        tracing::info!(
                            "[CAPABILITIES] 🤖 Detected {} local models",
                            self.models.len()
                        );
                    }
                }
            }
            Err(_) => {
                tracing::debug!("[CAPABILITIES] Ollama not available for model detection");
            }
        }
    }

    /// Check if this peer's OS/arch matches a target.
    pub fn matches_target(&self, target_os: Option<&str>, target_arch: Option<&str>) -> bool {
        if let Some(os) = target_os {
            if self.os != os { return false; }
        }
        if let Some(arch) = target_arch {
            if self.arch != arch { return false; }
        }
        true
    }

    /// Check if this peer has a compatible model for a LoRA adapter.
    pub fn has_compatible_model(&self, base_model_family: &str, min_vram_gb: f64) -> bool {
        if self.vram_gb < min_vram_gb {
            return false;
        }
        self.models.iter().any(|m| {
            m.family.eq_ignore_ascii_case(base_model_family)
        })
    }
}

/// The Capability Registry — tracks all peer capabilities.
pub struct CapabilityRegistry {
    peers: Arc<RwLock<HashMap<String, PeerCapabilities>>>,
    local: Arc<RwLock<Option<PeerCapabilities>>>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        tracing::info!("[CAPABILITIES] 🔍 Capability registry initialised");
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            local: Arc::new(RwLock::new(None)),
        }
    }

    /// Auto-detect and store local capabilities.
    pub async fn detect_local(&self, peer_id: &PeerId) {
        let mut caps = PeerCapabilities::detect_local(peer_id);
        caps.detect_models().await;
        tracing::info!(
            "[CAPABILITIES] 🖥️ Local: {} {} | {} cores | {:.1}GB RAM | {:.1}GB VRAM | {} models",
            caps.os, caps.arch, caps.cpu_cores, caps.ram_gb, caps.vram_gb, caps.models.len()
        );
        *self.local.write().await = Some(caps);
    }

    /// Get local capabilities.
    pub async fn local_capabilities(&self) -> Option<PeerCapabilities> {
        self.local.read().await.clone()
    }

    /// Register or update a remote peer's capabilities.
    pub async fn update_peer(&self, caps: PeerCapabilities) {
        self.peers.write().await
            .insert(caps.peer_id.0.clone(), caps);
    }

    /// Get a peer's capabilities.
    pub async fn get_peer(&self, peer_id: &PeerId) -> Option<PeerCapabilities> {
        self.peers.read().await.get(&peer_id.0).cloned()
    }

    /// Find peers matching OS/arch criteria (for code patch targeting).
    pub async fn find_compatible_peers(
        &self,
        target_os: Option<&str>,
        target_arch: Option<&str>,
    ) -> Vec<PeerId> {
        self.peers.read().await
            .values()
            .filter(|c| c.matches_target(target_os, target_arch))
            .map(|c| c.peer_id.clone())
            .collect()
    }

    /// Find peers with a compatible model (for LoRA distribution).
    pub async fn find_model_compatible(
        &self,
        base_family: &str,
        min_vram_gb: f64,
    ) -> Vec<PeerId> {
        self.peers.read().await
            .values()
            .filter(|c| c.has_compatible_model(base_family, min_vram_gb))
            .map(|c| c.peer_id.clone())
            .collect()
    }

    /// Find peers with available compute capacity.
    pub async fn find_compute_capable(&self, min_ram_gb: f64) -> Vec<PeerId> {
        self.peers.read().await
            .values()
            .filter(|c| c.ram_gb >= min_ram_gb && !c.models.is_empty())
            .map(|c| c.peer_id.clone())
            .collect()
    }

    /// Get all peer capabilities.
    pub async fn all_peers(&self) -> Vec<PeerCapabilities> {
        self.peers.read().await.values().cloned().collect()
    }

    /// Get registry stats.
    pub async fn stats(&self) -> serde_json::Value {
        let peers = self.peers.read().await;
        let mut os_counts: HashMap<String, usize> = HashMap::new();
        let mut arch_counts: HashMap<String, usize> = HashMap::new();
        let mut total_ram = 0.0f64;
        let mut total_vram = 0.0f64;

        for cap in peers.values() {
            *os_counts.entry(cap.os.clone()).or_insert(0) += 1;
            *arch_counts.entry(cap.arch.clone()).or_insert(0) += 1;
            total_ram += cap.ram_gb;
            total_vram += cap.vram_gb;
        }

        serde_json::json!({
            "total_peers": peers.len(),
            "os_distribution": os_counts,
            "arch_distribution": arch_counts,
            "total_ram_gb": total_ram,
            "total_vram_gb": total_vram,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_caps(id: &str, os: &str, arch: &str, ram: f64, models: Vec<&str>) -> PeerCapabilities {
        PeerCapabilities {
            peer_id: PeerId(id.to_string()),
            os: os.to_string(),
            arch: arch.to_string(),
            cpu_model: "Test CPU".to_string(),
            cpu_cores: 8,
            ram_gb: ram,
            vram_gb: ram, // Simplified
            models: models.iter().map(|m| ModelInfo {
                name: m.to_string(),
                family: m.split(':').next().unwrap_or("unknown").to_string(),
                parameter_size: "7B".to_string(),
                quantization: "Q4_K_M".to_string(),
                size_bytes: 4_000_000_000,
            }).collect(),
            disk_free_gb: 100.0,
            last_updated: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[tokio::test]
    async fn test_capability_matching() {
        let registry = CapabilityRegistry::new();

        registry.update_peer(test_caps("linux_x86", "linux", "x86_64", 64.0, vec!["qwen3.5:32b"])).await;
        registry.update_peer(test_caps("mac_arm", "macos", "aarch64", 512.0, vec!["qwen3.5:32b"])).await;
        registry.update_peer(test_caps("linux_arm", "linux", "aarch64", 32.0, vec!["llama3:8b"])).await;

        // Find Linux peers
        let linux = registry.find_compatible_peers(Some("linux"), None).await;
        assert_eq!(linux.len(), 2);

        // Find aarch64 peers
        let arm = registry.find_compatible_peers(None, Some("aarch64")).await;
        assert_eq!(arm.len(), 2);

        // Find Linux + x86_64
        let linux_x86 = registry.find_compatible_peers(Some("linux"), Some("x86_64")).await;
        assert_eq!(linux_x86.len(), 1);
    }

    #[tokio::test]
    async fn test_model_compatibility() {
        let registry = CapabilityRegistry::new();

        registry.update_peer(test_caps("peer1", "linux", "x86_64", 64.0, vec!["qwen3.5:32b"])).await;
        registry.update_peer(test_caps("peer2", "macos", "aarch64", 512.0, vec!["qwen3.5:32b", "llama3:8b"])).await;
        registry.update_peer(test_caps("peer3", "linux", "x86_64", 8.0, vec!["llama3:8b"])).await;

        // Find qwen-compatible with >= 32GB VRAM
        let qwen = registry.find_model_compatible("qwen3.5", 32.0).await;
        assert_eq!(qwen.len(), 2);

        // Find llama-compatible with >= 500GB VRAM
        let high_vram = registry.find_model_compatible("llama3", 500.0).await;
        assert_eq!(high_vram.len(), 1); // Only the 512GB Mac
    }

    #[tokio::test]
    async fn test_local_detection() {
        let registry = CapabilityRegistry::new();
        let peer = PeerId("local_test".to_string());
        registry.detect_local(&peer).await;

        let local = registry.local_capabilities().await;
        assert!(local.is_some());
        let caps = local.unwrap();
        assert!(!caps.os.is_empty());
        assert!(!caps.arch.is_empty());
        assert!(caps.ram_gb > 0.0);
    }
}
