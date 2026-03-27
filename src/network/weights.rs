/// Weight Exchange — LoRA adapter sharing across the mesh.
///
/// When one Apis trains a LoRA adapter from golden examples,
/// she broadcasts the availability to all peers. Interested peers
/// request the adapter bytes and apply via `ollama create`.
///
/// Open mesh: all attested peers can share and receive weight transfers.
/// Safety enforced by binary attestation + integrity watchdog, not trust tiers.
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::teacher::Teacher;
use crate::network::messages::{PeerId, MeshMessage};
use crate::network::trust::TrustStore;

/// Manages weight exchange between mesh peers.
pub struct WeightExchange {
    teacher: Arc<Teacher>,
    trust: Arc<RwLock<TrustStore>>,
    adapter_dir: PathBuf,
}

impl WeightExchange {
    pub fn new(teacher: Arc<Teacher>, trust: Arc<RwLock<TrustStore>>, mesh_dir: &std::path::Path) -> Self {
        let adapter_dir = mesh_dir.join("adapters");
        let _ = std::fs::create_dir_all(&adapter_dir);
        Self { teacher, trust, adapter_dir }
    }

    /// Create a LoRAAnnounce message for broadcasting after local training.
    pub fn create_announce(&self, local_peer_id: &PeerId) -> Option<MeshMessage> {
        let manifest = self.teacher.load_manifest();

        // Only announce if we have trained at least once
        if manifest.history.is_empty() {
            return None;
        }

        Some(MeshMessage::LoRAAnnounce {
            version: manifest.current.clone(),
            manifest_json: serde_json::to_string(&manifest).unwrap_or_default(),
            origin: local_peer_id.clone(),
        })
    }

    /// Handle incoming LoRA announcement. Returns true if we should request the adapter.
    pub async fn should_request(&self, announce_version: &str, peer_id: &PeerId) -> bool {
        // Only accept from attested peers
        let trust = self.trust.read().await;
        if !trust.can_share_weights(peer_id) {
            tracing::warn!("[WEIGHTS] Rejected LoRA announce from {} (not attested)", peer_id);
            return false;
        }

        // Compare versions — request if newer
        let local = self.teacher.load_manifest();
        if announce_version != local.current {
            tracing::info!("[WEIGHTS] 📡 New LoRA version available: {} (local: {})", announce_version, local.current);
            return true;
        }

        false
    }

    /// Stage received adapter bytes for application.
    pub async fn stage_adapter(&self, version: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
        let path = self.adapter_dir.join(format!("adapter_{}.gguf", version));
        tokio::fs::write(&path, bytes).await?;
        tracing::info!("[WEIGHTS] 📥 Adapter staged: {} ({} bytes)", version, bytes.len());
        Ok(path)
    }

    /// Apply a staged adapter via `ollama create`.
    pub async fn apply_adapter(&self, version: &str, adapter_path: &std::path::Path) -> Result<(), String> {
        let modelfile = format!(
            "FROM {}\nADAPTER {}",
            self.teacher.load_manifest().base,
            adapter_path.display()
        );

        let modelfile_path = self.adapter_dir.join("Modelfile.tmp");
        std::fs::write(&modelfile_path, &modelfile)
            .map_err(|e| format!("Failed to write Modelfile: {}", e))?;

        let output = tokio::process::Command::new("ollama")
            .args(["create", &format!("apis-mesh-{}", version), "-f", &modelfile_path.to_string_lossy()])
            .output()
            .await
            .map_err(|e| format!("Failed to run ollama create: {}", e))?;

        if output.status.success() {
            tracing::info!("[WEIGHTS] ✅ Applied LoRA adapter: apis-mesh-{}", version);
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("ollama create failed: {}", stderr))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_announce_without_training() {
        let tmp = std::env::temp_dir().join(format!("hive_weights_test_{}", std::process::id()));
        let teacher = Arc::new(Teacher::new(Some(tmp.clone())));
        let trust = Arc::new(RwLock::new(TrustStore::new(&tmp)));

        let exchange = WeightExchange::new(teacher, trust, &tmp);
        let peer_id = PeerId("test_peer".into());

        // No training history = no announce
        assert!(exchange.create_announce(&peer_id).is_none());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[tokio::test]
    async fn test_stage_adapter() {
        let tmp = std::env::temp_dir().join(format!("hive_weights_stage_{}", std::process::id()));
        let teacher = Arc::new(Teacher::new(Some(tmp.clone())));
        let trust = Arc::new(RwLock::new(TrustStore::new(&tmp)));

        let exchange = WeightExchange::new(teacher, trust, &tmp);
        let path = exchange.stage_adapter("v1", b"fake_adapter_bytes").await.unwrap();
        assert!(path.exists());

        std::fs::remove_dir_all(&tmp).ok();
    }
}
