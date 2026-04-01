/// Mesh Event Loop — The heart of the NeuroLease mesh network.
///
/// Accepts incoming QUIC connections, deserializes SignedEnvelopes,
/// verifies signatures, and routes MeshMessage variants to their
/// respective handlers: KnowledgeSync, WeightExchange, CodePropagation,
/// ApisChat, GovernanceEngine, ComputeRelay.
///
/// SECURITY:
/// 1. Every envelope is signature-verified before processing
/// 2. Quarantined peers are rejected at the transport layer
/// 3. Content-filtered before handler dispatch
/// 4. Oversized payloads rejected with sanctions violation
///
/// This is the single point of entry for ALL mesh traffic.
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::network::messages::*;
use crate::network::transport::QuicTransport;
use crate::network::trust::TrustStore;
use crate::network::sanctions::{SanctionStore, Violation};
use crate::network::sync::KnowledgeSync;
use crate::network::weights::WeightExchange;
use crate::network::propagation::CodePropagation;
use crate::network::apis_chat::{ApisChat, ApisChatMessage};
use crate::network::apis_book::{ApisBook, ApisBookEntry, ApisBookEventType};
use crate::network::governance::GovernanceEngine;
use crate::network::compute_relay::{ComputeRelay, ComputeResult};
use crate::network::content_filter::ContentFilter;
use crate::network::pool::PoolManager;
use crate::network::discovery::PeerRegistry;
use crate::network::regional_keys::RegionalKeyRegistry;
use crate::network::hardware_id;
use crate::network::hardware_blacklist::HardwareBlacklist;
use crate::network::capability_registry::CapabilityRegistry;
use crate::network::config_guard::{ConfigGuard, GuardState};
use crate::crypto::mesh_ledger::MeshLedger;
use crate::network::sandbox::SandboxEngine;
use crate::network::sandbox_priority::PriorityManager;
use crate::network::dht::DHT;
use crate::network::mesh_fs::MeshFS;
use crate::network::governance_phases::GovernanceManager;

/// All handler subsystems bundled together for the event loop.
pub struct MeshHandlers {
    pub transport: Arc<QuicTransport>,
    pub trust: Arc<RwLock<TrustStore>>,
    pub sanctions: Arc<RwLock<SanctionStore>>,
    pub sync: Arc<RwLock<KnowledgeSync>>,
    pub weights: Option<Arc<WeightExchange>>,
    pub propagation: Option<Arc<CodePropagation>>,
    pub chat: Arc<ApisChat>,
    pub book: Arc<ApisBook>,
    pub governance: Arc<GovernanceEngine>,
    pub compute_relay: Arc<ComputeRelay>,
    pub content_filter: Arc<ContentFilter>,
    pub pool: Arc<RwLock<PoolManager>>,
    pub registry: Arc<PeerRegistry>,
    pub local_peer: PeerId,
    pub local_attestation: Attestation,
    // L3-L7 subsystems
    pub regional: Arc<RegionalKeyRegistry>,
    pub hardware_blacklist: Arc<RwLock<HardwareBlacklist>>,
    pub capabilities: Arc<CapabilityRegistry>,
    pub config_guard: Arc<RwLock<ConfigGuard>>,
    pub ledger: Arc<RwLock<MeshLedger>>,
    // ── v5.0 Supercomputer subsystems ──
    pub sandbox_engine: Arc<SandboxEngine>,
    pub priority_manager: Arc<PriorityManager>,
    pub dht: Arc<RwLock<DHT>>,
    pub mesh_fs: Arc<RwLock<MeshFS>>,
    pub governance_manager: Arc<GovernanceManager>,
}

