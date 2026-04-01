/// Creator Key — Cryptographic authentication for the HIVE creator.
///
/// Uses real ed25519 digital signatures via `ed25519-dalek`.
///
/// The creator has a private ed25519 key stored on their physical machine.
/// The matching public key hash is embedded in the sealed binary.
/// Only the creator can:
/// - Mint HIVE Coin
/// - Access the monitoring dashboard
/// - Issue network-wide directives
/// - Override governance decisions
///
/// Users CANNOT:
/// - See the creator key
/// - Authenticate as creator
/// - Access creator telemetry
/// - Override creator decisions
///
/// PHYSICAL POSSESSION REQUIRED: The key file must exist on disk.
/// No runtime generation. No remote access. No delegation.
use sha2::{Sha256, Digest};
use ed25519_dalek::{SigningKey, VerifyingKey, Signer, Verifier, Signature};
use rand::rngs::OsRng;

/// The creator's public key hash (SHA-256 of the ed25519 verifying key bytes).
/// This is embedded in the sealed binary and cannot be changed without recompilation.
/// Set to a placeholder until the actual key is generated with `generate_creator_keypair()`.
const CREATOR_KEY_HASH: &str = "65e8bc87c9eb939feb4b84b82068ed4a25f439e27fce59ccf7b609af67440805";

/// Path where the creator's private key is stored (creator's machine only).
const CREATOR_KEY_PATH: &str = ".hive/creator.key";

/// Verify a challenge-response from a potential creator.
///
/// 1. Hash the provided public key and check against the embedded CREATOR_KEY_HASH.
/// 2. Verify the ed25519 signature over the challenge nonce.
/// Both checks must pass.
pub fn verify_creator(public_key_bytes: &[u8], signature_bytes: &[u8], challenge: &[u8]) -> bool {
    // ── Step 1: Verify the public key matches the embedded hash ───────
    let mut hasher = Sha256::new();
    hasher.update(public_key_bytes);
    let key_hash = format!("{:x}", hasher.finalize());

    if key_hash != CREATOR_KEY_HASH {
        tracing::warn!("[CREATOR] ❌ Public key hash mismatch — not the creator");
        return false;
    }

    // ── Step 2: Verify the ed25519 signature over the challenge ───────
    let vk_bytes: [u8; 32] = match public_key_bytes.try_into() {
        Ok(b) => b,
        Err(_) => {
            tracing::warn!("[CREATOR] ❌ Invalid public key length (expected 32 bytes)");
            return false;
        }
    };

    let verifying_key = match VerifyingKey::from_bytes(&vk_bytes) {
        Ok(vk) => vk,
        Err(_) => {
            tracing::warn!("[CREATOR] ❌ Invalid ed25519 public key");
            return false;
        }
    };

    let sig_bytes: [u8; 64] = match signature_bytes.try_into() {
        Ok(b) => b,
        Err(_) => {
            tracing::warn!("[CREATOR] ❌ Invalid signature length (expected 64 bytes)");
            return false;
        }
    };

    let signature = Signature::from_bytes(&sig_bytes);

    match verifying_key.verify(challenge, &signature) {
        Ok(()) => {
            tracing::info!("[CREATOR] ✅ Creator authenticated (ed25519 verified)");
            true
        }
        Err(_) => {
            tracing::warn!("[CREATOR] ❌ Signature verification failed — not the creator");
            false
        }
    }
}

/// Generate a new creator ed25519 keypair. Called ONCE during initial setup.
/// The private key is saved to CREATOR_KEY_PATH on the creator's machine.
/// The public key hash is printed — it must be embedded in the sealed binary.
pub fn generate_creator_keypair() -> Result<(Vec<u8>, Vec<u8>), String> {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    let private_key_bytes = signing_key.to_bytes().to_vec();
    let public_key_bytes = verifying_key.to_bytes().to_vec();

    // Save private key to creator key path
    let key_dir = std::path::Path::new(CREATOR_KEY_PATH)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let _ = std::fs::create_dir_all(key_dir);

    let key_hex = hex_encode(&private_key_bytes);
    std::fs::write(CREATOR_KEY_PATH, &key_hex)
        .map_err(|e| format!("Failed to save creator key: {}", e))?;

    // Compute the public key hash (this goes into the sealed binary)
    let pub_hash = {
        let mut h = Sha256::new();
        h.update(&public_key_bytes);
        format!("{:x}", h.finalize())
    };

    tracing::info!("[CREATOR] 🔑 Creator ed25519 keypair generated");
    tracing::info!("[CREATOR] Public key hash (embed in CREATOR_KEY_HASH): {}", pub_hash);
    tracing::info!("[CREATOR] Private key saved to: {}", CREATOR_KEY_PATH);

    Ok((public_key_bytes, private_key_bytes))
}

