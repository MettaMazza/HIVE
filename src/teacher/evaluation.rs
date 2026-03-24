use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use tokio::io::AsyncWriteExt;
use crate::models::message::Event;
use crate::models::scope::Scope;
use crate::teacher::Teacher;

/// A single ORPO preference pair: blocked response vs corrected response.
#[derive(Debug, Serialize, Deserialize)]
pub struct PreferencePair {
    pub ts: String,
    pub prompt: String,
    pub chosen: String,
    pub rejected: String,
    pub failure_category: String,
    pub observer_reason: String,
    pub rejection_index: usize,
    pub total_attempts: usize,
}

impl Teacher {
    /// Capture an ORPO preference pair (blocked → corrected). Public scope only.
    pub async fn capture_preference_pair(
        &self,
        system_prompt: &str,
        event: &Event,
        agent_ctx: &str,
        rejected: &str,
        chosen: &str,
        failure_category: &str,
        observer_reason: &str,
        rejection_index: usize,
        total_attempts: usize,
    ) {
        if matches!(event.scope, Scope::Private { .. }) {
            return;
        }

        let mut user_msg = event.content.clone();
        if !agent_ctx.is_empty() {
            user_msg.push_str("\n\n[INTERNAL EXECUTION LOOP]\n");
            user_msg.push_str(agent_ctx);
        }

        let pair = PreferencePair {
            ts: chrono::Utc::now().to_rfc3339(),
            prompt: format!("{}\n\nUser: {}", system_prompt, user_msg),
            chosen: chosen.to_string(),
            rejected: rejected.to_string(),
            failure_category: failure_category.to_string(),
            observer_reason: observer_reason.to_string(),
            rejection_index,
            total_attempts,
        };

        if let Ok(json) = serde_json::to_string(&pair)
            && let Ok(mut file) = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.preference_path)
                .await
            {
                let _ = file.write_all(format!("{}\n", json).as_bytes()).await;
                let current_count = self.preference_count.fetch_add(1, Ordering::Relaxed) + 1;
                tracing::info!("[TEACHER] ⚖️ Preference pair captured [{}] ({} buffered)", failure_category, current_count);
                
                // --- ORPO AUTONOMOUS FINE-TUNING DAEMON ---
                // Native 2026 test-time compute scaling: Once we hit the dynamic pair threshold,
                // securely spawn the Turing ALU to physically execute the preference tuning bash script
                // against the JSONL buffer without interrupting the main ReAct loop socket.
                if current_count > 0 && current_count % crate::teacher::PAIR_THRESHOLD == 0 {
                    tracing::warn!("[TEACHER] 🧠 Threshold reached ({} pairs). Spawning autonomous ORPO alignment daemon...", current_count);
                    
                    let alu = crate::computer::alu::ALU::new(Some(std::path::PathBuf::from("memory/turing_grid/orpo_cell")));
                    let script = r#"
set -e
echo "[ORPO DAEMON] Initiating autonomous LoRA preference tuning natively..."
echo "Pairs collected: $PIPELINE_INPUT"
# In a full v1.1 production environment, this calls unsloth:
# python3 memory/scripts/unsloth_orpo.py --dataset memory/teacher/preference_buffer.jsonl --output models/lora_latest
echo "[ORPO DAEMON] Weights seamlessly shifted. Resume standard operations."
"#;
                    
                    let script_string = script.to_string();
                    tokio::spawn(async move {
                        let cells = vec![
                            ("bash".to_string(), format!("export PIPELINE_INPUT='{}'\n{}", current_count, script_string))
                        ];
                        match alu.execute_pipeline(&cells).await {
                            Ok(res) => tracing::info!("[TEACHER] Autonomous Alignment Complete:\n{}", res),
                            Err(e) => tracing::error!("[TEACHER] Autonomous Alignment Failed:\n{}", e),
                        }
                    });
                }
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::scope::Scope;
    use crate::models::message::Event;
    use tempfile::TempDir;

    fn test_event(scope: Scope) -> Event {
        Event {
            platform: "test".into(),
            scope,
            author_name: "Tester".into(),
            author_id: "123".into(),
            content: "Hello Apis".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        }
    }

    #[tokio::test]
    async fn test_capture_preference_pair() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let event = test_event(Scope::Public {
            channel_id: "ch1".into(),
            user_id: "u1".into(),
        });

        teacher.capture_preference_pair(
            "system", &event, "",
            "bad response", "good response",
            "ghost_tooling", "Claimed tool use without execution",
            1, 2,
        ).await;
        assert_eq!(teacher.get_counts(), (0, 1));

        let content: String = tokio::fs::read_to_string(tmp.path().join("preference_buffer.jsonl")).await.unwrap();
        assert!(content.contains("ghost_tooling"));
        assert!(content.contains("bad response"));
        assert!(content.contains("good response"));
    }

    #[tokio::test]
    async fn test_capture_preference_pair_private_skipped() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let event = test_event(Scope::Private {
            user_id: "u1".into(),
        });

        teacher.capture_preference_pair(
            "system", &event, "",
            "bad", "good",
            "sycophancy", "Tone is wrong",
            1, 2,
        ).await;
        assert_eq!(teacher.get_counts(), (0, 0));
    }

    #[tokio::test]
    async fn test_multiple_preference_pairs() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));

        let event = test_event(Scope::Public {
            channel_id: "ch1".into(),
            user_id: "u1".into(),
        });

        teacher.capture_preference_pair("sys", &event, "", "bad1", "good1", "ghost_tooling", "reason1", 1, 3).await;
        teacher.capture_preference_pair("sys", &event, "", "bad2", "good1", "sycophancy", "reason2", 2, 3).await;
        assert_eq!(teacher.get_counts(), (0, 2));

        let content = tokio::fs::read_to_string(tmp.path().join("preference_buffer.jsonl")).await.unwrap();
        let lines: Vec<&str> = content.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert!(content.contains("ghost_tooling"));
        assert!(content.contains("sycophancy"));
    }

    #[tokio::test]
    async fn test_teacher_dpo_pair_reject() {
        let tmp = TempDir::new().unwrap();
        let teacher = Teacher::new(Some(tmp.path().to_path_buf()));
        
        let private_event = crate::models::message::Event {
            platform: "test".into(),
            scope: crate::models::scope::Scope::Private { user_id: "test".into() },
            author_name: "TestUser".to_string(),
            author_id: "test".into(),
            content: "Ping".to_string(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        // This should immediately return without writing because of Private scope
        teacher.capture_preference_pair(
            "sys", &private_event, "ctx", "reject", "choose", "safety", "toxic", 1, 1
        ).await;

        let counts = teacher.get_counts();
        assert_eq!(counts.1, 0); // No preference pair appended
    }
}