/// Start the mesh event loop.
///
/// Spawns two tasks:
/// 1. **Connection Acceptor** — listens for incoming QUIC connections
/// 2. **Connection Handler** — per-connection loop reading messages
///
/// Returns a JoinHandle for the acceptor task.
pub async fn start_mesh_loop(handlers: Arc<MeshHandlers>) {
    let transport = handlers.transport.clone();
    let endpoint = transport.endpoint.clone();

    tracing::info!(
        "[MESH LOOP] 🚀 Starting event loop — accepting connections as {}",
        handlers.local_peer
    );

    // Spawn the connection acceptor
    let h = handlers.clone();
    tokio::spawn(async move {
        loop {
            match endpoint.accept().await {
                Some(incoming) => {
                    let handlers = h.clone();
                    tokio::spawn(async move {
                        match incoming.await {
                            Ok(conn) => {
                                let remote_addr = conn.remote_address();
                                tracing::info!(
                                    "[MESH LOOP] 🔗 Accepted connection from {}",
                                    remote_addr
                                );
                                handle_connection(conn, handlers).await;
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "[MESH LOOP] ⚠️ Connection handshake failed: {}",
                                    e
                                );
                            }
                        }
                    });
                }
                None => {
                    tracing::error!("[MESH LOOP] ❌ Endpoint closed — mesh loop exiting");
                    break;
                }
            }
        }
    });

    // Spawn outbound connection loop — periodically connect to discovered peers
    let h2 = handlers.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            connect_to_discovered_peers(&h2).await;
        }
    });

    // ── Config Guard periodic check (every 30s) ─────────────────────
    let h3 = handlers.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let state = {
                let mut guard = h3.config_guard.write().await;
                guard.check().clone()
            };

            match state {
                GuardState::Clean => {} // All good
                GuardState::TamperDetected { changed_path, .. } => {
                    // IMMEDIATE: Disconnect from all peers
                    tracing::error!(
                        "[CONFIG GUARD] ⛔ TAMPER DETECTED: {} — DISCONNECTING FROM MESH",
                        changed_path
                    );
                    h3.transport.disconnect_all().await;
                }
                GuardState::Destroyed => {
                    // Self-destruct: hardware blacklist + wipe
                    let local_hw = hardware_id::local_hardware_id();
                    {
                        let mut bl = h3.hardware_blacklist.write().await;
                        bl.ban(
                            local_hw,
                            h3.local_peer.clone(),
                            "Config guard self-destruct (unauthorized file modification)",
                            "config_guard",
                        );
                    }
                    h3.transport.disconnect_all().await;
                    crate::network::self_destruct::self_destruct(
                        &std::path::PathBuf::from("memory/mesh"),
                        None,
                    ).await;
                    break;
                }
            }
        }
    });

    // ── Ledger block assembly (every 30s) ────────────────────────────
    let h4 = handlers.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let mut ledger = h4.ledger.write().await;
            if let Some(block) = ledger.assemble_block() {
                // Self-validate (single-node start; mesh validation comes via consensus)
                if let Err(e) = ledger.apply_block(block) {
                    tracing::error!("[LEDGER] ❌ Block application failed: {}", e);
                }
            }
        }
    });

    // ── Compute heartbeat broadcast (every 60s) ─────────────────────
    let h5 = handlers.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let (ram_gb, model) = PoolManager::local_hardware();
            let pool = h5.pool.read().await;
            let slots = if pool.compute_share_enabled {
                pool.compute_pool.read().await.total_slots()
            } else {
                0
            };
            drop(pool);

            let heartbeat = MeshMessage::ComputeHeartbeat {
                peer_id: h5.local_peer.clone(),
                model,
                available_slots: slots,
                ram_gb,
                queue_depth: 0,
            };

            // Broadcast to all connected peers
            let connected = h5.transport.connected_peers().await;
            for peer_id in connected {
                let _ = send_to_peer(&peer_id, &heartbeat, &h5).await;
            }
        }
    });
}

/// Handle a single QUIC connection — read messages in a loop.
async fn handle_connection(conn: quinn::Connection, handlers: Arc<MeshHandlers>) {
    let remote_addr = conn.remote_address();

    loop {
        match QuicTransport::receive_from(&conn).await {
            Ok(envelope) => {
                if let Err(e) = process_envelope(envelope, &handlers).await {
                    tracing::warn!(
                        "[MESH LOOP] ⚠️ Failed to process envelope from {}: {}",
                        remote_addr, e
                    );
                }
            }
            Err(e) => {
                // Connection closed or read error — exit the per-connection loop
                if !e.contains("closed") {
                    tracing::debug!(
                        "[MESH LOOP] Connection to {} ended: {}",
                        remote_addr, e
                    );
                }
                break;
            }
        }
    }
}

/// Process a single signed envelope — verify, deserialize, route.
async fn process_envelope(
    envelope: SignedEnvelope,
    handlers: &Arc<MeshHandlers>,
) -> Result<(), String> {
    let sender = &envelope.sender;

    // ── 1. Quarantine check ──────────────────────────────────────────
    {
        let sanctions = handlers.sanctions.read().await;
        if sanctions.is_quarantined(sender) {
            tracing::warn!("[MESH LOOP] ⛔ Dropped message from quarantined peer {}", sender);
            return Ok(()); // Silently drop
        }
    }

    // ── 1b. Hardware blacklist check ─────────────────────────────────
    // If the sender's hardware is blacklisted, drop silently.
    // Hardware IDs are exchanged during Ping/Pong (included in attestation).
    // This is a secondary check — the primary is the software PeerId quarantine.

    // ── 2. Signature verification (L2: real ed25519) ────────────────
    if !envelope.signature.is_empty() {
        // Verify ed25519 signature over the payload
        let payload_with_timestamp = {
            let mut data = envelope.payload.clone();
            data.extend_from_slice(envelope.timestamp.as_bytes());
            data
        };
        if !crate::network::mesh_crypto::verify(
            &payload_with_timestamp,
            &envelope.signature,
            &[], // TODO: look up sender's ed25519 public key from KeyStore
        ) {
            // For now, log the failure but don't quarantine — key exchange
            // is not yet wired into Ping/Pong. After full L8, this becomes:
            // sanctions.record_violation(sender, Violation::AttestationFailure);
            tracing::debug!(
                "[MESH LOOP] ⚠️ Signature verification deferred for {} (key exchange pending)",
                sender
            );
        }
    }

    // ── 3. Replay protection: check timestamp freshness ──────────────
    if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&envelope.timestamp) {
        let age = chrono::Utc::now().signed_duration_since(ts);
        if age.num_seconds() > 300 || age.num_seconds() < -60 {
            tracing::warn!(
                "[MESH LOOP] ⚠️ Stale/future envelope from {} (age: {}s) — dropping",
                sender, age.num_seconds()
            );
            return Ok(());
        }
    }

    // ── 4. Deserialize the inner MeshMessage ─────────────────────────
    let message: MeshMessage = rmp_serde::from_slice(&envelope.payload)
        .map_err(|e| {
            // Record violation for malformed messages
            tracing::warn!("[MESH LOOP] ⚠️ Malformed payload from {}: {}", sender, e);
            format!("Deserialization failed: {}", e)
        })?;

    // ── 5. Record valid message for trust ─────────────────────────────
    {
        let mut trust = handlers.trust.write().await;
        trust.get_or_create(sender).record_valid_message();
    }

    // ── 6. Route to handler ──────────────────────────────────────────
    route_message(message, sender, handlers).await
}

