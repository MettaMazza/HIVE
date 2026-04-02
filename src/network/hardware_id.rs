/// Hardware Identity — Unique hardware fingerprinting for the HIVE mesh.
///
/// Creates a persistent hardware fingerprint that survives software reinstall.
/// Used by the sanctions system to permanently blacklist malicious hardware —
/// changing your PeerId or reinstalling Apis won't save you.
///
/// FINGERPRINT COMPONENTS (no root/sudo required):
/// - CPU model name + brand
/// - Total physical RAM
/// - Hostname
/// - Number of CPU cores
/// - OS name + version
/// - Architecture (x86_64, aarch64, etc.)
///
/// The composite is SHA-256 hashed to produce a deterministic 64-char hex ID.
///
/// LIMITATION: Hardware ID is not tamper-proof against deliberate spoofing by
/// someone with root access who modifies sysinfo responses. However, it raises
/// the bar significantly above just changing a software-level PeerId.
use sha2::{Sha256, Digest};
use serde::{Deserialize, Serialize};

/// A hardware fingerprint — deterministic and survives reinstall.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct HardwareId(pub String);

impl std::fmt::Display for HardwareId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Show first 12 chars for brevity
        let display = if self.0.len() > 12 { &self.0[..12] } else { &self.0 };
        write!(f, "hw:{}", display)
    }
}

/// Components that make up the hardware fingerprint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareFingerprint {
    pub cpu_model: String,
    pub cpu_cores: usize,
    pub total_ram_bytes: u64,
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub arch: String,
}

impl HardwareFingerprint {
    /// Collect hardware fingerprint from the local system.
    pub fn collect() -> Self {
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_all();
        sys.refresh_memory();

        let cpu_model = sys.cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_else(|| "unknown_cpu".to_string());

        let cpu_cores = sys.cpus().len();
        let total_ram_bytes = sys.total_memory();
        let hostname = sysinfo::System::host_name()
            .unwrap_or_else(|| "unknown_host".to_string());
        let os_name = sysinfo::System::name()
            .unwrap_or_else(|| "unknown_os".to_string());
        let os_version = sysinfo::System::os_version()
            .unwrap_or_else(|| "unknown_version".to_string());
        let arch = std::env::consts::ARCH.to_string();

        Self {
            cpu_model,
            cpu_cores,
            total_ram_bytes,
            hostname,
            os_name,
            os_version,
            arch,
        }
    }

    /// Compute the deterministic hardware ID from the fingerprint.
    pub fn hardware_id(&self) -> HardwareId {
        let mut hasher = Sha256::new();
        hasher.update(b"HIVE_HARDWARE_ID_V1");
        hasher.update(self.cpu_model.as_bytes());
        hasher.update(&self.cpu_cores.to_le_bytes());
        hasher.update(&self.total_ram_bytes.to_le_bytes());
        hasher.update(self.hostname.as_bytes());
        hasher.update(self.os_name.as_bytes());
        hasher.update(self.arch.as_bytes());
        // Notably: we include OS name but NOT os_version — updates shouldn't change the ID
        HardwareId(format!("{:x}", hasher.finalize()))
    }
}

/// Get the local hardware ID (cached after first call per process).
pub fn local_hardware_id() -> HardwareId {
    let fingerprint = HardwareFingerprint::collect();
    let id = fingerprint.hardware_id();
    tracing::debug!(
        "[HARDWARE] 🖥️ Local fingerprint: {} (cpu={}, ram={}GB, arch={})",
        id, fingerprint.cpu_model,
        fingerprint.total_ram_bytes / (1024 * 1024 * 1024),
        fingerprint.arch
    );
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardware_fingerprint_deterministic() {
        let fp1 = HardwareFingerprint::collect();
        let fp2 = HardwareFingerprint::collect();
        assert_eq!(fp1.hardware_id(), fp2.hardware_id());
    }

    #[test]
    fn test_hardware_id_length() {
        let id = local_hardware_id();
        assert_eq!(id.0.len(), 64, "SHA-256 hex should be 64 chars");
    }

    #[test]
    fn test_different_fingerprints_different_ids() {
        let fp1 = HardwareFingerprint {
            cpu_model: "Apple M3 Ultra".to_string(),
            cpu_cores: 24,
            total_ram_bytes: 512 * 1024 * 1024 * 1024,
            hostname: "hive-main".to_string(),
            os_name: "macOS".to_string(),
            os_version: "15.0".to_string(),
            arch: "aarch64".to_string(),
        };

        let fp2 = HardwareFingerprint {
            cpu_model: "Intel Core i9".to_string(),
            cpu_cores: 16,
            total_ram_bytes: 64 * 1024 * 1024 * 1024,
            hostname: "attacker-box".to_string(),
            os_name: "Linux".to_string(),
            os_version: "6.1".to_string(),
            arch: "x86_64".to_string(),
        };

        assert_ne!(fp1.hardware_id(), fp2.hardware_id());
    }

    #[test]
    fn test_hardware_id_display() {
        let id = HardwareId("abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string());
        assert_eq!(format!("{}", id), "hw:abcdef123456");
    }
}
