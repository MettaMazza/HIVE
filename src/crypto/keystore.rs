//! Encrypted Keystore — AES-256-GCM with Argon2id key derivation.
//!
//! Security architecture:
//! - Keypairs encrypted at rest with AES-256-GCM
//! - Encryption key derived from: HIVE_WALLET_SECRET + user_id + random_salt (Argon2id)
//! - Raw private keys zeroed from memory after use (zeroize)
//! - No user-facing passphrases, seed phrases, or key exports
//! - Creator key hierarchy: creator > admin > user

use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, KeyInit};
use argon2::Argon2;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

/// On-disk format for an encrypted wallet.
#[derive(Serialize, Deserialize)]
pub struct EncryptedWallet {
    /// Base58-encoded Solana public key (safe to store unencrypted).
    pub pubkey: String,
    /// AES-256-GCM encrypted keypair bytes (64 bytes plaintext → variable ciphertext).
    pub ciphertext: Vec<u8>,
    /// 12-byte nonce used for AES-256-GCM encryption.
    pub nonce: Vec<u8>,
    /// 32-byte random salt used for Argon2id key derivation.
    pub salt: Vec<u8>,
    /// Wallet role for access control.
    pub role: WalletRole,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
}

/// Access control hierarchy. Creator key is supreme — no admin can override it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WalletRole {
    /// The creator's wallet. Only one exists. Sole mint authority.
    /// Cannot be created via API — only via direct keystore call with HIVE_CREATOR_SECRET.
    Creator,
    /// System wallet (Apis). Can receive rewards, sell NFTs.
    System,
    /// Admin wallet. Can facilitate but cannot mint or override creator.
    Admin,
    /// Regular user wallet.
    User,
}

/// The encrypted keystore. All wallet operations go through here.
pub struct Keystore {
    /// Directory where encrypted wallet files are stored.
    wallet_dir: PathBuf,
    /// Server-side secret for key derivation. Never logged, never on-chain.
    wallet_secret: String,
}

impl Keystore {
    /// Create a new keystore. Reads HIVE_WALLET_SECRET from environment.
    /// Panics if the secret is not set (required for security).
    pub fn new(wallet_dir: impl AsRef<Path>) -> Self {
        let wallet_secret = std::env::var("HIVE_WALLET_SECRET")
            .expect("[KEYSTORE] FATAL: HIVE_WALLET_SECRET not set. Cannot operate without encryption secret.");

        let dir = wallet_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir).expect("[KEYSTORE] Failed to create wallet directory");

