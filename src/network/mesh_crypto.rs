/// Mesh Encryption — Production E2E encryption for the HIVE mesh.
///
/// Uses real cryptographic primitives:
/// - **ed25519-dalek** for digital signatures (identity + message signing)
/// - **x25519-dalek** for Diffie-Hellman key exchange (shared secret derivation)
/// - **chacha20poly1305** for AEAD encryption (authenticated encryption with associated data)
///
/// Every mesh message is signed by the sender's ed25519 key and can be
/// encrypted point-to-point using X25519 key exchange + ChaCha20-Poly1305.
///
/// SECURITY:
/// - Constant-time operations via dalek crates
/// - 12-byte random nonces (never reused — generated per-message)
/// - Authenticated encryption prevents tampering & truncation
/// - Key material zeroized on drop
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;

use ed25519_dalek::{SigningKey, VerifyingKey, Signer, Verifier, Signature};
use x25519_dalek::{StaticSecret, PublicKey as X25519PublicKey};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::rngs::OsRng;

// ─── Simulation Mode ────────────────────────────────────────────────────
//
// When HIVE_CRYPTO_SIMULATION=true:
//   - sign() returns dummy 64-byte zero signatures
//   - verify() always returns true
//   - encrypt_message() returns plaintext as-is (no AEAD overhead)
//   - decrypt_message() returns ciphertext as-is
//   - seal()/open() still work structurally but without real crypto
//
// This allows the ENTIRE mesh to run end-to-end for testing while
// skipping the real cryptographic operations. All types and APIs are
// identical — switching to production is just flipping the env var off.

/// Check if crypto simulation mode is active.
pub fn is_simulation() -> bool {
    std::env::var("HIVE_CRYPTO_SIMULATION")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

// ─── Key Types ──────────────────────────────────────────────────────────

/// A 256-bit symmetric key (32 bytes).
pub type Key256 = [u8; 32];

/// A mesh identity keypair — ed25519 for signing, x25519 for encryption.
#[derive(Clone)]
pub struct MeshKeypair {
    /// Ed25519 signing key (private)
    pub signing_key: SigningKey,
    /// Ed25519 verifying key (public) — serializable
    pub verifying_key: VerifyingKey,
    /// X25519 static secret (for key exchange)
    x25519_secret: StaticSecret,
    /// X25519 public key (shareable)
    pub x25519_public: X25519PublicKey,
    /// When this keypair was created
    pub created_at: String,
}

/// Serializable form of the keypair (for disk persistence).
#[derive(Serialize, Deserialize)]
pub struct MeshKeypairSnapshot {
    pub ed25519_secret: Vec<u8>,  // 32 bytes
    pub x25519_secret: Vec<u8>,   // 32 bytes
    pub created_at: String,
}

/// An encrypted + signed message envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedEnvelope {
    /// Ed25519 public key of the sender (32 bytes)
    pub sender_verifying_key: Vec<u8>,
    /// X25519 public key of the sender (32 bytes)
    pub sender_x25519_public: Vec<u8>,
    /// 12-byte random nonce (unique per message)
    pub nonce: Vec<u8>,
    /// AEAD ciphertext (includes Poly1305 authentication tag)
    pub ciphertext: Vec<u8>,
    /// Ed25519 signature over (nonce || ciphertext)
    pub signature: Vec<u8>,
    /// Timestamp for replay protection
    pub timestamp: String,
}

// ─── Key Generation ─────────────────────────────────────────────────────

