/// QUIC Transport Layer — Encrypted P2P communication for the SafeNet mesh.
///
/// Uses quinn (Rust QUIC) with self-signed certificates derived from the peer's
/// identity. All traffic is TLS 1.3 encrypted. No external CA required.
///
/// SURVIVABILITY: Works over any IP network — internet, LAN, WiFi Direct,
/// even a direct ethernet cable between two machines.
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::network::messages::{PeerId, SignedEnvelope};

/// QUIC Transport — manages encrypted P2P connections.
pub struct QuicTransport {
    pub endpoint: quinn::Endpoint,
    pub peer_id: PeerId,
    connections: Arc<RwLock<HashMap<PeerId, quinn::Connection>>>,
    _cert_der: Vec<u8>,
}

impl QuicTransport {
    /// Create a QUIC endpoint bound to the given port.
    /// Generates a self-signed TLS certificate from the peer identity.
    pub fn bind(port: u16, peer_id: &PeerId) -> Result<Self, String> {
        // Generate self-signed cert from peer identity
        let subject_alt_names = vec![peer_id.0.clone()];
        let cert_params = rcgen::CertificateParams::new(subject_alt_names)
            .map_err(|e| format!("Failed to create cert params: {}", e))?;
        let key_pair = rcgen::KeyPair::generate()
            .map_err(|e| format!("Failed to generate key pair: {}", e))?;
        let cert = cert_params.self_signed(&key_pair)
            .map_err(|e| format!("Failed to self-sign cert: {}", e))?;

        let cert_der = cert.der().to_vec();
        let key_der = key_pair.serialize_der();

        // Build rustls server config (accepts any client cert — we verify via PeerId)
        let cert_chain = vec![rustls::pki_types::CertificateDer::from(cert_der.clone())];
        let private_key = rustls::pki_types::PrivateKeyDer::try_from(key_der)
            .map_err(|e| format!("Failed to parse private key: {}", e))?;

        let server_crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain.clone(), private_key.clone_key())
            .map_err(|e| format!("Failed to build server TLS config: {}", e))?;

