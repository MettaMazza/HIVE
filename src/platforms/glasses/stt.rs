//! Speech-to-Text for the Glasses Platform.
//!
//! Transcribes raw PCM audio (16kHz, 16-bit, mono) to text using
//! Google Speech Recognition via a tiny Python subprocess.
//! This mirrors the Ernos 3.0 approach (stt.py using SpeechRecognition lib)
//! and avoids introducing large native dependencies like Whisper.cpp.
//!
//! For production, this can be replaced with:
//! - Whisper.cpp via FFI (offline, fast on M3 Ultra)
//! - HTTP call to a local Whisper server
//! - Apple's on-device Speech framework

use std::path::PathBuf;

/// Path to the STT worker script.
fn stt_worker_path() -> PathBuf {
    PathBuf::from("src/platforms/glasses/stt_worker.py")
}

/// Transcribe raw PCM audio (16kHz, 16-bit, mono) to text.
///
/// Writes the audio to a temporary WAV file, runs the Python STT worker,
/// and returns the transcription. Returns empty string on failure.
pub async fn transcribe_pcm(pcm_data: &[u8]) -> String {
    if pcm_data.len() < 3200 {
        // Less than 100ms of audio — not enough to transcribe
        return String::new();
    }

    // Write PCM data as a WAV file to a temp location
    let temp_id = uuid::Uuid::new_v4();
    let wav_path = std::env::temp_dir().join(format!("hive_stt_{}.wav", temp_id));

    if let Err(e) = write_wav_file(&wav_path, pcm_data, 16000, 16, 1).await {
        tracing::error!("[STT] Failed to write temp WAV: {}", e);
        return String::new();
    }

    // Run the Python STT worker
    let python_cmd = std::env::var("HIVE_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let worker = stt_worker_path();

    let result = tokio::process::Command::new(&python_cmd)
        .arg(&worker)
        .arg(&wav_path)
        .kill_on_drop(true)
        .output()
        .await;

    // Clean up temp file
    let _ = tokio::fs::remove_file(&wav_path).await;

    match result {
        Ok(output) => {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if text.starts_with("TRANSCRIPTION:") {
                    text.strip_prefix("TRANSCRIPTION:").unwrap_or("").trim().to_string()
                } else {
                    tracing::warn!("[STT] Unexpected output format: {}", text);
                    String::new()
                }
            } else {
                let err = String::from_utf8_lossy(&output.stderr);
                tracing::warn!("[STT] Worker failed: {}", err);
                String::new()
            }
        }
        Err(e) => {
            tracing::error!("[STT] Failed to spawn worker: {}", e);
            String::new()
        }
    }
}

/// Write raw PCM data as a WAV file.
async fn write_wav_file(
    path: &std::path::Path,
    pcm_data: &[u8],
    sample_rate: u32,
    bits_per_sample: u16,
    channels: u16,
) -> std::io::Result<()> {
    let data_size = pcm_data.len() as u32;
    let byte_rate = sample_rate * (channels as u32) * (bits_per_sample as u32) / 8;
    let block_align = channels * bits_per_sample / 8;

    let mut header = Vec::with_capacity(44);
    // RIFF header
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data_size).to_le_bytes());
    header.extend_from_slice(b"WAVE");
    // fmt chunk
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    header.extend_from_slice(&1u16.to_le_bytes());  // PCM format
    header.extend_from_slice(&channels.to_le_bytes());
    header.extend_from_slice(&sample_rate.to_le_bytes());
    header.extend_from_slice(&byte_rate.to_le_bytes());
    header.extend_from_slice(&block_align.to_le_bytes());
    header.extend_from_slice(&bits_per_sample.to_le_bytes());
    // data chunk
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_size.to_le_bytes());

    let mut file_data = header;
    file_data.extend_from_slice(pcm_data);

    tokio::fs::write(path, &file_data).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_write_wav_file() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_stt_wav.wav");

        // 100ms of silence at 16kHz, 16-bit, mono = 3200 bytes
        let pcm_data = vec![0u8; 3200];
        write_wav_file(&path, &pcm_data, 16000, 16, 1).await.unwrap();

        let bytes = tokio::fs::read(&path).await.unwrap();
        // 44 bytes header + 3200 bytes data
        assert_eq!(bytes.len(), 44 + 3200);
        // Check RIFF header
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn test_transcribe_pcm_too_short() {
        // Audio too short — should return empty
        let result = transcribe_pcm(&[0u8; 100]).await;
        assert!(result.is_empty());
    }
}
