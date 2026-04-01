/// Config Guard — Runtime filesystem protection for the HIVE mesh.
///
/// Monitors protected paths (src/, prompts/, Cargo.toml) for unauthorized changes.
/// The ONLY user-editable path is `memory/identity.json`.
///
/// ENFORCEMENT (Option C — Both):
/// 1. On detection of unauthorized file change → IMMEDIATE mesh disconnect
/// 2. Start a 60-second countdown
/// 3. If the change is NOT reverted within 60s → SELF-DESTRUCT + HARDWARE BLACKLIST
/// 4. If reverted within 60s → reconnect to mesh (warning logged)
///
/// EXEMPTION: The Creator Key machine (where .hive/creator.key exists) is exempt.
/// This allows the creator to develop, test, and deploy code changes normally.
///
/// IMPLEMENTATION: Uses SHA-256 hash verification of protected directories.
/// Hashes are computed at boot and re-verified every 30 seconds.
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use sha2::{Sha256, Digest};

/// Protected paths — any write here on a non-creator machine triggers enforcement.
const PROTECTED_PATHS: &[&str] = &[
    "src/",
    "prompts/",
    "Cargo.toml",
    "Cargo.lock",
    ".env",
];

/// The ONLY user-editable file.
#[allow(dead_code)]
const USER_EDITABLE: &[&str] = &[
    "memory/identity.json",
    "memory/",
];

/// State of the config guard.
#[derive(Debug, Clone, PartialEq)]
pub enum GuardState {
    /// Normal operation — all hashes match.
    Clean,
    /// Tamper detected — mesh disconnected, countdown running.
    TamperDetected {
        changed_path: String,
        detected_at: std::time::Instant,
        deadline: std::time::Instant,
    },
    /// Self-destruct executed — terminal state.
    Destroyed,
}

impl std::fmt::Display for GuardState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Clean => write!(f, "CLEAN"),
            Self::TamperDetected { changed_path, deadline, .. } => {
                let remaining = deadline.duration_since(std::time::Instant::now());
                write!(f, "TAMPER DETECTED: {} ({}s to revert)", changed_path, remaining.as_secs())
            }
            Self::Destroyed => write!(f, "DESTROYED"),
        }
    }
}

/// The Config Guard — monitors protected files for unauthorized changes.
pub struct ConfigGuard {
    /// Baseline hashes computed at boot.
    baseline_hashes: HashMap<String, String>,
    /// Whether this is the creator's machine (exempt from enforcement).
    is_creator_machine: bool,
    /// Current guard state.
    state: GuardState,
    /// Root directory of the project.
    project_root: PathBuf,
    /// Countdown duration before self-destruct (seconds).
    destruct_countdown_secs: u64,
}

impl ConfigGuard {
    /// Initialize the config guard. Computes baseline hashes of all protected paths.
    pub fn new(project_root: &Path) -> Self {
        let is_creator = crate::network::creator_key::creator_key_exists();

        if is_creator {
            tracing::info!("[CONFIG GUARD] 🔑 Creator key detected — enforcement EXEMPTED");
        } else {
            tracing::info!("[CONFIG GUARD] 🛡️ Filesystem protection ACTIVE on {} protected paths", PROTECTED_PATHS.len());
        }

        let mut guard = Self {
            baseline_hashes: HashMap::new(),
            is_creator_machine: is_creator,
            state: GuardState::Clean,
            project_root: project_root.to_path_buf(),
            destruct_countdown_secs: 60,
        };

        guard.compute_baseline();
        guard
    }

    /// Compute SHA-256 hashes of all protected paths.
    fn compute_baseline(&mut self) {
        self.baseline_hashes.clear();

        for path_str in PROTECTED_PATHS {
            let path = self.project_root.join(path_str);
            if path.exists() {
                let hash = if path.is_dir() {
                    Self::hash_directory(&path)
                } else {
                    Self::hash_file(&path)
                };

                if let Some(h) = hash {
                    self.baseline_hashes.insert(path_str.to_string(), h);
                }
            }
        }

        tracing::info!(
            "[CONFIG GUARD] 📋 Baseline: {} paths hashed",
            self.baseline_hashes.len()
        );
    }