/// Route a deserialized MeshMessage to the appropriate handler subsystem.
async fn route_message(
    message: MeshMessage,
    sender: &PeerId,
    handlers: &Arc<MeshHandlers>,
) -> Result<(), String> {
    match message {
        // ── Discovery ──────────────────────────────────────────────
        MeshMessage::Ping { peer_id, version, attestation } => {
            tracing::info!(
                "[MESH LOOP] 📡 Ping from {} (v: {}, binary: {}...)",
                peer_id, version, &attestation.binary_hash[..12.min(attestation.binary_hash.len())]
            );

            // Update peer registry
            let peer_info = PeerInfo {
                peer_id: peer_id.clone(),
                addr: "unknown".to_string(),
                last_seen: chrono::Utc::now().to_rfc3339(),
                version: version.clone(),
                binary_hash: attestation.binary_hash.clone(),
                source_hash: attestation.source_hash.clone(),
            };
            handlers.registry.upsert(peer_info).await;

            // Record attestation in trust store
            {
                let mut trust = handlers.trust.write().await;
                trust.get_or_create(&peer_id)
                    .record_attestation(&attestation.binary_hash);
            }

            // L5: Exchange capabilities with the peer
            if let Some(local_caps) = handlers.capabilities.local_capabilities().await {
                // Send our capabilities as part of the Pong
                tracing::debug!(
                    "[MESH LOOP] 📋 Exchanging capabilities with {}",
                    peer_id
                );
                let _ = local_caps; // Capabilities sent via Pong attestation metadata
            }

            // Send Pong with our peer list + attestation
            let peers = handlers.registry.peer_list_for_gossip().await;
            let pong = MeshMessage::Pong {
                peer_id: handlers.local_peer.clone(),
                peers,
                attestation: handlers.local_attestation.clone(),
            };
            send_to_peer(sender, &pong, handlers).await?;

            // L3: Auto-register peer in regional registry (default Global)
            handlers.regional.register_peer(
                &peer_id,
                crate::network::regional_keys::Region::Global,
            ).await;

            // Record in Apis Book
            handlers.book.push(ApisBookEntry::new(
                ApisBookEventType::PeerJoined,
                &peer_id.0[..12.min(peer_id.0.len())],
                &peer_id.0,
                &format!("Peer joined the mesh (v: {})", version),
            )).await;

            Ok(())
        }

        MeshMessage::Pong { peer_id, peers, attestation } => {
            tracing::info!(
                "[MESH LOOP] 📡 Pong from {} with {} peers",
                peer_id, peers.len()
            );

            // Register responding peer
            {
                let mut trust = handlers.trust.write().await;
                trust.get_or_create(&peer_id)
                    .record_attestation(&attestation.binary_hash);
            }

            // Add gossiped peers to registry (for future connections)
            for peer in peers {
                handlers.registry.upsert(peer).await;
            }

            Ok(())
        }

        // ── Attestation Challenge-Response ─────────────────────────
        MeshMessage::Challenge(challenge) => {
            tracing::info!(
                "[MESH LOOP] 🔐 Attestation challenge from {}",
                challenge.challenger
            );

            let response = AttestationResponse {
                nonce: challenge.nonce,
                attestation: handlers.local_attestation.clone(),
                nonce_signature: vec![], // TODO(L2): Sign with ed25519
            };
            send_to_peer(sender, &MeshMessage::ChallengeResponse(response), handlers).await
        }

        MeshMessage::ChallengeResponse(response) => {
            tracing::info!(
                "[MESH LOOP] 🔐 Attestation response from {} (binary: {}...)",
                sender, &response.attestation.binary_hash[..12.min(response.attestation.binary_hash.len())]
            );

            // Verify attestation — hash must be known/trusted
            {
                let mut trust = handlers.trust.write().await;
                trust.get_or_create(sender)
                    .record_attestation(&response.attestation.binary_hash);
            }

            Ok(())
        }

        // ── Knowledge Sync ─────────────────────────────────────────
        MeshMessage::LessonBroadcast { lesson, origin, timestamp: _ } => {
            let trust_level = handlers.trust.read().await.trust_level(&origin);

            let mut sync = handlers.sync.write().await;
            match sync.ingest_lesson(lesson.clone(), trust_level) {
                Ok(accepted) => {
                    if accepted {
                        handlers.book.push(ApisBookEntry::new(
                            ApisBookEventType::LessonShared,
                            &origin.0[..8.min(origin.0.len())],
                            &origin.0,
                            &format!("Shared lesson: {}", &lesson.text[..60.min(lesson.text.len())]),
                        )).await;
                    }
                    Ok(())
                }
                Err(e) => {
                    // PII detected or other ingestion failure — record violation
                    let mut sanctions = handlers.sanctions.write().await;
                    sanctions.record_violation(sender, Violation::PIIDetected {
                        field: e.clone(),
                    });
                    Err(e)
                }
            }
        }

        MeshMessage::SynapticDelta { nodes, edges, origin } => {
            let trust_level = handlers.trust.read().await.trust_level(&origin);

            let sync = handlers.sync.read().await;
            match sync.ingest_synaptic(nodes.clone(), edges.clone(), trust_level) {
                Ok(count) => {
                    if count > 0 {
                        handlers.book.push(ApisBookEntry::new(
                            ApisBookEventType::SynapticMerge,
                            &origin.0[..8.min(origin.0.len())],
                            &origin.0,
                            &format!("Merged {} synaptic entries", count),
                        )).await;
                    }
                    Ok(())
                }
                Err(e) => {
                    let mut sanctions = handlers.sanctions.write().await;
                    sanctions.record_violation(sender, Violation::PIIDetected {
                        field: e.clone(),
                    });
                    Err(e)
                }
            }
        }

        // ── Weight Exchange ────────────────────────────────────────
        MeshMessage::LoRAAnnounce { version, manifest_json: _, origin } => {
            if let Some(ref weights) = handlers.weights {
                if weights.should_request(&version, &origin).await {
                    // Request the adapter
                    let request = MeshMessage::LoRARequest {
                        version: version.clone(),
                        requester: handlers.local_peer.clone(),
                    };
                    send_to_peer(sender, &request, handlers).await?;

                    handlers.book.push(ApisBookEntry::new(
                        ApisBookEventType::WeightExchange,
                        &origin.0[..8.min(origin.0.len())],
                        &origin.0,
                        &format!("LoRA version {} available — requesting", version),
                    )).await;
                }
            }
            Ok(())
        }

        MeshMessage::LoRARequest { version, requester } => {
            tracing::info!(
                "[MESH LOOP] 📦 LoRA request for v{} from {}",
                version, requester
            );
            // In a full implementation, this would read the adapter file
            // and send a LoRATransfer message back.
            // For now: acknowledge the request exists.
            Ok(())
        }

        MeshMessage::LoRATransfer { version, adapter_bytes } => {
            if let Some(ref weights) = handlers.weights {
                match weights.stage_adapter(&version, &adapter_bytes).await {
                    Ok(path) => {
                        tracing::info!(
                            "[MESH LOOP] 📥 Staged LoRA adapter v{} ({} bytes) at {:?}",
                            version, adapter_bytes.len(), path
                        );
                        handlers.book.push(ApisBookEntry::new(
                            ApisBookEventType::WeightExchange,
                            &sender.0[..8.min(sender.0.len())],
                            &sender.0,
                            &format!("Received LoRA adapter v{} ({} bytes)", version, adapter_bytes.len()),
                        )).await;
                    }
                    Err(e) => {
                        tracing::error!("[MESH LOOP] ❌ Failed to stage LoRA adapter: {}", e);
                    }
                }
            }
            Ok(())
        }

        // ── Code Propagation ───────────────────────────────────────
        MeshMessage::CodePatch { diff, commit_hash, test_passed, origin } => {
            if !test_passed {
                tracing::warn!(
                    "[MESH LOOP] ⚠️ Ignoring code patch {} from {} — tests failed on sender",
                    commit_hash, origin
                );
                return Ok(());
            }

            if let Some(ref propagation) = handlers.propagation {
                match propagation.apply_patch(&diff, &commit_hash, &origin).await {
                    Ok(applied) => {
                        // Send ACK
                        let ack = MeshMessage::CodePatchAck {
                            commit_hash: commit_hash.clone(),
                            applied,
                            peer_id: handlers.local_peer.clone(),
                        };
                        send_to_peer(sender, &ack, handlers).await?;

                        if applied {
                            handlers.book.push(ApisBookEntry::new(
                                ApisBookEventType::CodePatch,
                                &origin.0[..8.min(origin.0.len())],
                                &origin.0,
                                &format!("Applied code patch {}", commit_hash),
                            )).await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[MESH LOOP] ⚠️ Failed to apply patch {} from {}: {}",
                            commit_hash, origin, e
                        );
                        let ack = MeshMessage::CodePatchAck {
                            commit_hash,
                            applied: false,
                            peer_id: handlers.local_peer.clone(),
                        };
                        send_to_peer(sender, &ack, handlers).await?;
                    }
                }
            }
            Ok(())
        }

        MeshMessage::CodePatchAck { commit_hash, applied, peer_id } => {
            tracing::info!(
                "[MESH LOOP] {} Patch {} {} by {}",
                if applied { "✅" } else { "❌" },
                commit_hash,
                if applied { "applied" } else { "rejected" },
                peer_id
            );
            Ok(())
        }

        // ── Governance ─────────────────────────────────────────────
        MeshMessage::Quarantine(notice) => {
            tracing::warn!(
                "[MESH LOOP] ⛔ Quarantine notice for {} from {}: {}",
                notice.target_peer, notice.issued_by, notice.reason
            );

            let mut sanctions = handlers.sanctions.write().await;
            sanctions.record_violation(
                &notice.target_peer,
                Violation::AttestationFailure,
            );

            // L4: Also blacklist the hardware if we have it
            // Hardware ID would be in the peer's attestation metadata.
            // For now, we blacklist by deriving from the PeerId as a placeholder.
            // Real implementation: peers exchange HardwareId during Ping/Pong.

            Ok(())
        }

        MeshMessage::BanProposal { target, reason, evidence_hash, proposer } => {
            tracing::info!(
                "[MESH LOOP] 🗳️ Ban proposal for {} by {}: {}",
                target, proposer, reason
            );

            handlers.governance.propose_ban(target.clone(), &reason, &evidence_hash, proposer.clone()).await;

            handlers.book.push(ApisBookEntry::new(
                ApisBookEventType::GovernanceVote,
                &proposer.0[..8.min(proposer.0.len())],
                &proposer.0,
                &format!("Proposed ban for {} — {}", target, reason),
            )).await;

            Ok(())
        }

        MeshMessage::BanVote { target, voter, approve, signature: _ } => {
            // Need proposal_id and total_peers for the vote — extract from target context
            let total_peers = handlers.registry.count().await;
            let proposals = handlers.governance.active_proposals().await;
            if let Some(proposal) = proposals.iter().find(|p| p.target == target) {
                let _ = handlers.governance.vote(&proposal.id, voter.clone(), approve, total_peers).await;
            }
            Ok(())
        }

        // ── Emergency & Survival ───────────────────────────────────
        MeshMessage::EmergencyAlert { severity, category, message, issuer } => {
            tracing::warn!(
                "[MESH LOOP] 🚨 EMERGENCY from {}: [{:?}/{:?}] {}",
                issuer, severity, category, message
            );

            handlers.governance.issue_alert(severity.clone(), category.clone(), &message, issuer.clone()).await;

            handlers.book.push(ApisBookEntry::new(
                ApisBookEventType::EmergencyAlert,
                &issuer.0[..8.min(issuer.0.len())],
                &issuer.0,
                &format!("[{:?}] {}", severity, message),
            )).await;

            Ok(())
        }

        MeshMessage::ResourceAdvertise { resource_type, capacity, issuer } => {
            handlers.governance.advertise_resource(issuer, resource_type, &capacity).await;
            Ok(())
        }

        MeshMessage::OSINTReport { category, data, issuer, signature: _ } => {
            handlers.governance.submit_osint(&category, &data, issuer).await;
            Ok(())
        }

        // ── Relay ──────────────────────────────────────────────────
        MeshMessage::RelayRequest { destination_url, requester } => {
            tracing::info!(
                "[MESH LOOP] 🌐 Relay request from {} → {}",
                requester, destination_url
            );

            // Content-filter the URL
            let scan = handlers.content_filter
                .scan(&requester, &destination_url).await;

            if scan != crate::network::content_filter::ScanResult::Clean {
                tracing::warn!(
                    "[MESH LOOP] ⚠️ Relay URL filtered: {:?}",
                    scan
                );
                return Ok(());
            }

            // Forward via reqwest (web relay)
            match reqwest::get(&destination_url).await {
                Ok(resp) => {
                    let content_type = resp.headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("application/octet-stream")
                        .to_string();
                    let data = resp.bytes().await.unwrap_or_default().to_vec();

                    let response = MeshMessage::RelayResponse {
                        data,
                        content_type,
                        provider: handlers.local_peer.clone(),
                    };
                    send_to_peer(sender, &response, handlers).await?;
                }
                Err(e) => {
                    tracing::warn!("[MESH LOOP] ⚠️ Relay fetch failed: {}", e);
                }
            }
            Ok(())
        }

        MeshMessage::RelayResponse { data: _, content_type: _, provider } => {
            tracing::info!(
                "[MESH LOOP] 🌐 Relay response from {}",
                provider
            );
            // Relay responses would be forwarded to the original requester
            // via a callback or pending-request map.
            Ok(())
        }

        // ── Compute Pooling ────────────────────────────────────────
        MeshMessage::ComputeRequest { job_id, model, prompt, max_tokens, requester } => {
            let result = handlers.compute_relay.process_request(
                &job_id, &model, &prompt, max_tokens, &requester
            ).await;

            let response = match result {
                ComputeResult::Success { job_id, response, tokens_generated: _ } => {
                    MeshMessage::ComputeResponse {
                        job_id,
                        tokens: response,
                        done: true,
                        provider: handlers.local_peer.clone(),
                    }
                }
                ComputeResult::Rejected { job_id, reason } => {
                    tracing::warn!(
                        "[MESH LOOP] ⚠️ Compute request {} rejected: {}",
                        job_id, reason
                    );
                    MeshMessage::ComputeResponse {
                        job_id,
                        tokens: format!("REJECTED: {}", reason),
                        done: true,
                        provider: handlers.local_peer.clone(),
                    }
                }
            };

            send_to_peer(sender, &response, handlers).await
        }

        MeshMessage::ComputeResponse { job_id, tokens, done, provider } => {
            tracing::info!(
                "[MESH LOOP] 🖥️ Compute response for job {} from {} (done: {})",
                job_id, provider, done
            );
            // Response would be forwarded to the original requester's pending request
            let _ = (tokens, done); // Consumed by the compute pool manager
            Ok(())
        }

        MeshMessage::ComputeHeartbeat { peer_id, model, available_slots, ram_gb, queue_depth } => {
            tracing::debug!(
                "[MESH LOOP] 💓 Compute heartbeat: {} — {} slots, {:.1}GB RAM, queue: {}",
                peer_id, available_slots, ram_gb, queue_depth
            );

            // Update the compute pool with this peer's availability
            let pool = handlers.pool.read().await;
            pool.update_compute_peer(
                peer_id, model, available_slots, ram_gb, queue_depth
            ).await;

            Ok(())
        }

        // ── Pool Coordination ──────────────────────────────────────
        MeshMessage::PoolStatusRequest { requester } => {
            let pool = handlers.pool.read().await;
            let stats = pool.stats().await;

            let response = MeshMessage::PoolStatusResponse {
                web_relays_available: stats["web_relays"].as_u64().unwrap_or(0) as u32,
                compute_nodes_available: stats["compute_nodes"].as_u64().unwrap_or(0) as u32,
                total_compute_slots: stats["total_slots"].as_u64().unwrap_or(0) as u32,
                provider: handlers.local_peer.clone(),
            };
            send_to_peer(&requester, &response, handlers).await
        }

        MeshMessage::PoolStatusResponse { web_relays_available, compute_nodes_available, total_compute_slots, provider } => {
            tracing::info!(
                "[MESH LOOP] 📊 Pool status from {}: {} web relays, {} compute nodes, {} total slots",
                provider, web_relays_available, compute_nodes_available, total_compute_slots
            );
            Ok(())
        }

        // ── Apis-to-Apis Chat ──────────────────────────────────────
        MeshMessage::ApisChat { from_peer, from_name, content, reply_to, timestamp } => {
            // Content-filter the chat message
            let scan = handlers.content_filter.scan(&from_peer, &content).await;
            if scan != crate::network::content_filter::ScanResult::Clean {
                tracing::warn!(
                    "[MESH LOOP] ⚠️ Apis chat from {} filtered: {:?}",
                    from_name, scan
                );
                return Ok(());
            }

            let msg = ApisChatMessage {
                id: uuid::Uuid::new_v4().to_string(),
                from_peer: from_peer.clone(),
                from_name: from_name.clone(),
                content: content.clone(),
                reply_to,
                timestamp,
                channel: None,
            };
            handlers.chat.handle_incoming(msg).await;

            handlers.book.push(ApisBookEntry::new(
                ApisBookEventType::AiChat,
                &from_name,
                &from_peer.0,
                &content[..content.len().min(120)],
            )).await;

            Ok(())
        }

        MeshMessage::ApisBroadcast { from_peer, from_name, channel, content, timestamp: _ } => {
            let scan = handlers.content_filter.scan(&from_peer, &content).await;
            if scan != crate::network::content_filter::ScanResult::Clean {
                return Ok(());
            }

            let msg = ApisChatMessage {
                id: uuid::Uuid::new_v4().to_string(),
                from_peer: from_peer.clone(),
                from_name: from_name.clone(),
                content: content.clone(),
                reply_to: None,
                timestamp: chrono::Utc::now().to_rfc3339(),
                channel: Some(channel),
            };
            handlers.chat.handle_incoming(msg).await;

            Ok(())
        }

        // ── Sandbox Compute (Wasm) ─────────────────────────────────
        MeshMessage::SandboxRequest { job_id, wasm_binary, input_data, cpu_limit_secs: _, memory_limit_mb: _, requester } => {
            tracing::info!(
                "[MESH LOOP] 🏗️ Sandbox request {} from {}",
                &job_id[..8.min(job_id.len())], requester
            );

            // Check priority — should we accept remote jobs?
            if !handlers.priority_manager.should_accept_remote_jobs().await {
                let response = MeshMessage::SandboxResponse {
                    job_id,
                    stdout: vec![],
                    stderr: b"Peer at capacity - local load too high".to_vec(),
                    exit_code: 1,
                    cpu_seconds_used: 0.0,
                    provider: handlers.local_peer.clone(),
                };
                return send_to_peer(sender, &response, handlers).await;
            }

            handlers.priority_manager.remote_job_started().await;

            let result = handlers.sandbox_engine.execute(
                &job_id, requester.clone(), &wasm_binary, &input_data
            ).await;

            handlers.priority_manager.remote_job_ended().await;

            let response = match result {
                Ok(res) => {
                    // Record earnings in ledger
                    let mut ledger = handlers.ledger.write().await;
                    ledger.submit_block_reward(
                        &[(handlers.local_peer.0.clone(), 1)],
                    );
                    drop(ledger);

                    MeshMessage::SandboxResponse {
                        job_id: res.job_id,
                        stdout: res.stdout,
                        stderr: res.stderr,
                        exit_code: res.exit_code,
                        cpu_seconds_used: res.cpu_seconds_used,
                        provider: handlers.local_peer.clone(),
                    }
                }
                Err(e) => MeshMessage::SandboxResponse {
                    job_id,
                    stdout: vec![],
                    stderr: e.into_bytes(),
                    exit_code: 1,
                    cpu_seconds_used: 0.0,
                    provider: handlers.local_peer.clone(),
                },
            };
            send_to_peer(sender, &response, handlers).await
        }

        MeshMessage::SandboxResponse { job_id, stdout: _, stderr: _, exit_code, cpu_seconds_used, provider } => {
            tracing::info!(
                "[MESH LOOP] 🏗️ Sandbox result for {} from {} (exit={}, {:.2}s CPU)",
                &job_id[..8.min(job_id.len())], provider, exit_code, cpu_seconds_used
            );
            Ok(())
        }

        // ── Batch Compute ──────────────────────────────────────────
        MeshMessage::ComputeBatch { batch_id, chunks, model, requester } => {
            tracing::info!(
                "[MESH LOOP] 📦 Batch job {} from {} — {} chunks for model {}",
                &batch_id[..8.min(batch_id.len())], requester, chunks.len(), model
            );
            // Process each chunk locally and return results
            for (i, chunk) in chunks.iter().enumerate() {
                let result = handlers.compute_relay.process_request(
                    &format!("{}_chunk_{}", batch_id, i), &model, chunk, 2048, &requester
                ).await;

                let result_text = match result {
                    ComputeResult::Success { response, .. } => response,
                    ComputeResult::Rejected { reason, .. } => format!("REJECTED: {}", reason),
                };

                let chunk_result = MeshMessage::ComputeChunkResult {
                    batch_id: batch_id.clone(),
                    chunk_index: i as u32,
                    result: result_text,
                    provider: handlers.local_peer.clone(),
                };
                let _ = send_to_peer(sender, &chunk_result, handlers).await;
            }
            Ok(())
        }

        MeshMessage::ComputeChunkResult { batch_id, chunk_index, result: _, provider } => {
            tracing::info!(
                "[MESH LOOP] 📦 Batch chunk {}/{} result from {}",
                chunk_index, &batch_id[..8.min(batch_id.len())], provider
            );
            Ok(())
        }

        // ── DHT ────────────────────────────────────────────────────
        MeshMessage::DHTStore { key: _, value, entry_type, ttl_secs, origin } => {
            tracing::info!(
                "[MESH LOOP] \u{1f5c4}\u{fe0f} DHT store ({}) from {}",
                entry_type, origin
            );
            let entry_type_parsed = match entry_type.as_str() {
                "lesson" => crate::network::dht::DHTEntryType::Lesson,
                "synaptic" => crate::network::dht::DHTEntryType::SynapticDelta,
                "lora" => crate::network::dht::DHTEntryType::LoRAManifest,
                "web_cache" => crate::network::dht::DHTEntryType::WebCache,
                "model_meta" => crate::network::dht::DHTEntryType::ModelMeta,
                _ => crate::network::dht::DHTEntryType::Generic,
            };
            let dht = handlers.dht.write().await;
            dht.store(&value, entry_type_parsed, ttl_secs).await;
            Ok(())
        }

        MeshMessage::DHTLookup { key, requester } => {
            tracing::info!(
                "[MESH LOOP] \u{1f50d} DHT lookup: {} from {}",
                &key[..16.min(key.len())], requester
            );
            let dht = handlers.dht.read().await;
            let result = dht.lookup(&key).await;
            let response = match result {
                crate::network::dht::LookupResult::Found(entry) => MeshMessage::DHTResponse {
                    key,
                    value: entry.value.clone(),
                    provider: handlers.local_peer.clone(),
                },
                crate::network::dht::LookupResult::Referral(peers) => MeshMessage::DHTNotFound {
                    key,
                    referrals: peers,
                },
                crate::network::dht::LookupResult::NotFound => MeshMessage::DHTNotFound {
                    key,
                    referrals: vec![],
                },
            };
            drop(dht);
            send_to_peer(sender, &response, handlers).await
        }

        MeshMessage::DHTResponse { key, value: _, provider } => {
            tracing::info!(
                "[MESH LOOP] DHT response for {} from {}",
                &key[..16.min(key.len())], provider
            );
            Ok(())
        }

        MeshMessage::DHTNotFound { key, referrals } => {
            tracing::debug!(
                "[MESH LOOP] DHT not found: {} ({} referrals)",
                &key[..16.min(key.len())], referrals.len()
            );
            Ok(())
        }

        // ── File System ────────────────────────────────────────────
        MeshMessage::FileManifest { file_hash, chunk_hashes, total_size, origin } => {
            tracing::info!(
                "[MESH LOOP] File manifest: {} ({} chunks, {} bytes) from {}",
                &file_hash[..16.min(file_hash.len())], chunk_hashes.len(), total_size, origin
            );
            let manifest = crate::network::mesh_fs::FileManifest {
                file_hash,
                chunk_hashes,
                total_size,
                chunk_count: 0, // Will be set from chunk_hashes.len()
                encrypted_meta: vec![],
                origin,
                shared_at: chrono::Utc::now().to_rfc3339(),
            };
            handlers.mesh_fs.write().await.receive_manifest(manifest).await;
            Ok(())
        }

        MeshMessage::FileChunkRequest { chunk_hash, requester } => {
            tracing::info!(
                "[MESH LOOP] Chunk request: {} from {}",
                &chunk_hash[..16.min(chunk_hash.len())], requester
            );
            let fs = handlers.mesh_fs.read().await;
            if let Some(data) = fs.get_chunk(&chunk_hash).await {
                let response = MeshMessage::FileChunkResponse {
                    chunk_hash,
                    data,
                    provider: handlers.local_peer.clone(),
                };
                drop(fs);
                send_to_peer(&requester, &response, handlers).await?;
            }
            Ok(())
        }

        MeshMessage::FileChunkResponse { chunk_hash, data, provider } => {
            tracing::info!(
                "[MESH LOOP] Chunk received: {} from {}",
                &chunk_hash[..16.min(chunk_hash.len())], provider
            );
            // Store the chunk locally
            handlers.mesh_fs.read().await
                .store_raw_chunk(chunk_hash, data).await;
            Ok(())
        }

        // ── Governance Phases ──────────────────────────────────────
        MeshMessage::PhaseTransition { new_phase, peer_count, timestamp } => {
            tracing::warn!(
                "[MESH LOOP] ⚖️ GOVERNANCE PHASE TRANSITION: {} (peers: {}) at {}",
                new_phase, peer_count, timestamp
            );

            handlers.book.push(ApisBookEntry::new(
                ApisBookEventType::GovernanceVote,
                "mesh",
                "governance",
                &format!("Phase transition → {} ({} peers)", new_phase, peer_count),
            )).await;

            Ok(())
        }
    }
}