        let server_config = quinn::ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
                .map_err(|e| format!("Failed to build QUIC server config: {}", e))?
        ));

        // Build client config (skip server cert verification — we verify via PeerId attestation)
        let client_crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth();

        let client_config = quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)
                .map_err(|e| format!("Failed to build QUIC client config: {}", e))?
        ));

        let mut endpoint = quinn::Endpoint::server(
            server_config,
            format!("0.0.0.0:{}", port).parse::<SocketAddr>()
                .map_err(|e| format!("Invalid bind address: {}", e))?
        ).map_err(|e| format!("Failed to bind QUIC endpoint on port {}: {}", port, e))?;

        endpoint.set_default_client_config(client_config);

        tracing::info!("[TRANSPORT] 🔒 QUIC endpoint bound on port {} (TLS 1.3, self-signed)", port);

        Ok(Self {
            endpoint,
            peer_id: peer_id.clone(),
            connections: Arc::new(RwLock::new(HashMap::new())),
            _cert_der: cert_der,
        })
    }

    /// Connect to a remote peer at the given address.
    pub async fn connect(&self, addr: SocketAddr) -> Result<quinn::Connection, String> {
        let connection = self.endpoint
            .connect(addr, "hive-peer")
            .map_err(|e| format!("Failed to initiate QUIC connection to {}: {}", addr, e))?
            .await
            .map_err(|e| format!("QUIC handshake failed with {}: {}", addr, e))?;

        tracing::info!("[TRANSPORT] 🔗 Connected to peer at {}", addr);
        Ok(connection)
    }

    /// Register a connection with a known peer ID.
    pub async fn register_connection(&self, peer_id: PeerId, conn: quinn::Connection) {
        self.connections.write().await.insert(peer_id, conn);
    }

    /// Send a signed envelope to a connected peer via a uni-directional stream.
    pub async fn send(&self, peer_id: &PeerId, envelope: &SignedEnvelope) -> Result<(), String> {
        let connections = self.connections.read().await;
        let conn = connections.get(peer_id)
            .ok_or_else(|| format!("No connection to peer {}", peer_id))?;

        let mut send = conn.open_uni().await
            .map_err(|e| format!("Failed to open stream to {}: {}", peer_id, e))?;

        let payload = rmp_serde::to_vec(envelope)
            .map_err(|e| format!("Failed to serialize envelope: {}", e))?;

        // Length-prefix the payload (4 bytes, big-endian)
        let len_bytes = (payload.len() as u32).to_be_bytes();
        send.write_all(&len_bytes).await
            .map_err(|e| format!("Failed to write length prefix: {}", e))?;
        send.write_all(&payload).await
            .map_err(|e| format!("Failed to write payload: {}", e))?;
        send.finish()
            .map_err(|e| format!("Failed to finish stream: {}", e))?;

        Ok(())
    }

    /// Broadcast a mesh message to all connected peers.
    pub async fn broadcast(&self, envelope: &SignedEnvelope) {
        let connections = self.connections.read().await;
        for (peer_id, _conn) in connections.iter() {
            if let Err(e) = self.send(peer_id, envelope).await {
                tracing::warn!("[TRANSPORT] ⚠️ Failed to send to {}: {}", peer_id, e);
            }
        }
    }

    /// Accept incoming uni-directional streams and deserialize envelopes.
    /// Call this in a loop for each accepted connection.
    pub async fn receive_from(conn: &quinn::Connection) -> Result<SignedEnvelope, String> {
        let mut recv = conn.accept_uni().await
            .map_err(|e| format!("Failed to accept stream: {}", e))?;

        // Read length prefix (4 bytes)
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await
            .map_err(|e| format!("Failed to read length prefix: {}", e))?;
        let len = u32::from_be_bytes(len_buf) as usize;

        // Sanity check — reject payloads over 200MB (MAX_ENVELOPE_SIZE)
        if len > crate::network::messages::MAX_ENVELOPE_SIZE {
            return Err(format!("Payload too large: {} bytes (max {})", len, crate::network::messages::MAX_ENVELOPE_SIZE));
        }

        let mut payload = vec![0u8; len];
        recv.read_exact(&mut payload).await
            .map_err(|e| format!("Failed to read payload: {}", e))?;

        rmp_serde::from_slice(&payload)
            .map_err(|e| format!("Failed to deserialize envelope: {}", e))
    }

    /// Get the number of active connections.
    pub async fn connection_count(&self) -> usize {
        self.connections.read().await.len()
    }

    /// Remove a disconnected peer.
    pub async fn remove_peer(&self, peer_id: &PeerId) {
        self.connections.write().await.remove(peer_id);
    }

    /// Get all connected peer IDs.
    pub async fn connected_peers(&self) -> Vec<PeerId> {
        self.connections.read().await.keys().cloned().collect()
    }

    /// Disconnect from all peers — used by config guard and self-destruct.
    pub async fn disconnect_all(&self) {
        let mut conns = self.connections.write().await;
        let count = conns.len();
        for (peer_id, conn) in conns.drain() {
            conn.close(0u32.into(), b"mesh_disconnect");
            tracing::info!("[TRANSPORT] 🔌 Disconnected from {}", peer_id);
        }
        if count > 0 {
            tracing::warn!("[TRANSPORT] ⚠️ Disconnected from all {} peers", count);
        }
    }
}

/// Custom certificate verifier that skips TLS server cert verification.
/// We verify peers via PeerId attestation at the mesh protocol layer instead.
#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // Peer identity is verified via PeerId attestation, not TLS certs
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_bind() {
        let peer_id = PeerId("test_transport_peer_12345678901234567890123456789012".to_string());
        // Use a random high port to avoid conflicts
        let port = 19473 + (std::process::id() % 1000) as u16;
        let transport = QuicTransport::bind(port, &peer_id);
        assert!(transport.is_ok(), "Transport should bind successfully: {:?}", transport.err());
    }

    #[tokio::test]
    async fn test_transport_connection_count() {
        let peer_id = PeerId("test_conn_count_peer_1234567890123456789012345678901".to_string());
        let port = 19500 + (std::process::id() % 1000) as u16;
        let transport = QuicTransport::bind(port, &peer_id).unwrap();
        assert_eq!(transport.connection_count().await, 0);
    }
}