/// Sign a challenge with the creator's private key.
/// Returns the ed25519 signature bytes or None if this isn't the creator's machine.
pub fn sign_challenge(challenge: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let private_bytes = load_creator_private_key()?;
    let key_bytes: [u8; 32] = private_bytes.try_into().ok()?;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();

    let signature = signing_key.sign(challenge);

    Some((
        verifying_key.to_bytes().to_vec(),
        signature.to_bytes().to_vec(),
    ))
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

// ─── Pre-generated Key Import ───────────────────────────────────────

/// Import a pre-generated ed25519 private key from hex string.
/// Use this instead of generate_creator_keypair() for production.
pub fn import_creator_key(private_key_hex: &str) -> Result<Vec<u8>, String> {
    let private_bytes: Vec<u8> = private_key_hex.trim()
        .as_bytes()
        .chunks(2)
        .filter_map(|chunk| {
            let s = std::str::from_utf8(chunk).ok()?;
            u8::from_str_radix(s, 16).ok()
        })
        .collect();

    if private_bytes.len() != 32 {
        return Err(format!("Invalid key length: {} bytes (expected 32)", private_bytes.len()));
    }

    // Verify it's a valid ed25519 key
    let key_bytes: [u8; 32] = private_bytes.clone().try_into()
        .map_err(|_| "Invalid key bytes".to_string())?;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();
    let public_bytes = verifying_key.to_bytes().to_vec();

    // Save to disk
    let key_dir = std::path::Path::new(CREATOR_KEY_PATH)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let _ = std::fs::create_dir_all(key_dir);
    std::fs::write(CREATOR_KEY_PATH, private_key_hex.trim())
        .map_err(|e| format!("Failed to save creator key: {}", e))?;

    let pub_hash = {
        let mut h = Sha256::new();
        h.update(&public_bytes);
        format!("{:x}", h.finalize())
    };

    tracing::info!("[CREATOR] 🔑 Pre-generated key imported");
    tracing::info!("[CREATOR] Public key hash (embed in CREATOR_KEY_HASH): {}", pub_hash);

    Ok(public_bytes)
}

// ─── Backup & Recovery (2FA-style) ──────────────────────────────────

/// Path for the encrypted backup file.
const BACKUP_PATH: &str = ".hive/creator.backup";

/// Create an encrypted backup of the creator key.
/// The backup is AES-256-GCM encrypted with a passphrase-derived key (Argon2).
/// Store this file on a USB drive, second machine, or secure cloud storage.
///
/// RECOVERY: If the original machine is compromised or destroyed,
/// use `recover_from_backup()` with the passphrase to restore the key.
pub fn create_backup(passphrase: &str) -> Result<String, String> {
    let private_key = load_creator_private_key()
        .ok_or("No creator key found to back up")?;

    // Derive encryption key from passphrase using Argon2
    let salt = {
        let mut s = [0u8; 16];
        use rand::RngCore;
        OsRng.fill_bytes(&mut s);
        s
    };

    let mut derived_key = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(
            passphrase.as_bytes(),
            &salt,
            &mut derived_key,
        )
        .map_err(|e| format!("Argon2 key derivation failed: {}", e))?;

    // Encrypt with AES-256-GCM
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
    use aes_gcm::Nonce as AesNonce;

    let cipher = Aes256Gcm::new((&derived_key).into());
    let nonce_bytes = {
        let mut n = [0u8; 12];
        use rand::RngCore;
        OsRng.fill_bytes(&mut n);
        n
    };
    let nonce = AesNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, private_key.as_slice())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    // Save backup: salt (16) || nonce (12) || ciphertext
    let mut backup_data = Vec::new();
    backup_data.extend_from_slice(&salt);
    backup_data.extend_from_slice(&nonce_bytes);
    backup_data.extend_from_slice(&ciphertext);

    // Save as hex
    let backup_hex = hex_encode(&backup_data);

    let key_dir = std::path::Path::new(BACKUP_PATH)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let _ = std::fs::create_dir_all(key_dir);
    std::fs::write(BACKUP_PATH, &backup_hex)
        .map_err(|e| format!("Failed to save backup: {}", e))?;

    tracing::info!("[CREATOR] 💾 Encrypted backup created at {}", BACKUP_PATH);
    tracing::info!("[CREATOR] ⚠️ Store this file + your passphrase separately (like 2FA)");
    tracing::info!("[CREATOR] ⚠️ Without the passphrase, the backup is UNRECOVERABLE");

    Ok(backup_hex)
}