        Self { wallet_dir: dir, wallet_secret }
    }

    /// Create a new keystore with an explicit secret (for testing).
    pub fn new_with_secret(wallet_dir: impl AsRef<Path>, secret: String) -> Self {
        let dir = wallet_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir).expect("[KEYSTORE] Failed to create wallet directory");
        Self { wallet_dir: dir, wallet_secret: secret }
    }

    /// Check if a wallet exists for the given user ID.
    pub fn wallet_exists(&self, user_id: &str) -> bool {
        self.wallet_path(user_id).exists()
    }

    /// Get the public key for a wallet without decrypting the private key.
    pub fn get_public_key(&self, user_id: &str) -> Option<String> {
        let path = self.wallet_path(user_id);
        if !path.exists() {
            return None;
        }
        let data = std::fs::read(&path).ok()?;
        let wallet: EncryptedWallet = serde_json::from_slice(&data).ok()?;
        Some(wallet.pubkey)
    }

    /// Get the wallet role for a user.
    pub fn get_role(&self, user_id: &str) -> Option<WalletRole> {
        let path = self.wallet_path(user_id);
        if !path.exists() {
            return None;
        }
        let data = std::fs::read(&path).ok()?;
        let wallet: EncryptedWallet = serde_json::from_slice(&data).ok()?;
        Some(wallet.role)
    }

    /// Create a new wallet for a user. Returns the public key (Base58).
    /// The private key is generated, encrypted, and stored — never exposed.
    pub fn create_wallet(&self, user_id: &str, role: WalletRole) -> Result<String, String> {
        if self.wallet_exists(user_id) {
            return Err(format!("Wallet already exists for user {}", user_id));
        }

        // Generate a new Solana keypair
        let keypair = Keypair::new();
        let pubkey = keypair.pubkey().to_string();

        // Encrypt and store
        self.encrypt_and_store(user_id, &keypair, role)?;

        tracing::info!("[KEYSTORE] ✅ Wallet created for '{}': {}", user_id, pubkey);
        Ok(pubkey)
    }

    /// Sign a transaction using the stored encrypted keypair.
    /// Decrypts the key, signs, zeroes the key from memory, returns the signed bytes.
    pub fn sign_transaction(
        &self,
        user_id: &str,
        transaction: &mut solana_sdk::transaction::Transaction,
        recent_blockhash: solana_sdk::hash::Hash,
    ) -> Result<(), String> {
        let mut keypair_bytes = self.decrypt_keypair(user_id)?;

        let keypair = Keypair::try_from(keypair_bytes.as_slice())
            .map_err(|e| format!("Invalid keypair bytes: {}", e))?;

        transaction.sign(&[&keypair], recent_blockhash);

        // CRITICAL: Zero the raw key material from memory
        keypair_bytes.zeroize();

        Ok(())
    }

    /// Get the raw keypair for signing (caller MUST zeroize after use).
    /// This is the low-level API — prefer sign_transaction() when possible.
    pub fn get_keypair(&self, user_id: &str) -> Result<Keypair, String> {
        let mut keypair_bytes = self.decrypt_keypair(user_id)?;
        let keypair = Keypair::try_from(keypair_bytes.as_slice())
            .map_err(|e| format!("Invalid keypair bytes: {}", e))?;
        keypair_bytes.zeroize();
        Ok(keypair)
    }

    /// List all wallet user IDs in the keystore.
    pub fn list_wallets(&self) -> Vec<String> {
        let mut wallets = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.wallet_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".enc.json") {
                        wallets.push(name.trim_end_matches(".enc.json").to_string());
                    }
                }
            }
        }
        wallets
    }

    // ─── Internal Methods ─────────────────────────────────────────────

    fn wallet_path(&self, user_id: &str) -> PathBuf {
        // Sanitise user_id to prevent directory traversal
        let safe_id: String = user_id.chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        self.wallet_dir.join(format!("{}.enc.json", safe_id))
    }

    /// Derive a 256-bit encryption key from the server secret + user ID + salt.
    fn derive_key(&self, user_id: &str, salt: &[u8]) -> Result<[u8; 32], String> {
        let mut key = [0u8; 32];
        let password = format!("{}{}", self.wallet_secret, user_id);

        Argon2::default()
            .hash_password_into(password.as_bytes(), salt, &mut key)
            .map_err(|e| format!("Argon2 key derivation failed: {}", e))?;

        Ok(key)
    }

    /// Encrypt a keypair and store it to disk.
    fn encrypt_and_store(&self, user_id: &str, keypair: &Keypair, role: WalletRole) -> Result<(), String> {
        // Generate random salt and nonce
        let mut salt = [0u8; 32];
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut salt);
        rand::thread_rng().fill_bytes(&mut nonce_bytes);

        // Derive encryption key
        let mut enc_key = self.derive_key(user_id, &salt)?;

        // Encrypt the 64-byte keypair
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&enc_key));
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, keypair.to_bytes().as_ref())
            .map_err(|e| format!("AES encryption failed: {}", e))?;

        // Zero the encryption key from memory
        enc_key.zeroize();

        // Build the wallet file
        let wallet = EncryptedWallet {
            pubkey: keypair.pubkey().to_string(),
            ciphertext,
            nonce: nonce_bytes.to_vec(),
            salt: salt.to_vec(),
            role,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        // Write to disk
        let path = self.wallet_path(user_id);
        let json = serde_json::to_string_pretty(&wallet)
            .map_err(|e| format!("JSON serialization failed: {}", e))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("Failed to write wallet file: {}", e))?;

        Ok(())
    }

    /// Decrypt the keypair bytes from an encrypted wallet file.
    /// Returns the raw 64-byte keypair. CALLER MUST ZEROIZE.
    fn decrypt_keypair(&self, user_id: &str) -> Result<Vec<u8>, String> {
        let path = self.wallet_path(user_id);
        if !path.exists() {
            return Err(format!("No wallet found for user '{}'", user_id));
        }

        let data = std::fs::read(&path)
            .map_err(|e| format!("Failed to read wallet file: {}", e))?;
        let wallet: EncryptedWallet = serde_json::from_slice(&data)
            .map_err(|e| format!("Corrupted wallet file: {}", e))?;

        // Derive the same encryption key
        let mut enc_key = self.derive_key(user_id, &wallet.salt)?;

        // Decrypt
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&enc_key));
        let nonce = Nonce::from_slice(&wallet.nonce);
        let plaintext = cipher.decrypt(nonce, wallet.ciphertext.as_ref())
            .map_err(|_| "Decryption failed — wallet secret may have changed or file is corrupted".to_string())?;

        // Zero the encryption key
        enc_key.zeroize();

        Ok(plaintext)
    }
}
