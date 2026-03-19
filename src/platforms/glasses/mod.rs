//! HIVE Glasses Platform — Native WebSocket bridge for Meta Ray-Ban smart glasses.
//!
//! Protocol (mirrors Ernos 3.0 glasses_handler for Android app compatibility):
//!
//!   Client → Server:
//!     Binary frames:  Raw PCM audio (16kHz, 16-bit, mono, 100ms chunks)
//!     JSON:           {"type": "frame", "jpeg": "<base64>"}
//!     JSON:           {"type": "end_of_speech"}
//!     JSON:           {"type": "ping"}
//!
//!   Server → Client:
//!     Binary frames:  Raw PCM audio response (24kHz, 16-bit, mono)
//!     JSON:           {"type": "text", "content": "..."}
//!     JSON:           {"type": "thinking"}
//!     JSON:           {"type": "done"}
//!     JSON:           {"type": "pong"}
//!     JSON:           {"type": "error", "message": "..."}

pub mod stt;
pub mod link;

use async_trait::async_trait;
use std::collections::VecDeque;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{StreamExt, SinkExt};

use crate::models::message::{Event, Response};
use crate::models::scope::Scope;
use crate::platforms::{Platform, PlatformError};

/// Maximum camera frames to keep in ring buffer per session.
const MAX_FRAME_BUFFER: usize = 3;

/// Glasses platform port — configurable via HIVE_GLASSES_PORT env var.
fn glasses_port() -> u16 {
    std::env::var("HIVE_GLASSES_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8422)
}

/// JWT auth token for glasses connections — matches HIVE_GLASSES_TOKEN env var.
fn glasses_token() -> String {
    std::env::var("HIVE_GLASSES_TOKEN").unwrap_or_default()
}

/// Session state for a single glasses WebSocket connection.
struct GlassesSession {
    user_id: String,
    username: String,
    audio_chunks: Vec<Vec<u8>>,
    frames: VecDeque<Vec<u8>>,
    is_processing: bool,
    turns_processed: usize,
}

impl GlassesSession {
    fn new(user_id: String, username: String) -> Self {
        Self {
            user_id,
            username,
            audio_chunks: Vec::new(),
            frames: VecDeque::with_capacity(MAX_FRAME_BUFFER),
            is_processing: false,
            turns_processed: 0,
        }
    }

    fn add_audio_chunk(&mut self, data: Vec<u8>) {
        self.audio_chunks.push(data);
    }

    fn add_frame(&mut self, jpeg_b64: &str) {
        if let Ok(bytes) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, jpeg_b64) {
            if self.frames.len() >= MAX_FRAME_BUFFER {
                self.frames.pop_front();
            }
            self.frames.push_back(bytes);
        }
    }

    fn has_audio(&self) -> bool {
        let total_bytes: usize = self.audio_chunks.iter().map(|c| c.len()).sum();
        // At least 500ms of audio at 16kHz, 16-bit, mono = 16000 bytes
        total_bytes >= 16000
    }

    fn take_audio(&mut self) -> Vec<u8> {
        let combined: Vec<u8> = self.audio_chunks.iter().flat_map(|c| c.iter().copied()).collect();
        self.audio_chunks.clear();
        combined
    }

    #[allow(dead_code)]
    fn latest_frame(&self) -> Option<&[u8]> {
        self.frames.back().map(|v| v.as_slice())
    }
}

// ──────────────────────────────────────────────────────────────────
// Connection Registry — maps platform_id → mpsc::Sender<String>
// ──────────────────────────────────────────────────────────────────

/// Per-connection response channel type.
type ResponseSender = mpsc::Sender<String>;

/// Active glasses connections — maps platform ID → response sender.
static GLASSES_CONNECTIONS: std::sync::LazyLock<
    tokio::sync::RwLock<std::collections::HashMap<String, ResponseSender>>
> = std::sync::LazyLock::new(|| tokio::sync::RwLock::new(std::collections::HashMap::new()));

/// Register a glasses connection for response delivery.
async fn register_connection(platform_id: &str, tx: ResponseSender) {
    GLASSES_CONNECTIONS.write().await.insert(platform_id.to_string(), tx);
    tracing::debug!("[GLASSES] 📝 Registered connection: {}", platform_id);
}

/// Remove a glasses connection.
async fn unregister_connection(platform_id: &str) {
    GLASSES_CONNECTIONS.write().await.remove(platform_id);
    tracing::debug!("[GLASSES] 🗑️ Unregistered connection: {}", platform_id);
}