/// Generate a new mesh keypair with real cryptographic keys.
pub fn generate_keypair() -> MeshKeypair {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    let x25519_secret = StaticSecret::random_from_rng(OsRng);
    let x25519_public = X25519PublicKey::from(&x25519_secret);

    MeshKeypair {
        signing_key,
        verifying_key,
        x25519_secret,
        x25519_public,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

impl MeshKeypair {
    /// Serialize to a persistable snapshot.
    pub fn to_snapshot(&self) -> MeshKeypairSnapshot {
        MeshKeypairSnapshot {
            ed25519_secret: self.signing_key.to_bytes().to_vec(),
            x25519_secret: self.x25519_secret.to_bytes().to_vec(),
            created_at: self.created_at.clone(),
        }
    }

    /// Restore from a snapshot.
    pub fn from_snapshot(snap: &MeshKeypairSnapshot) -> Result<Self, String> {
        let ed_bytes: [u8; 32] = snap.ed25519_secret.clone()
            .try_into()
            .map_err(|_| "Invalid ed25519 secret length".to_string())?;
        let signing_key = SigningKey::from_bytes(&ed_bytes);
        let verifying_key = signing_key.verifying_key();

        let x_bytes: [u8; 32] = snap.x25519_secret.clone()
            .try_into()
            .map_err(|_| "Invalid x25519 secret length".to_string())?;
        let x25519_secret = StaticSecret::from(x_bytes);
        let x25519_public = X25519PublicKey::from(&x25519_secret);

        Ok(Self {
            signing_key,
            verifying_key,
            x25519_secret,
            x25519_public,
            created_at: snap.created_at.clone(),
        })
    }

    /// Get the ed25519 verifying key bytes (public identity).
    pub fn public_key_bytes(&self) -> Vec<u8> {
        self.verifying_key.to_bytes().to_vec()
    }

    /// Get the x25519 public key bytes (for key exchange).
    pub fn x25519_public_bytes(&self) -> Vec<u8> {
        self.x25519_public.to_bytes().to_vec()
    }
}

// ─── Signing ────────────────────────────────────────────────────────────

/// Sign arbitrary data with ed25519.
/// In simulation mode, returns a 64-byte zero signature.
pub fn sign(data: &[u8], keypair: &MeshKeypair) -> Vec<u8> {
    if is_simulation() {
        return vec![0u8; 64];
    }
    keypair.signing_key.sign(data).to_bytes().to_vec()
}

/// Verify an ed25519 signature.
/// In simulation mode, always returns true.
pub fn verify(data: &[u8], signature_bytes: &[u8], verifying_key_bytes: &[u8]) -> bool {
    if is_simulation() {
        return true;
    }
    let vk_bytes: [u8; 32] = match verifying_key_bytes.try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let verifying_key = match VerifyingKey::from_bytes(&vk_bytes) {
        Ok(vk) => vk,
        Err(_) => return false,
    };
    let sig_bytes: [u8; 64] = match signature_bytes.try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let signature = Signature::from_bytes(&sig_bytes);
    verifying_key.verify(data, &signature).is_ok()
}

// ─── Key Exchange ───────────────────────────────────────────────────────

/// Compute a shared secret using X25519 Diffie-Hellman.
/// Both parties derive the same 32-byte shared secret.
pub fn compute_shared_secret(our_keypair: &MeshKeypair, their_x25519_public: &[u8]) -> Key256 {
    let their_bytes: [u8; 32] = their_x25519_public
        .try_into()
        .unwrap_or([0u8; 32]);
    let their_public = X25519PublicKey::from(their_bytes);
    let shared = our_keypair.x25519_secret.diffie_hellman(&their_public);
    *shared.as_bytes()
}

// ─── AEAD Encryption ────────────────────────────────────────────────────

/// Encrypt plaintext with ChaCha20-Poly1305 AEAD.
/// Returns (ciphertext_with_tag, nonce).
/// In simulation mode, returns plaintext as-is with a zero nonce.
pub fn encrypt_message(plaintext: &[u8], shared_key: &Key256) -> Result<(Vec<u8>, Vec<u8>), String> {
    if is_simulation() {
        return Ok((plaintext.to_vec(), vec![0u8; 12]));
    }
    let cipher = ChaCha20Poly1305::new(shared_key.into());

    let mut nonce_bytes = [0u8; 12];
    use rand::RngCore;
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, plaintext)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    Ok((ciphertext, nonce_bytes.to_vec()))
}

/// Decrypt ciphertext with ChaCha20-Poly1305 AEAD.
/// Returns plaintext or error if authentication fails (tampered message).
/// In simulation mode, returns ciphertext as-is (it was never encrypted).
pub fn decrypt_message(ciphertext: &[u8], shared_key: &Key256, nonce_bytes: &[u8]) -> Result<Vec<u8>, String> {
    if is_simulation() {
        return Ok(ciphertext.to_vec());
    }
    let cipher = ChaCha20Poly1305::new(shared_key.into());

    let nonce_arr: [u8; 12] = nonce_bytes
        .try_into()
        .map_err(|_| "Invalid nonce length (expected 12 bytes)".to_string())?;
    let nonce = Nonce::from_slice(&nonce_arr);

    cipher.decrypt(nonce, ciphertext)
        .map_err(|_| "Decryption failed: authentication tag mismatch (message tampered)".to_string())
}

// ─── Envelope Creation ──────────────────────────────────────────────────

/// Create an encrypted + signed envelope for sending to a specific peer.
pub fn seal(plaintext: &str, sender: &MeshKeypair, recipient_x25519_public: &[u8]) -> Result<EncryptedEnvelope, String> {
    let shared = compute_shared_secret(sender, recipient_x25519_public);
    let (ciphertext, nonce) = encrypt_message(plaintext.as_bytes(), &shared)?;

    // Sign (nonce || ciphertext) so the recipient can verify authenticity
    let mut signed_data = Vec::with_capacity(nonce.len() + ciphertext.len());
    signed_data.extend_from_slice(&nonce);
    signed_data.extend_from_slice(&ciphertext);
    let signature = sign(&signed_data, sender);

    Ok(EncryptedEnvelope {
        sender_verifying_key: sender.public_key_bytes(),
        sender_x25519_public: sender.x25519_public_bytes(),
        nonce,
        ciphertext,
        signature,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Open an encrypted + signed envelope.
/// Verifies the signature before decrypting.
pub fn open(envelope: &EncryptedEnvelope, recipient: &MeshKeypair) -> Result<String, String> {
    // 1. Verify signature
    let mut signed_data = Vec::with_capacity(envelope.nonce.len() + envelope.ciphertext.len());
    signed_data.extend_from_slice(&envelope.nonce);
    signed_data.extend_from_slice(&envelope.ciphertext);

    if !verify(&signed_data, &envelope.signature, &envelope.sender_verifying_key) {
        return Err("Signature verification failed — message is not authentic".to_string());
    }

    // 2. Decrypt
    let shared = compute_shared_secret(recipient, &envelope.sender_x25519_public);
    let plaintext = decrypt_message(&envelope.ciphertext, &shared, &envelope.nonce)?;

    String::from_utf8(plaintext)
        .map_err(|e| format!("Decrypted data is not valid UTF-8: {}", e))
}

// ─── Key Store ──────────────────────────────────────────────────────────

/// Persistent key store for managing our keypair and peer public keys.
pub struct KeyStore {
    our_keypair: RwLock<MeshKeypair>,
    /// Peer ID → (ed25519 verifying key, x25519 public key)
    peer_keys: RwLock<HashMap<String, (Vec<u8>, Vec<u8>)>>,
    persist_path: String,
}

#[derive(Serialize, Deserialize)]
struct KeyStoreSnapshot {
    keypair: MeshKeypairSnapshot,
    /// Peer ID → (ed25519_public, x25519_public)
    peers: HashMap<String, (Vec<u8>, Vec<u8>)>,
}

impl KeyStore {
    pub fn new() -> Self {
        let persist_path = "memory/mesh_keys.json".to_string();

        if let Ok(data) = std::fs::read_to_string(&persist_path) {
            if let Ok(snap) = serde_json::from_str::<KeyStoreSnapshot>(&data) {
                if let Ok(keypair) = MeshKeypair::from_snapshot(&snap.keypair) {
                    tracing::info!("[CRYPTO] 🔐 Loaded keypair + {} peer keys from disk", snap.peers.len());
                    return Self {
                        our_keypair: RwLock::new(keypair),
                        peer_keys: RwLock::new(snap.peers),
                        persist_path,
                    };
                }
            }
        }

        let keypair = generate_keypair();
        tracing::info!("[CRYPTO] 🔑 Generated new ed25519 + x25519 mesh keypair");

        Self {
            our_keypair: RwLock::new(keypair),
            peer_keys: RwLock::new(HashMap::new()),
            persist_path,
        }
    }

    #[cfg(test)]
    fn new_test() -> Self {
        Self {
            our_keypair: RwLock::new(generate_keypair()),
            peer_keys: RwLock::new(HashMap::new()),
            persist_path: format!("/tmp/hive_test_keys_{}.json", uuid::Uuid::new_v4()),
        }
    }

    pub async fn our_public_key(&self) -> Vec<u8> {
        self.our_keypair.read().await.public_key_bytes()
    }

    pub async fn our_x25519_public(&self) -> Vec<u8> {
        self.our_keypair.read().await.x25519_public_bytes()
    }

    pub async fn register_peer(&self, peer_id: &str, ed25519_public: Vec<u8>, x25519_public: Vec<u8>) {
        self.peer_keys.write().await.insert(
            peer_id.to_string(),
            (ed25519_public, x25519_public),
        );
        self.persist().await;
    }

    pub async fn encrypt_for_peer(&self, peer_id: &str, plaintext: &str) -> Result<EncryptedEnvelope, String> {
        let peers = self.peer_keys.read().await;
        let (_ed_key, x_key) = peers.get(peer_id)
            .ok_or_else(|| format!("No keys registered for peer {}", peer_id))?;
        let keypair = self.our_keypair.read().await;
        seal(plaintext, &keypair, x_key)
    }

    pub async fn decrypt_envelope(&self, envelope: &EncryptedEnvelope) -> Result<String, String> {
        let keypair = self.our_keypair.read().await;
        open(envelope, &keypair)
    }

    /// Sign data with our ed25519 key.
    pub async fn sign_data(&self, data: &[u8]) -> Vec<u8> {
        let keypair = self.our_keypair.read().await;
        sign(data, &keypair)
    }

    /// Verify a signature from a known peer.
    pub async fn verify_peer_signature(&self, peer_id: &str, data: &[u8], signature: &[u8]) -> bool {
        let peers = self.peer_keys.read().await;
        match peers.get(peer_id) {
            Some((ed_key, _)) => verify(data, signature, ed_key),
            None => false,
        }
    }

    async fn persist(&self) {
        let snap = KeyStoreSnapshot {
            keypair: self.our_keypair.read().await.to_snapshot(),
            peers: self.peer_keys.read().await.clone(),
        };
        if let Ok(json) = serde_json::to_string(&snap) {
            if let Some(parent) = std::path::Path::new(&self.persist_path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&self.persist_path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let kp = generate_keypair();
        assert_eq!(kp.public_key_bytes().len(), 32);
        assert_eq!(kp.x25519_public_bytes().len(), 32);
    }

    #[test]
    fn test_keypairs_are_unique() {
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();
        assert_ne!(kp1.public_key_bytes(), kp2.public_key_bytes());
        assert_ne!(kp1.x25519_public_bytes(), kp2.x25519_public_bytes());
    }

    #[test]
    fn test_sign_verify() {
        let kp = generate_keypair();
        let data = b"Hello, mesh!";
        let sig = sign(data, &kp);
        assert!(verify(data, &sig, &kp.public_key_bytes()));
        assert!(!verify(b"wrong data", &sig, &kp.public_key_bytes()));
    }

    #[test]
    fn test_sign_verify_wrong_key() {
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();
        let data = b"Hello";
        let sig = sign(data, &kp1);
        // Wrong key should fail
        assert!(!verify(data, &sig, &kp2.public_key_bytes()));
    }

    #[test]
    fn test_shared_secret_symmetric() {
        let alice = generate_keypair();
        let bob = generate_keypair();

        let secret_ab = compute_shared_secret(&alice, &bob.x25519_public_bytes());
        let secret_ba = compute_shared_secret(&bob, &alice.x25519_public_bytes());

        // Both parties derive the same shared secret
        assert_eq!(secret_ab, secret_ba);
    }

    #[test]
    fn test_shared_secret_different_for_different_pairs() {
        let alice = generate_keypair();
        let bob = generate_keypair();
        let eve = generate_keypair();

        let secret_ab = compute_shared_secret(&alice, &bob.x25519_public_bytes());
        let secret_ae = compute_shared_secret(&alice, &eve.x25519_public_bytes());

        assert_ne!(secret_ab, secret_ae);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key: Key256 = [42u8; 32];
        let plaintext = b"Hello, mesh! This is a secret message.";

        let (ciphertext, nonce) = encrypt_message(plaintext, &key).unwrap();
        // AEAD adds a 16-byte authentication tag
        assert_eq!(ciphertext.len(), plaintext.len() + 16);
        assert_ne!(&ciphertext[..plaintext.len()], &plaintext[..]);

        let decrypted = decrypt_message(&ciphertext, &key, &nonce).unwrap();
        assert_eq!(&decrypted[..], &plaintext[..]);
    }

    #[test]
    fn test_wrong_key_fails_aead() {
        let key1: Key256 = [42u8; 32];
        let key2: Key256 = [99u8; 32];
        let plaintext = b"Secret data";

        let (ciphertext, nonce) = encrypt_message(plaintext, &key1).unwrap();
        // AEAD decryption with wrong key MUST fail (authentication)
        let result = decrypt_message(&ciphertext, &key2, &nonce);
        assert!(result.is_err(), "Decryption with wrong key must fail");
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let key: Key256 = [42u8; 32];
        let plaintext = b"Secret data";

        let (mut ciphertext, nonce) = encrypt_message(plaintext, &key).unwrap();
        // Flip a byte in the ciphertext
        ciphertext[0] ^= 0xFF;
        let result = decrypt_message(&ciphertext, &key, &nonce);
        assert!(result.is_err(), "Tampered ciphertext must fail AEAD verification");
    }

    #[test]
    fn test_seal_open_envelope() {
        let alice = generate_keypair();
        let bob = generate_keypair();

        let envelope = seal("Top secret mesh intel", &alice, &bob.x25519_public_bytes()).unwrap();
        assert!(!envelope.ciphertext.is_empty());
        assert_eq!(envelope.sender_verifying_key, alice.public_key_bytes());

        let decrypted = open(&envelope, &bob).unwrap();
        assert_eq!(decrypted, "Top secret mesh intel");
    }

    #[test]
    fn test_envelope_wrong_recipient() {
        let alice = generate_keypair();
        let bob = generate_keypair();
        let eve = generate_keypair();

        let envelope = seal("For Bob only", &alice, &bob.x25519_public_bytes()).unwrap();
        let result = open(&envelope, &eve);
        // Eve can't decrypt — AEAD authentication will fail
        assert!(result.is_err(), "Wrong recipient must fail AEAD");
    }

    #[test]
    fn test_envelope_tampered_signature() {
        let alice = generate_keypair();
        let bob = generate_keypair();

        let mut envelope = seal("Signed message", &alice, &bob.x25519_public_bytes()).unwrap();
        // Tamper with the signature
        envelope.signature[0] ^= 0xFF;
        let result = open(&envelope, &bob);
        assert!(result.is_err(), "Tampered signature must be rejected");
    }

    #[test]
    fn test_snapshot_roundtrip() {
        let kp = generate_keypair();
        let snap = kp.to_snapshot();
        let restored = MeshKeypair::from_snapshot(&snap).unwrap();
        assert_eq!(kp.public_key_bytes(), restored.public_key_bytes());
        assert_eq!(kp.x25519_public_bytes(), restored.x25519_public_bytes());

        // Signing should still work after restore
        let data = b"test data";
        let sig = sign(data, &restored);
        assert!(verify(data, &sig, &kp.public_key_bytes()));
    }

    #[tokio::test]
    async fn test_key_store() {
        let store = KeyStore::new_test();
        let our_key = store.our_public_key().await;
        assert_eq!(our_key.len(), 32);

        let bob = generate_keypair();
        store.register_peer(
            "bob",
            bob.public_key_bytes(),
            bob.x25519_public_bytes(),
        ).await;

        let envelope = store.encrypt_for_peer("bob", "Hello Bob").await.unwrap();
        assert!(!envelope.ciphertext.is_empty());

        // Bob should be able to decrypt
        let decrypted = open(&envelope, &bob).unwrap();
        assert_eq!(decrypted, "Hello Bob");
    }

    #[tokio::test]
    async fn test_key_store_sign_verify() {
        let store = KeyStore::new_test();
        let data = b"mesh protocol data";

        let sig = store.sign_data(data).await;
        let our_pub = store.our_public_key().await;

        assert!(verify(data, &sig, &our_pub));
    }

    #[test]
    fn test_empty_message() {
        let key: Key256 = [42u8; 32];
        let (ciphertext, nonce) = encrypt_message(b"", &key).unwrap();
        // AEAD tag only (16 bytes) for empty plaintext
        assert_eq!(ciphertext.len(), 16);
        let decrypted = decrypt_message(&ciphertext, &key, &nonce).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn test_large_message() {
        let key: Key256 = [42u8; 32];
        let plaintext = vec![0xAB; 10000];
        let (ciphertext, nonce) = encrypt_message(&plaintext, &key).unwrap();
        let decrypted = decrypt_message(&ciphertext, &key, &nonce).unwrap();
        assert_eq!(decrypted, plaintext);
    }
}
