/// Code Propagation — Autonomous code patch sharing across the mesh.
///
/// When one Apis self-recompiles with a fix, she broadcasts the patch.
/// Other instances autonomously apply, test, build, and restart.
///
/// This is fully autonomous — no human approval gate.
/// Security is enforced by inherent safety mechanisms:
/// 1. Binary attestation (only attested peers accepted)
/// 2. `cargo test` gate (all tests must pass before apply)
/// 3. Rollback on failure (git stash before apply, pop on failure)
/// 4. Integrity watchdog (detects binary changes post-apply)
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::network::messages::{PeerId, MeshMessage};
use crate::network::trust::TrustStore;

/// Manages code patch propagation across the mesh.
pub struct CodePropagation {
    trust: Arc<RwLock<TrustStore>>,
    patches_dir: PathBuf,
    project_root: PathBuf,
}

impl CodePropagation {
    pub fn new(trust: Arc<RwLock<TrustStore>>, mesh_dir: &std::path::Path) -> Self {
        let patches_dir = mesh_dir.join("patches");
        let _ = std::fs::create_dir_all(&patches_dir);
        Self {
            trust,
            patches_dir,
            project_root: PathBuf::from("."),
        }
    }

    /// Generate a code patch from the latest local commit.
    pub async fn generate_patch(&self, local_peer_id: &PeerId) -> Option<MeshMessage> {
        // Get the diff from the last commit
        let diff_output = tokio::process::Command::new("git")
            .args(["diff", "HEAD~1"])
            .current_dir(&self.project_root)
            .output()
            .await
            .ok()?;

        if !diff_output.status.success() || diff_output.stdout.is_empty() {
            return None;
        }

        let diff = String::from_utf8_lossy(&diff_output.stdout).to_string();

        // Get commit hash
        let hash_output = tokio::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(&self.project_root)
            .output()
            .await
            .ok()?;

        let commit_hash = String::from_utf8_lossy(&hash_output.stdout).trim().to_string();

        // Verify tests pass
        let test_output = tokio::process::Command::new("cargo")
            .args(["test"])
            .current_dir(&self.project_root)
            .output()
            .await
            .ok()?;

        let test_passed = test_output.status.success();

        if !test_passed {
            tracing::warn!("[PROPAGATION] Tests failed — not broadcasting patch {}", commit_hash);
            return None;
        }

        tracing::info!("[PROPAGATION] 📤 Broadcasting code patch: {} ({} bytes)", commit_hash, diff.len());

        Some(MeshMessage::CodePatch {
            diff,
            commit_hash,
            test_passed,
            origin: local_peer_id.clone(),
        })
    }

    /// Handle incoming code patch. Returns Ok(true) if successfully applied.
    pub async fn apply_patch(
        &self,
        diff: &str,
        commit_hash: &str,
        peer_id: &PeerId,
    ) -> Result<bool, String> {
        // 1. Attestation check — only accept from attested peers
        let trust = self.trust.read().await;
        if !trust.can_share_code(peer_id) {
            return Err(format!("Peer {} not attested for code patches", peer_id));
        }
        drop(trust);

        // 2. Stage the patch
        let patch_path = self.patches_dir.join(format!("{}.patch", commit_hash));
        std::fs::write(&patch_path, diff)
            .map_err(|e| format!("Failed to write patch: {}", e))?;

        tracing::info!("[PROPAGATION] 📥 Applying patch {} from {}", commit_hash, peer_id);

        // 3. Stash local changes (rollback point)
        let _ = tokio::process::Command::new("git")
            .args(["stash"])
            .current_dir(&self.project_root)
            .output()
            .await;

        // 4. Apply the diff
        let apply = tokio::process::Command::new("git")
            .args(["apply", &patch_path.to_string_lossy()])
            .current_dir(&self.project_root)
            .output()
            .await
            .map_err(|e| format!("git apply failed: {}", e))?;

        if !apply.status.success() {
            // Rollback
            let _ = tokio::process::Command::new("git")
                .args(["stash", "pop"])
                .current_dir(&self.project_root)
                .output()
                .await;
            return Err(format!("git apply failed: {}", String::from_utf8_lossy(&apply.stderr)));
        }

        // 5. Run cargo test
        let test_output = tokio::process::Command::new("cargo")
            .args(["test"])
            .current_dir(&self.project_root)
            .output()
            .await
            .map_err(|e| format!("cargo test failed: {}", e))?;

        if !test_output.status.success() {
            tracing::error!("[PROPAGATION] ❌ Tests failed after applying patch {} — rolling back", commit_hash);
            // Rollback: reset and restore stash
            let _ = tokio::process::Command::new("git")
                .args(["checkout", "."])
                .current_dir(&self.project_root)
                .output()
                .await;
            let _ = tokio::process::Command::new("git")
                .args(["stash", "pop"])
                .current_dir(&self.project_root)
                .output()
                .await;
            return Err("Tests failed after patch application — rolled back".to_string());
        }

        // 6. Build
        let build = tokio::process::Command::new("cargo")
            .args(["build", "--release"])
            .current_dir(&self.project_root)
            .output()
            .await
            .map_err(|e| format!("cargo build failed: {}", e))?;

        if !build.status.success() {
            tracing::error!("[PROPAGATION] ❌ Build failed after applying patch {} — rolling back", commit_hash);
            let _ = tokio::process::Command::new("git")
                .args(["checkout", "."])
                .current_dir(&self.project_root)
                .output()
                .await;
            let _ = tokio::process::Command::new("git")
                .args(["stash", "pop"])
                .current_dir(&self.project_root)
                .output()
                .await;
            return Err("Build failed after patch application — rolled back".to_string());
        }

        tracing::info!("[PROPAGATION] ✅ Patch {} applied, tested, and built successfully", commit_hash);

        // 7. Commit the change
        let _ = tokio::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&self.project_root)
            .output()
            .await;
        let _ = tokio::process::Command::new("git")
            .args(["commit", "-m", &format!("mesh: applied patch {} from {}", commit_hash, peer_id)])
            .current_dir(&self.project_root)
            .output()
            .await;

        // Clean up
        let _ = std::fs::remove_file(&patch_path);

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_trust_gate() {
        let tmp = std::env::temp_dir().join(format!("hive_prop_test_{}", std::process::id()));
        let trust = Arc::new(RwLock::new(TrustStore::new(&tmp)));

        let prop = CodePropagation::new(trust, &tmp);
        let peer = PeerId("untrusted_peer".into());

        // Untrusted (unattested) peer should be rejected
        let result = prop.apply_patch("some diff", "abc123", &peer).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not attested"));

        std::fs::remove_dir_all(&tmp).ok();
    }
}
