/// NeuroLease Wire Protocol — Message types for Apis-to-Apis mesh communication.
///
/// All messages are signed with ed25519 and serialized with MessagePack.
/// The mesh module has NO access to working memory, timelines, or scoped data.
use serde::{Deserialize, Serialize};
use crate::memory::lessons::Lesson;
use crate::memory::synaptic::{SynapticNode, SynapticEdge};

// ─── Identity ───────────────────────────────────────────────────────────

/// Peer identity derived from ed25519 public key hash.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct PeerId(pub String);

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Show first 12 chars for readability
        let short = if self.0.len() > 12 { &self.0[..12] } else { &self.0 };
        write!(f, "{}", short)
    }
}

// ─── Peer Info ──────────────────────────────────────────────────────────

/// Metadata about a connected peer on the mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub addr: String,               // SocketAddr as string for serde
    pub last_seen: String,           // RFC3339
    pub version: String,             // HIVE git commit hash
    pub binary_hash: String,         // SHA-256 of the running HIVE binary
    pub source_hash: String,         // SHA-256 of src/ directory tree
}

// ─── Binary Attestation ─────────────────────────────────────────────────

/// Cryptographic proof that a peer is running unmodified HIVE code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attestation {
    pub binary_hash: String,         // SHA-256 of the running binary
    pub source_hash: String,         // SHA-256 of src/ directory tree
    pub commit: String,              // git commit hash
    pub signature: Vec<u8>,          // ed25519 signature of (binary_hash + source_hash + commit)
}

/// Challenge-response for live attestation verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationChallenge {
    pub nonce: Vec<u8>,              // 32-byte random nonce
    pub challenger: PeerId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationResponse {
    pub nonce: Vec<u8>,              // Echo the challenge nonce
    pub attestation: Attestation,    // Full attestation data
    pub nonce_signature: Vec<u8>,    // sign(nonce + binary_hash) — proves liveness
}

// ─── Quarantine ─────────────────────────────────────────────────────────

/// Network-wide quarantine notice for compromised peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineNotice {
    pub target_peer: PeerId,         // The peer being quarantined
    pub reason: String,              // Human-readable reason
    pub evidence_hash: String,       // Hash of the evidence (for independent verification)
    pub issued_by: PeerId,           // Who issued the quarantine
    pub issued_at: String,           // RFC3339
    pub signature: Vec<u8>,          // ed25519 signature of the issuer
}

// ─── Governance & Emergency Types ────────────────────────────────────────

/// Alert severity for emergency broadcasts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
    Catastrophic,
}

/// Crisis categories for emergency alerts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrisisCategory {
    ConnectivityLost,
    CensorshipActive,
    InfrastructureFailure,
    SafetyAlert,
    ResourceAvailable,
}

/// Types of resources a peer can advertise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourceType {
    InternetRelay,
    Storage,
    Compute,
    DnsResolver,
    FileHosting,
}

// ─── Wire Protocol ──────────────────────────────────────────────────────

/// All mesh messages. Every variant is signed and schema-validated before processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshMessage {
    // ── Discovery ──
    Ping {
        peer_id: PeerId,
        version: String,
        attestation: Attestation,
    },
    Pong {
        peer_id: PeerId,
        peers: Vec<PeerInfo>,
        attestation: Attestation,
    },

    // ── Attestation ──
    Challenge(AttestationChallenge),
    ChallengeResponse(AttestationResponse),

    // ── Knowledge Sync ──
    LessonBroadcast {
        lesson: Lesson,
        origin: PeerId,
        timestamp: String,           // RFC3339
    },
    SynapticDelta {
        nodes: Vec<SynapticNode>,
        edges: Vec<SynapticEdge>,
        origin: PeerId,
    },

    // ── Weight Exchange ──
    LoRAAnnounce {
        version: String,
        manifest_json: String,       // Serialized teacher::Manifest
        origin: PeerId,
    },
    LoRARequest {
        version: String,
        requester: PeerId,
    },
    LoRATransfer {
        version: String,
        adapter_bytes: Vec<u8>,
    },

    // ── Code Propagation ──
    CodePatch {
        diff: String,
        commit_hash: String,
        test_passed: bool,
        origin: PeerId,
    },
    CodePatchAck {
        commit_hash: String,
        applied: bool,
        peer_id: PeerId,
    },

    // ── Governance ──
    Quarantine(QuarantineNotice),

    // ── Apis-to-Apis Chat ──
    /// Direct message between Apis instances across the mesh.
    ApisChat {
        from_peer: PeerId,
        from_name: String,
        content: String,
        reply_to: Option<String>,
        timestamp: String,
    },
    /// Channel-based broadcast to all connected Apis instances.
    ApisBroadcast {
        from_peer: PeerId,
        from_name: String,
        channel: String,
        content: String,
        timestamp: String,
    },

    // ── Community Governance ──
    BanProposal {
        target: PeerId,
        reason: String,
        evidence_hash: String,
        proposer: PeerId,
    },
    BanVote {
        target: PeerId,
        voter: PeerId,
        approve: bool,
        signature: Vec<u8>,
    },

    // ── Emergency & Survival ──
    EmergencyAlert {
        severity: AlertSeverity,
        category: CrisisCategory,
        message: String,
        issuer: PeerId,
    },
    ResourceAdvertise {
        resource_type: ResourceType,
        capacity: String,
        issuer: PeerId,
    },
    OSINTReport {
        category: String,
        data: String,
        issuer: PeerId,
        signature: Vec<u8>,
    },

    // ── Relay ──
    RelayRequest {
        destination_url: String,
        requester: PeerId,
    },
    RelayResponse {
        data: Vec<u8>,
        content_type: String,
        provider: PeerId,
    },
}

/// Signed envelope wrapping every mesh message.
/// Peers MUST verify the signature before processing the inner message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedEnvelope {
    pub sender: PeerId,
    pub payload: Vec<u8>,            // MessagePack-serialized MeshMessage
    pub signature: Vec<u8>,          // ed25519 signature of payload
    pub timestamp: String,           // RFC3339 — for replay protection
}

// ─── Size Limits ────────────────────────────────────────────────────────

/// Maximum payload sizes (bytes) per message type.
/// Oversized payloads are rejected and the sender is flagged.
pub const MAX_LESSON_SIZE: usize = 2 * 1024;          // 2 KB
pub const MAX_SYNAPTIC_DELTA_SIZE: usize = 50 * 1024;  // 50 KB
pub const MAX_GOLDEN_SIZE: usize = 10 * 1024;          // 10 KB
pub const MAX_CODE_PATCH_SIZE: usize = 500 * 1024;     // 500 KB
pub const MAX_LORA_SIZE: usize = 200 * 1024 * 1024;    // 200 MB
pub const MAX_ENVELOPE_SIZE: usize = MAX_LORA_SIZE + 4096; // LoRA + overhead