// ──────────────────────────────────────────────────────────────────
// GlassesPlatform
// ──────────────────────────────────────────────────────────────────

pub struct GlassesPlatform;

impl GlassesPlatform {
    pub fn new() -> Self {
        Self
    }

    /// Validate a simple bearer token from the query string.
    #[allow(dead_code)] // Will be used when HIVE_GLASSES_TOKEN auth is enforced
    fn validate_token(query: &str) -> Option<(String, String)> {
        let expected = glasses_token();
        if expected.is_empty() {
            tracing::warn!("[GLASSES] ⚠️ No HIVE_GLASSES_TOKEN set — accepting all connections");
            return Some(("glasses_user".to_string(), "Glasses User".to_string()));
        }

        for param in query.split('&') {
            if let Some(token) = param.strip_prefix("token=") {
                if token == expected {
                    return Some(("glasses_user".to_string(), "Glasses User".to_string()));
                }
            }
        }
        None
    }
}

#[async_trait]
impl Platform for GlassesPlatform {
    fn name(&self) -> &str {
        "glasses"
    }

    async fn start(&self, event_sender: mpsc::Sender<Event>) -> Result<(), PlatformError> {
        let port = glasses_port();
        let addr = format!("0.0.0.0:{}", port);

        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            PlatformError::Other(format!("Failed to bind glasses WebSocket on {}: {}", addr, e))
        })?;

        tracing::info!("[GLASSES] 🕶️ WebSocket server listening on ws://{}", addr);

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        tracing::info!("[GLASSES] 🔌 New connection from {}", peer_addr);
                        let sender = event_sender.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, sender).await {
                                tracing::warn!("[GLASSES] Connection error from {}: {}", peer_addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("[GLASSES] Accept error: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    async fn send(&self, response: Response) -> Result<(), PlatformError> {
        // Route the response to the correct WebSocket connection via the registry.
        // The platform_id in the response matches the one registered by handle_connection.
        let platform_id = &response.platform;

        if response.is_telemetry {
            // Skip telemetry for glasses — the user gets voice, not text updates
            return Ok(());
        }

        if let Some(tx) = GLASSES_CONNECTIONS.read().await.get(platform_id) {
            if let Err(e) = tx.send(response.text).await {
                tracing::warn!("[GLASSES] Failed to route response to {}: {}", platform_id, e);
            }
        } else {
            tracing::warn!("[GLASSES] No active connection for platform_id: {}", platform_id);
        }
        Ok(())
    }
}

// ──────────────────────────────────────────────────────────────────
// Per-Connection Handler
// ──────────────────────────────────────────────────────────────────

/// Handle a single WebSocket connection from glasses.
async fn handle_connection(
    stream: tokio::net::TcpStream,
    event_sender: mpsc::Sender<Event>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let mut session = GlassesSession::new("glasses_user".to_string(), "Glasses User".to_string());

    // Per-connection response channel
    let (response_tx, mut response_rx): (ResponseSender, mpsc::Receiver<String>) = mpsc::channel(10);
    let connection_id = uuid::Uuid::new_v4().to_string();
    let platform_id = format!("glasses:{}:0:0", connection_id);
    register_connection(&platform_id, response_tx).await;

    // Send connected message
    let connected_msg = serde_json::json!({
        "type": "connected",
        "user": session.username,
        "message": "HIVE glasses bridge active. Say 'Hey Apis' to begin."
    });
    ws_sender.send(Message::Text(connected_msg.to_string().into())).await?;

    tracing::info!("[GLASSES] 🕶️ Session started (id={})", connection_id);

    // Main event loop — multiplexes WebSocket input and engine responses
    loop {
        tokio::select! {
            // ── Inbound: WebSocket messages from the glasses/phone ──
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        if !session.is_processing {
                            session.add_audio_chunk(data.to_vec());
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                            match data.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                                "link_request" => {
                                    if let Some(did) = data.get("discord_id").and_then(|v| v.as_str()) {
                                        match link::request_code_for_user(did, &platform_id).await {
                                            Ok(_) => {
                                                let ack = serde_json::json!({
                                                    "type": "link_requested",
                                                    "message": "Verification code sent via Discord DM!"
                                                });
                                                let _ = ws_sender.send(Message::Text(ack.to_string().into())).await;
                                            }
                                            Err(e) => {
                                                let err = serde_json::json!({
                                                    "type": "link_error",
                                                    "message": e
                                                });
                                                let _ = ws_sender.send(Message::Text(err.to_string().into())).await;
                                            }
                                        }
                                    }
                                }
                                "link_verify" => {
                                    if let Some(code) = data.get("code").and_then(|v| v.as_str()) {
                                        match link::verify_code_from_app(code).await {
                                            Ok(token) => {
                                                let ack = serde_json::json!({
                                                    "type": "link_success",
                                                    "device_token": token,
                                                    "message": "Account linked successfully!"
                                                });
                                                let _ = ws_sender.send(Message::Text(ack.to_string().into())).await;
                                            }
                                            Err(e) => {
                                                let err = serde_json::json!({
                                                    "type": "link_error",
                                                    "message": e
                                                });
                                                let _ = ws_sender.send(Message::Text(err.to_string().into())).await;
                                            }
                                        }
                                    }
                                }
                                "frame" => {
                                    if let Some(jpeg_b64) = data.get("jpeg").and_then(|v| v.as_str()) {
                                        session.add_frame(jpeg_b64);
                                    }
                                }
                                "authenticate" => {
                                    // App sends stored device_token to auto-link on reconnect
                                    if let Some(token) = data.get("device_token").and_then(|v| v.as_str()) {
                                        if let Some((uid, uname)) = link::authenticate_device(token, &platform_id).await {
                                            session.user_id = uid.clone();
                                            session.username = uname.clone();
                                            let ack = serde_json::json!({
                                                "type": "authenticated",
                                                "user": uname,
                                                "discord_id": uid,
                                            });
                                            let _ = ws_sender.send(Message::Text(ack.to_string().into())).await;
                                            tracing::info!("[GLASSES] 🔓 Auto-authenticated as {}", uname);
                                        } else {
                                            let nack = serde_json::json!({
                                                "type": "auth_failed",
                                                "message": "Device token not linked. Use /link <code> in Discord.",
                                            });
                                            let _ = ws_sender.send(Message::Text(nack.to_string().into())).await;
                                        }
                                    }
                                }
                                "unlink" => {
                                    if let Some(token) = data.get("device_token").and_then(|v| v.as_str()) {
                                        // Explicit logout — removes persistent link from disk
                                        link::unlink_device(token).await;
                                    }
                                    session.user_id = "glasses_user".to_string();
                                    session.username = "Glasses User".to_string();
                                    let ack = serde_json::json!({
                                        "type": "unlinked",
                                        "message": "Account unlinked. Generate a new link code to re-link.",
                                    });
                                    let _ = ws_sender.send(Message::Text(ack.to_string().into())).await;
                                    tracing::info!("[GLASSES] 🗑️ Device explicitly unlinked");
                                }
                                "end_of_speech" => {
                                    if session.has_audio() && !session.is_processing {
                                        session.is_processing = true;

                                        // Send thinking indicator
                                        let thinking_msg = serde_json::json!({"type": "thinking"});
                                        let _ = ws_sender.send(Message::Text(thinking_msg.to_string().into())).await;

                                        // Take audio and transcribe via STT
                                        let audio_data = session.take_audio();
                                        let transcribed_text = stt::transcribe_pcm(&audio_data).await;

                                        if transcribed_text.is_empty() {
                                            tracing::debug!("[GLASSES] Empty transcription — skipping");
                                            session.is_processing = false;
                                            continue;
                                        }

                                        tracing::info!("[GLASSES] 🕶️ [{}] STT: \"{}\"", session.username, transcribed_text);

                                        // Resolve identity: use Discord-linked user if available
                                        let (user_id, username, scope) = if let Some((discord_id, discord_name)) = link::get_linked_identity(&platform_id).await {
                                            // Linked to Discord — use their identity and private scope
                                            (discord_id.clone(), discord_name.clone(), Scope::Private {
                                                user_id: discord_id,
                                            })
                                        } else {
                                            // Anonymous — isolated scope per connection
                                            (session.user_id.clone(), session.username.clone(), Scope::Private {
                                                user_id: format!("anon:{}", connection_id),
                                            })
                                        };

                                        let mut final_content = transcribed_text.clone();
                                        let mut has_frame = false;
                                        if let Some(frame_b64) = session.latest_frame() {
                                            use base64::Engine;
                                            if let Ok(jpeg_bytes) = base64::engine::general_purpose::STANDARD.decode(frame_b64) {
                                                let frame_path = format!("/tmp/hive_glasses_{}.jpg", connection_id);
                                                if std::fs::write(&frame_path, jpeg_bytes).is_ok() {
                                                    final_content.push_str(&format!("\n\n[USER_ATTACHMENT]({})", frame_path));
                                                    has_frame = true;
                                                }
                                            }
                                        }

                                        if !has_frame {
                                            final_content.push_str("\n\n[SYSTEM NOTIFICATION TO AGENT: The user's continuous glasses camera feed is currently OFF, likely because the Android companion app is in the background. You CANNOT see right now. Remember you are STILL on the Wearable Glasses platform, NOT Discord. Use your voice-first persona.]");
                                        }

                                        // Create HIVE event with resolved identity
                                        let event = Event {
                                            platform: platform_id.clone(),
                                            scope,
                                            author_name: username,
                                            author_id: user_id,
                                            content: final_content,
                                        };

                                        if event_sender.send(event).await.is_err() {
                                            tracing::error!("[GLASSES] Engine event channel closed");
                                            break;
                                        }

                                        session.turns_processed += 1;
                                        // is_processing remains true until response arrives
                                    }
                                }
                                "message" => {
                                    // Text-based chat from the app (no STT needed)
                                    if let Some(content) = data.get("content").and_then(|v| v.as_str()) {
                                        let content = content.trim().to_string();
                                        if !content.is_empty() && !session.is_processing {
                                            session.is_processing = true;

                                            // Send thinking indicator
                                            let thinking_msg = serde_json::json!({"type": "thinking"});
                                            let _ = ws_sender.send(Message::Text(thinking_msg.to_string().into())).await;

                                            tracing::info!("[GLASSES] 💬 [{}] Text: \"{}\"", session.username, content);

                                            // Resolve identity
                                            let (user_id, username, scope) = if let Some((discord_id, discord_name)) = link::get_linked_identity(&platform_id).await {
                                                (discord_id.clone(), discord_name.clone(), Scope::Private {
                                                    user_id: discord_id,
                                                })
                                            } else {
                                                (session.user_id.clone(), session.username.clone(), Scope::Private {
                                                    user_id: format!("anon:{}", connection_id),
                                                })
                                            };

                                            let mut final_content = content.clone();
                                            let mut has_frame = false;
                                            if let Some(frame_b64) = session.latest_frame() {
                                                use base64::Engine;
                                                if let Ok(jpeg_bytes) = base64::engine::general_purpose::STANDARD.decode(frame_b64) {
                                                    let frame_path = format!("/tmp/hive_glasses_{}.jpg", connection_id);
                                                    if std::fs::write(&frame_path, jpeg_bytes).is_ok() {
                                                        final_content.push_str(&format!("\n\n[USER_ATTACHMENT]({})", frame_path));
                                                        has_frame = true;
                                                    }
                                                }
                                            }

                                            if !has_frame {
                                                final_content.push_str("\n\n[SYSTEM NOTIFICATION TO AGENT: The user's continuous glasses camera feed is currently OFF, likely because the Android companion app is in the background. You CANNOT see right now. Remember you are STILL on the Wearable Glasses platform, NOT Discord. Use your voice-first persona.]");
                                            }

                                            let event = Event {
                                                platform: platform_id.clone(),
                                                scope,
                                                author_name: username,
                                                author_id: user_id,
                                                content: final_content,
                                            };

                                            if event_sender.send(event).await.is_err() {
                                                tracing::error!("[GLASSES] Engine event channel closed");
                                                break;
                                            }

                                            session.turns_processed += 1;
                                        }
                                    }
                                }
                                "ping" => {
                                    let pong = serde_json::json!({"type": "pong"});
                                    let _ = ws_sender.send(Message::Text(pong.to_string().into())).await;
                                }
                                _ => {
                                    tracing::debug!("[GLASSES] Unknown message type: {}", data.get("type").and_then(|v| v.as_str()).unwrap_or("none"));
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!("[GLASSES] 🕶️ Disconnected: {} ({} turns)", session.username, session.turns_processed);
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::warn!("[GLASSES] WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // ── Outbound: Engine responses routed via send() → connection registry ──
            Some(response_text) = response_rx.recv() => {
                // Send text response to WebSocket client
                let text_msg = serde_json::json!({
                    "type": "text",
                    "content": &response_text,
                });
                let _ = ws_sender.send(Message::Text(text_msg.to_string().into())).await;

                // Stream TTS audio in real-time using Kokoro's native streaming
                let python_cmd = std::env::var("HIVE_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
                let stream_worker = std::path::Path::new("src/voice/tts_stream_worker.py");

                match tokio::process::Command::new(&python_cmd)
                    .arg(stream_worker)
                    .arg(&response_text)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .kill_on_drop(true)
                    .spawn()
                {
                    Ok(mut child) => {
                        if let Some(stdout) = child.stdout.take() {
                            use tokio::io::AsyncReadExt;
                            let mut reader = tokio::io::BufReader::new(stdout);
                            // Read PCM chunks (4800 bytes = 100ms at 24kHz 16-bit mono)
                            let mut buf = vec![0u8; 4800];
                            loop {
                                match reader.read(&mut buf).await {
                                    Ok(0) => break, // EOF — stream done
                                    Ok(n) => {
                                        let _ = ws_sender.send(
                                            Message::Binary(buf[..n].to_vec().into())
                                        ).await;
                                    }
                                    Err(e) => {
                                        tracing::warn!("[GLASSES] TTS stream read error: {}", e);
                                        break;
                                    }
                                }
                            }
                        }
                        // Wait for process to finish and log any stderr
                        match child.wait_with_output().await {
                            Ok(output) => {
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                if !stderr.is_empty() {
                                    for line in stderr.lines() {
                                        if line.starts_with("ERROR") {
                                            tracing::warn!("[GLASSES] TTS: {}", line);
                                        } else {
                                            tracing::debug!("[GLASSES] TTS: {}", line);
                                        }
                                    }
                                }
                            }
                            Err(e) => tracing::warn!("[GLASSES] TTS process error: {}", e),
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[GLASSES] TTS spawn failed: {}", e);
                    }
                }

                // Send done indicator
                let done_msg = serde_json::json!({"type": "done"});
                let _ = ws_sender.send(Message::Text(done_msg.to_string().into())).await;

                session.is_processing = false;
            }
        }
    }

    // Cleanup — clear runtime session cache (persistent link stays on disk)
    unregister_connection(&platform_id).await;
    link::clear_session(&platform_id).await;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glasses_session_audio() {
        let mut session = GlassesSession::new("user1".into(), "User One".into());
        assert!(!session.has_audio());

        // Add 500ms of audio at 16kHz, 16-bit, mono = 16000 bytes
        session.add_audio_chunk(vec![0u8; 16000]);
        assert!(session.has_audio());

        let audio = session.take_audio();
        assert_eq!(audio.len(), 16000);
        assert!(!session.has_audio());
    }

    #[test]
    fn test_glasses_session_frames() {
        let mut session = GlassesSession::new("user1".into(), "User One".into());
        assert!(session.latest_frame().is_none());

        use base64::Engine;
        let test_data = base64::engine::general_purpose::STANDARD.encode(b"test_jpeg_data");
        session.add_frame(&test_data);
        assert!(session.latest_frame().is_some());
        assert_eq!(session.latest_frame().unwrap(), b"test_jpeg_data");

        for i in 0..MAX_FRAME_BUFFER + 1 {
            let data = base64::engine::general_purpose::STANDARD.encode(format!("frame_{}", i).as_bytes());
            session.add_frame(&data);
        }
        assert_eq!(session.frames.len(), MAX_FRAME_BUFFER);
    }

    #[test]
    fn test_validate_token_no_config() {
        let result = GlassesPlatform::validate_token("token=anything");
        assert!(result.is_some() || std::env::var("HIVE_GLASSES_TOKEN").is_ok());
    }

    #[test]
    fn test_platform_name() {
        let platform = GlassesPlatform::new();
        assert_eq!(platform.name(), "glasses");
    }

    #[test]
    fn test_tts_float_to_pcm16_conversion() {
        // Test the float32 → i16 conversion logic
        let sample_f32: f32 = 0.5;
        let clamped = sample_f32.clamp(-1.0, 1.0);
        let sample_i16 = (clamped * 32767.0) as i16;
        assert_eq!(sample_i16, 16383);

        // Full scale positive
        let full_pos = 1.0_f32.clamp(-1.0, 1.0);
        assert_eq!((full_pos * 32767.0) as i16, 32767);

        // Full scale negative
        let full_neg = (-1.0_f32).clamp(-1.0, 1.0);
        assert_eq!((full_neg * 32767.0) as i16, -32767);

        // Over-range clamping
        let over = 1.5_f32.clamp(-1.0, 1.0);
        assert_eq!((over * 32767.0) as i16, 32767);
    }
}