    /// Hash a single file.
    fn hash_file(path: &Path) -> Option<String> {
        let data = std::fs::read(path).ok()?;
        let mut hasher = Sha256::new();
        hasher.update(&data);
        Some(format!("{:x}", hasher.finalize()))
    }

    /// Hash a directory recursively (all files sorted by name).
    fn hash_directory(dir: &Path) -> Option<String> {
        let mut hasher = Sha256::new();
        let mut entries: Vec<PathBuf> = Vec::new();

        Self::collect_files(dir, &mut entries);
        entries.sort();

        for entry in &entries {
            if let Ok(data) = std::fs::read(entry) {
                // Include the relative path in the hash so file renames are detected
                let relative = entry.strip_prefix(dir).unwrap_or(entry);
                hasher.update(relative.to_string_lossy().as_bytes());
                hasher.update(&data);
            }
        }

        Some(format!("{:x}", hasher.finalize()))
    }

    /// Recursively collect all files in a directory.
    fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Skip target/, .git/, and other non-source dirs
                    let name = path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    if name == "target" || name == ".git" || name.starts_with('.') {
                        continue;
                    }
                    Self::collect_files(&path, files);
                } else {
                    files.push(path);
                }
            }
        }
    }

    /// Check all protected paths against their baseline hashes.
    /// Returns the first changed path, or None if all clean.
    pub fn verify(&self) -> Option<String> {
        if self.is_creator_machine {
            return None; // Creator exempt
        }

        for (path_str, baseline_hash) in &self.baseline_hashes {
            let path = self.project_root.join(path_str);
            let current_hash = if path.is_dir() {
                Self::hash_directory(&path)
            } else {
                Self::hash_file(&path)
            };

            match current_hash {
                Some(h) if h != *baseline_hash => {
                    tracing::error!(
                        "[CONFIG GUARD] ⛔ TAMPER DETECTED: {} (hash changed)",
                        path_str
                    );
                    return Some(path_str.clone());
                }
                None => {
                    tracing::error!(
                        "[CONFIG GUARD] ⛔ TAMPER DETECTED: {} (file deleted)",
                        path_str
                    );
                    return Some(path_str.clone());
                }
                _ => {} // Hash matches — clean
            }
        }

        None
    }

    /// Run the guard check. Called periodically (every 30s).
    ///
    /// Returns the updated state:
    /// - `Clean` → nothing to do
    /// - `TamperDetected` → caller should disconnect from mesh
    /// - `Destroyed` → caller should execute self-destruct
    pub fn check(&mut self) -> &GuardState {
        if self.is_creator_machine {
            return &self.state;
        }

        match &self.state {
            GuardState::Clean => {
                // Check for new tampering
                if let Some(changed_path) = self.verify() {
                    let now = std::time::Instant::now();
                    let deadline = now + std::time::Duration::from_secs(self.destruct_countdown_secs);

                    tracing::error!(
                        "╔═════════════════════════════════════════════════════════╗"
                    );
                    tracing::error!(
                        "║  ⛔ UNAUTHORIZED FILE MODIFICATION DETECTED             ║"
                    );
                    tracing::error!(
                        "║  Path: {:50}║", &changed_path[..changed_path.len().min(50)]
                    );
                    tracing::error!(
                        "║  MESH DISCONNECTED. You have {}s to revert.           ║",
                        self.destruct_countdown_secs
                    );
                    tracing::error!(
                        "║  Failure to revert → SELF-DESTRUCT + HARDWARE BAN      ║"
                    );
                    tracing::error!(
                        "╚═════════════════════════════════════════════════════════╝"
                    );

                    self.state = GuardState::TamperDetected {
                        changed_path,
                        detected_at: now,
                        deadline,
                    };
                }
            }
            GuardState::TamperDetected { changed_path, deadline, .. } => {
                // Check if the change was reverted
                if self.verify().is_none() {
                    tracing::info!(
                        "[CONFIG GUARD] ✅ Change reverted — reconnecting to mesh"
                    );
                    self.state = GuardState::Clean;
                    return &self.state;
                }

                // Check if the countdown has expired
                if std::time::Instant::now() >= *deadline {
                    tracing::error!(
                        "[CONFIG GUARD] 💀 Countdown expired — {} was NOT reverted",
                        changed_path
                    );
                    tracing::error!(
                        "[CONFIG GUARD] 💀 EXECUTING SELF-DESTRUCT + HARDWARE BLACKLIST"
                    );
                    self.state = GuardState::Destroyed;
                }
            }
            GuardState::Destroyed => {
                // Terminal state — nothing more to do
            }
        }

        &self.state
    }

    /// Get the current state.
    pub fn state(&self) -> &GuardState {
        &self.state
    }

    /// Whether this is the creator's machine.
    pub fn is_creator(&self) -> bool {
        self.is_creator_machine
    }

    /// Get seconds remaining before self-destruct (if in tamper state).
    pub fn seconds_remaining(&self) -> Option<u64> {
        match &self.state {
            GuardState::TamperDetected { deadline, .. } => {
                let now = std::time::Instant::now();
                if now < *deadline {
                    Some((*deadline - now).as_secs())
                } else {
                    Some(0)
                }
            }
            _ => None,
        }
    }

    /// Refresh the baseline (after a legitimate creator-key update).
    pub fn refresh_baseline(&mut self) {
        if !self.is_creator_machine {
            tracing::warn!("[CONFIG GUARD] ⚠️ Baseline refresh denied — not creator machine");
            return;
        }
        self.compute_baseline();
        tracing::info!("[CONFIG GUARD] 📋 Baseline refreshed by creator");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_file() {
        let tmp = std::env::temp_dir().join(format!("hive_guard_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let file = tmp.join("test.txt");
        std::fs::write(&file, "hello").unwrap();

        let hash1 = ConfigGuard::hash_file(&file).unwrap();
        assert_eq!(hash1.len(), 64); // SHA-256 hex

        // Same content = same hash
        let hash2 = ConfigGuard::hash_file(&file).unwrap();
        assert_eq!(hash1, hash2);

        // Different content = different hash
        std::fs::write(&file, "world").unwrap();
        let hash3 = ConfigGuard::hash_file(&file).unwrap();
        assert_ne!(hash1, hash3);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_hash_directory() {
        let tmp = std::env::temp_dir().join(format!("hive_guard_dir_{}", std::process::id()));
        let _ = std::fs::create_dir_all(tmp.join("sub"));
        std::fs::write(tmp.join("a.rs"), "fn main() {}").unwrap();
        std::fs::write(tmp.join("sub/b.rs"), "fn helper() {}").unwrap();

        let hash1 = ConfigGuard::hash_directory(&tmp).unwrap();
        assert_eq!(hash1.len(), 64);

        // Modify a file → hash changes
        std::fs::write(tmp.join("sub/b.rs"), "fn modified() {}").unwrap();
        let hash2 = ConfigGuard::hash_directory(&tmp).unwrap();
        assert_ne!(hash1, hash2);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_guard_states() {
        assert_eq!(format!("{}", GuardState::Clean), "CLEAN");
        assert_eq!(format!("{}", GuardState::Destroyed), "DESTROYED");
    }

    #[test]
    fn test_protected_paths_defined() {
        assert!(!PROTECTED_PATHS.is_empty());
        assert!(PROTECTED_PATHS.contains(&"src/"));
        assert!(PROTECTED_PATHS.contains(&"Cargo.toml"));
    }
}