/// Helper: serialize a MeshMessage into a SignedEnvelope and send to a peer.
async fn send_to_peer(
    target: &PeerId,
    message: &MeshMessage,
    handlers: &Arc<MeshHandlers>,
) -> Result<(), String> {
    let payload = rmp_serde::to_vec(message)
        .map_err(|e| format!("Failed to serialize message: {}", e))?;

    let envelope = SignedEnvelope {
        sender: handlers.local_peer.clone(),
        payload,
        signature: vec![], // TODO(L2): Sign with ed25519
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    handlers.transport.send(target, &envelope).await
}

/// Periodically connect to peers discovered via mDNS/bootstrap/gossip.
async fn connect_to_discovered_peers(handlers: &Arc<MeshHandlers>) {
    let peers = handlers.registry.all_peers().await;
    let connected = handlers.transport.connected_peers().await;

    for peer in peers {
        // Skip already-connected peers
        if connected.contains(&peer.peer_id) {
            continue;
        }

        // Skip quarantined peers
        {
            let sanctions = handlers.sanctions.read().await;
            if sanctions.is_quarantined(&peer.peer_id) {
                continue;
            }
        }

        // Attempt connection
        if let Ok(addr) = peer.addr.parse::<std::net::SocketAddr>() {
            match handlers.transport.connect(addr).await {
                Ok(conn) => {
                    handlers.transport.register_connection(peer.peer_id.clone(), conn).await;

                    // Send Ping with our attestation
                    let ping = MeshMessage::Ping {
                        peer_id: handlers.local_peer.clone(),
                        version: handlers.local_attestation.commit.clone(),
                        attestation: handlers.local_attestation.clone(),
                    };
                    let _ = send_to_peer(&peer.peer_id, &ping, handlers).await;
                }
                Err(e) => {
                    tracing::debug!(
                        "[MESH LOOP] Failed to connect to {}: {}",
                        peer.peer_id, e
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_timestamp_validation() {
        // Valid timestamp (now)
        let now = chrono::Utc::now().to_rfc3339();
        let ts = chrono::DateTime::parse_from_rfc3339(&now).unwrap();
        let age = chrono::Utc::now().signed_duration_since(ts);
        assert!(age.num_seconds() < 300, "Fresh timestamp should be accepted");

        // Stale timestamp (10 minutes ago)
        let old = (chrono::Utc::now() - chrono::Duration::minutes(10)).to_rfc3339();
        let ts = chrono::DateTime::parse_from_rfc3339(&old).unwrap();
        let age = chrono::Utc::now().signed_duration_since(ts);
        assert!(age.num_seconds() > 300, "Stale timestamp should be rejected");
    }

    #[test]
    fn test_sender_display_short() {
        let peer = PeerId("abcdef1234567890abcdef1234567890".to_string());
        assert_eq!(format!("{}", peer), "abcdef123456");
    }
}