/// Recover the creator key from an encrypted backup file.
/// Requires the original passphrase used during backup creation.
pub fn recover_from_backup(backup_hex: &str, passphrase: &str) -> Result<Vec<u8>, String> {
    let backup_data: Vec<u8> = backup_hex.trim()
        .as_bytes()
        .chunks(2)
        .filter_map(|chunk| {
            let s = std::str::from_utf8(chunk).ok()?;
            u8::from_str_radix(s, 16).ok()
        })
        .collect();

    if backup_data.len() < 28 {
        return Err("Backup data too short".to_string());
    }

    // Extract salt (16) || nonce (12) || ciphertext
    let salt = &backup_data[..16];
    let nonce_bytes = &backup_data[16..28];
    let ciphertext = &backup_data[28..];

    // Derive key from passphrase
    let mut derived_key = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(
            passphrase.as_bytes(),
            salt,
            &mut derived_key,
        )
        .map_err(|e| format!("Argon2 key derivation failed: {}", e))?;

    // Decrypt with AES-256-GCM
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
    use aes_gcm::Nonce as AesNonce;

    let cipher = Aes256Gcm::new((&derived_key).into());
    let nonce = AesNonce::from_slice(nonce_bytes);

    let private_key = cipher.decrypt(nonce, ciphertext)
        .map_err(|_| "Decryption failed — wrong passphrase or corrupted backup".to_string())?;

    if private_key.len() != 32 {
        return Err(format!("Recovered key has invalid length: {} bytes", private_key.len()));
    }

    // Save recovered key to disk
    let key_hex = hex_encode(&private_key);
    let key_dir = std::path::Path::new(CREATOR_KEY_PATH)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let _ = std::fs::create_dir_all(key_dir);
    std::fs::write(CREATOR_KEY_PATH, &key_hex)
        .map_err(|e| format!("Failed to save recovered key: {}", e))?;

    // Compute public key hash for verification
    let key_bytes: [u8; 32] = private_key.clone().try_into()
        .map_err(|_| "Invalid key bytes".to_string())?;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();
    let public_bytes = verifying_key.to_bytes().to_vec();

    let pub_hash = {
        let mut h = Sha256::new();
        h.update(&public_bytes);
        format!("{:x}", h.finalize())
    };

    tracing::info!("[CREATOR] 🔑 Creator key recovered from backup!");
    tracing::info!("[CREATOR] Public key hash: {}", pub_hash);

    if pub_hash == CREATOR_KEY_HASH {
        tracing::info!("[CREATOR] ✅ Hash matches embedded CREATOR_KEY_HASH — recovery verified");
    } else {
        tracing::warn!("[CREATOR] ⚠️ Hash does NOT match embedded CREATOR_KEY_HASH");
        tracing::warn!("[CREATOR] This may be a different key or the binary needs recompilation");
    }

    Ok(public_bytes)
}

/// Load backup from disk (convenience).
pub fn load_backup_from_disk() -> Option<String> {
    std::fs::read_to_string(BACKUP_PATH).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creator_key_hash_mismatch() {
        // Random key should not match the placeholder
        let fake_key = vec![0u8; 32];
        assert!(!verify_creator(&fake_key, &[0u8; 64], &[1, 2, 3]));
    }

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn test_real_ed25519_creator_flow() {
        // Generate a keypair
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let public_bytes = verifying_key.to_bytes().to_vec();

        // Compute what the embedded hash would be
        let mut hasher = Sha256::new();
        hasher.update(&public_bytes);
        let expected_hash = format!("{:x}", hasher.finalize());

        // Sign a challenge
        let challenge = b"test challenge nonce 12345";
        let signature = signing_key.sign(challenge);

        // Verify signature directly (bypassing CREATOR_KEY_HASH check)
        assert!(verifying_key.verify(challenge, &signature).is_ok());

        // The verify_creator should fail because the hash won't match
        // the hardcoded CREATOR_KEY_HASH (which is a placeholder)
        assert!(!verify_creator(
            &public_bytes,
            &signature.to_bytes(),
            challenge,
        ));

        // But the signature IS valid — the only reason it fails is the hash mismatch
        assert_ne!(expected_hash, CREATOR_KEY_HASH);
    }

    #[test]
    fn test_invalid_signature_length() {
        // Too-short signature should be rejected
        let fake_key = vec![0u8; 32];
        assert!(!verify_creator(&fake_key, &[0u8; 32], &[1, 2, 3]));
    }

    #[test]
    fn test_invalid_key_length() {
        // Too-short key should be rejected
        let fake_key = vec![0u8; 16];
        assert!(!verify_creator(&fake_key, &[0u8; 64], &[1, 2, 3]));
    }
}
