/// Binary Integrity Watchdog — ensures the HIVE binary is unmodified.
///
/// Computes SHA-256 of the running binary at boot and re-verifies every 60 seconds.
/// If the hash changes (hot-patching, injection), the mesh connection is severed.
/// Also computes a source tree hash for attestation to remote peers.
use sha2::{Sha256, Digest};
use std::path::Path;
use std::time::Duration;
use tokio::sync::watch;

/// Compute SHA-256 of a file. Returns hex string.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Compute SHA-256 of the entire src/ directory tree (sorted, deterministic).
pub fn sha256_source_tree(src_dir: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut paths: Vec<std::path::PathBuf> = Vec::new();

    fn collect_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
        if dir.is_dir() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    collect_files(&path, out)?;
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        Ok(())
    }

    collect_files(src_dir, &mut paths)?;
    paths.sort(); // Deterministic ordering

    for path in &paths {
        let bytes = std::fs::read(path)?;
        // Include the relative path in the hash so file renames are detected
        let rel = path.strip_prefix(src_dir).unwrap_or(path);
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update(&bytes);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Get the path to the currently running binary.
pub fn current_binary_path() -> std::io::Result<std::path::PathBuf> {
    std::env::current_exe()
}

/// Get the current git commit hash (short form).
pub fn git_commit_hash() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// The integrity watchdog. Runs continuously to detect binary tampering.
pub struct IntegrityWatchdog {
    pub binary_hash: String,
    pub source_hash: String,
    pub commit: String,
    check_interval: Duration,
    binary_path: std::path::PathBuf,
    src_dir: std::path::PathBuf,
    /// Sends `false` if integrity check fails (for mesh shutdown)
    integrity_tx: watch::Sender<bool>,
    pub integrity_rx: watch::Receiver<bool>,
}

impl IntegrityWatchdog {
    pub fn new(check_interval_secs: u64) -> std::io::Result<Self> {
        let binary_path = current_binary_path()?;
        let src_dir = std::path::PathBuf::from("src");

        let binary_hash = sha256_file(&binary_path)?;
        let source_hash = sha256_source_tree(&src_dir).unwrap_or_else(|_| "unknown".to_string());
        let commit = git_commit_hash();

        let (integrity_tx, integrity_rx) = watch::channel(true);

        tracing::info!(
            "[INTEGRITY] 🛡️ Boot hashes — binary: {}..., source: {}..., commit: {}",
            &binary_hash[..12], &source_hash[..12.min(source_hash.len())], commit
        );

        Ok(Self {
            binary_hash,
            source_hash,
            commit,
            check_interval: Duration::from_secs(check_interval_secs),
            binary_path,
            src_dir,
            integrity_tx,
            integrity_rx,
        })
    }

    /// Recompute binary hash and compare to boot value.
    pub fn verify_binary(&self) -> bool {
        match sha256_file(&self.binary_path) {
            Ok(current) => {
                if current != self.binary_hash {
                    tracing::error!(
                        "[INTEGRITY] ❌ Binary hash CHANGED! Boot: {}... Current: {}...",
                        &self.binary_hash[..12], &current[..12]
                    );
                    false
                } else {
                    true
                }
            }
            Err(e) => {
                tracing::error!("[INTEGRITY] ❌ Failed to read binary for verification: {}", e);
                false // Can't verify = assume compromised
            }
        }
    }

    /// Recompute source hash and compare to boot value.
    pub fn verify_source(&self) -> bool {
        match sha256_source_tree(&self.src_dir) {
            Ok(current) => {
                if current != self.source_hash {
                    tracing::warn!(
                        "[INTEGRITY] ⚠️ Source hash changed (self-recompile in progress?). Boot: {}... Current: {}...",
                        &self.source_hash[..12], &current[..12]
                    );
                    // Source changes are expected during self-recompile.
                    // Only the BINARY hash change triggers mesh disconnect.
                    true
                } else {
                    true
                }
            }
            Err(_) => true, // Source dir missing is OK (deployed without source)
        }
    }

    /// Run the watchdog loop. Sends `false` on the watch channel if integrity fails.
    pub async fn run(&self) {
        loop {
            tokio::time::sleep(self.check_interval).await;

            if !self.verify_binary() {
                tracing::error!("[INTEGRITY] 🚨 BINARY TAMPERING DETECTED — disconnecting from mesh!");
                let _ = self.integrity_tx.send(false);
                break;
            }

            tracing::trace!("[INTEGRITY] ✅ Binary integrity verified.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_sha256_file() {
        let tmp = std::env::temp_dir().join(format!("hive_integrity_test_{}", std::process::id()));
        let mut f = std::fs::File::create(&tmp).unwrap();
        f.write_all(b"hello world").unwrap();
        drop(f);

        let hash = sha256_file(&tmp).unwrap();
        // Known SHA-256 of "hello world"
        assert_eq!(hash, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_sha256_source_tree_deterministic() {
        let tmp = std::env::temp_dir().join(format!("hive_src_test_{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("sub")).unwrap();
        std::fs::write(tmp.join("a.rs"), "fn a() {}").unwrap();
        std::fs::write(tmp.join("sub/b.rs"), "fn b() {}").unwrap();

        let hash1 = sha256_source_tree(&tmp).unwrap();
        let hash2 = sha256_source_tree(&tmp).unwrap();
        assert_eq!(hash1, hash2, "Source tree hash should be deterministic");

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_git_commit_hash_format() {
        let commit = git_commit_hash();
        // Either a short hex hash or "unknown"
        assert!(
            commit == "unknown" || commit.len() >= 7,
            "Commit should be a short hash or 'unknown': {}", commit
        );
    }
}
