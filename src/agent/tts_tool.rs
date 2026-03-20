use crate::models::tool::{ToolResult, ToolStatus};
use crate::agent::preferences::extract_tag;
use tokio::sync::mpsc;

pub async fn execute_voice_synthesizer(
    task_id: String,
    description: String,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    let text = extract_tag(&description, "text:").unwrap_or_else(|| description.clone());
    
    tracing::debug!("[AGENT:tts] ▶ task_id={}", task_id);
    // Attempt ONNX generation
    let mock_res = match crate::voice::kokoro::KokoroTTS::new().await {
        Ok(engine) => {
            match engine.get_audio_path(&text).await {
                Ok(path) => Ok(path),
                Err(e) => Err(format!("Voice generation failed: {}", e)),
            }
        }
        Err(e) => Err(format!("Voice initialization failed: {}", e)),
    };

    execute_voice_synthesizer_inner(task_id, &text, telemetry_tx, mock_res).await
}

pub async fn execute_voice_synthesizer_inner(
    task_id: String,
    text: &str,
    telemetry_tx: Option<mpsc::Sender<String>>,
    engine_result: Result<std::path::PathBuf, String>
) -> ToolResult {
    if let Some(tx) = &telemetry_tx {
        let max_len = 50;
        let mut display_text = text.to_string();
        if display_text.len() > max_len {
            display_text.truncate(max_len);
            display_text.push_str("...");
        }
        let _ = tx
            .send(format!(
                "🗣️ Voice Synthesizer: generating Kokoro audio for '{}'\n",
                display_text
            ))
            .await;
        let _ = tx.send("typing_indicator".into()).await;
    }

    match engine_result {
        Ok(path) => {
            if let Some(tx) = &telemetry_tx {
                let _ = tx
                    .send(format!("✨ Audio generation complete: {}\n", path.display()))
                    .await;
            }
            ToolResult {
                task_id,
                tokens_used: 0,
                status: ToolStatus::Success,
                output: format!(
                    "Audio generated successfully. YOU MUST include this EXACT tag in your human conversational response to proactively play the audio for the user:\n\n[ATTACH_AUDIO]({})\n\n",
                    path.display()
                ),
            }
        }
        Err(e) => {
            if let Some(tx) = &telemetry_tx {
                let _ = tx
                    .send(format!("❌ Voice Generator error: {}\n", e))
                    .await;
            }
            ToolResult {
                task_id,
                tokens_used: 0,
                status: ToolStatus::Failed(e.clone()),
                output: format!("{}", e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_voice_synthesizer_success() {
        let res = execute_voice_synthesizer_inner(
            "1".into(),
            "A message successfully mocked out so telemtry fires",
            None,
            Ok(std::path::PathBuf::from("/mock/path.wav"))
        ).await;
        
        assert_eq!(res.status, ToolStatus::Success);
        assert!(res.output.contains("/mock/path.wav"));
    }

    #[tokio::test]
    async fn test_execute_voice_synthesizer_failure() {
        let (tx, _rx) = mpsc::channel(10);
        
        let res = execute_voice_synthesizer_inner(
            "2".into(),
            "A very long message that gets safely truncated because it is well over fifty characters in length and therefore hits the branch safely.",
            Some(tx),
            Err("Some dummy IO fail".into())
        ).await;
        
        assert_eq!(res.status, ToolStatus::Failed("Some dummy IO fail".into()));
        assert!(res.output.contains("fail"));
    }
}
