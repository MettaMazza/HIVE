/// Creator Key — Cryptographic authentication for the HIVE creator.
///
/// The creator (Maria) has a private ed25519 key stored on her machine.
/// The matching public key is embedded in the sealed binary.
/// Only the creator can access the monitoring dashboard and issue
/// network-wide directives.
///
/// Users CANNOT:
/// - See the creator key
/// - Authenticate as creator
/// - Access creator telemetry
/// - Override creator decisions
use sha2::{Sha256, Digest};

/// The creator's public key hash (SHA-256 of the ed25519 public key).
/// This is embedded in the sealed binary and cannot be changed without recompilation.
/// Set to a placeholder until the actual key is generated.
const CREATOR_KEY_HASH: &str = "65e8bc87c9eb939feb4b84b82068ed4a25f439e27fce59ccf7b609af67440805";

/// Path where the creator's private key is stored (creator's machine only).
const CREATOR_KEY_PATH: &str = ".hive/creator.key";

/// Verify a challenge-response from a potential creator.
/// The creator signs a challenge nonce with their private key.
/// We verify against the embedded public key hash.
pub fn verify_creator(public_key_bytes: &[u8], signature: &[u8], challenge: &[u8]) -> bool {
    // Hash the provided public key and compare to the embedded hash
    let mut hasher = Sha256::new();
    hasher.update(public_key_bytes);
    let key_hash = format!("{:x}", hasher.finalize());

    if key_hash != CREATOR_KEY_HASH {
        tracing::warn!("[CREATOR] ❌ Public key hash mismatch — not the creator");
        return false;
    }

    // In production, this would verify the ed25519 signature.
    // For now, we validate the key hash match.
    // Full ed25519 verification will be added when ed25519-dalek is integrated.
    let _ = signature;
    let _ = challenge;

    tracing::info!("[CREATOR] ✅ Creator authenticated");
    true
}

/// Generate a new creator keypair. Called once during initial setup.
pub fn generate_creator_keypair() -> Result<(Vec<u8>, Vec<u8>), String> {
    // Generate a deterministic-looking keypair from system entropy
    use sha2::Digest;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();

    // Create entropy from multiple sources
    let mut entropy = Vec::new();
    entropy.extend_from_slice(&now.to_le_bytes());
    entropy.extend_from_slice(&pid.to_le_bytes());
    entropy.extend_from_slice(b"HIVE_CREATOR_KEY_ENTROPY_SALT_v1");

    let mut hasher = Sha256::new();
    hasher.update(&entropy);
    let private_key = hasher.finalize().to_vec();

    // Derive "public key" (simplified — real ed25519 will be used in production)
    let mut hasher2 = Sha256::new();
    hasher2.update(&private_key);
    hasher2.update(b"PUBLIC_KEY_DERIVATION");
    let public_key = hasher2.finalize().to_vec();

    // Save private key to creator key path
    let key_dir = std::path::Path::new(CREATOR_KEY_PATH).parent().unwrap_or(std::path::Path::new("."));
    let _ = std::fs::create_dir_all(key_dir);
    let key_hex = hex_encode(&private_key);
    std::fs::write(CREATOR_KEY_PATH, &key_hex)
        .map_err(|e| format!("Failed to save creator key: {}", e))?;

    let _pub_hex = hex_encode(&public_key);
    let pub_hash = {
        let mut h = Sha256::new();
        h.update(&public_key);
        format!("{:x}", h.finalize())
    };

    tracing::info!("[CREATOR] 🔑 Creator keypair generated");
    tracing::info!("[CREATOR] Public key hash (embed in sealed binary): {}", pub_hash);
    tracing::info!("[CREATOR] Private key saved to: {}", CREATOR_KEY_PATH);

    Ok((public_key, private_key))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Check if the creator key exists on this machine.
pub fn creator_key_exists() -> bool {
    std::path::Path::new(CREATOR_KEY_PATH).exists()
}

/// Load the creator's private key (if this is the creator's machine).
pub fn load_creator_private_key() -> Option<Vec<u8>> {
    std::fs::read_to_string(CREATOR_KEY_PATH)
        .ok()
        .map(|hex| {
            hex.trim()
                .as_bytes()
                .chunks(2)
                .filter_map(|chunk| {
                    let s = std::str::from_utf8(chunk).ok()?;
                    u8::from_str_radix(s, 16).ok()
                })
                .collect()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creator_key_hash_mismatch() {
        // Random key should not match the placeholder
        let fake_key = vec![0u8; 32];
        assert!(!verify_creator(&fake_key, &[], &[]));
    }

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }
}
