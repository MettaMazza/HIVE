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

        match serde_json::to_string(&pair) {
            Ok(json) => {
                match tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.preference_path)
                    .await
                {
                    Ok(mut file) => {
                        if let Err(e) = file.write_all(format!("{}\n", json).as_bytes()).await {
                            tracing::error!("[TEACHER] ❌ Failed to write preference pair to '{}': {}", self.preference_path.display(), e);
                            return;
                        }
                        let current_count = self.preference_count.fetch_add(1, Ordering::Relaxed) + 1;
                        tracing::info!("[TEACHER] ⚖️ Preference pair captured [{}] ({} buffered)", failure_category, current_count);

                        // --- ORPO AUTONOMOUS ALIGNMENT DAEMON ---
                        // Once we accumulate PAIR_THRESHOLD preference pairs, spawn the real
                        // training pipeline in micro mode to immediately align on the new data.
                        if current_count > 0 && current_count % crate::teacher::PAIR_THRESHOLD == 0 {
                            tracing::warn!("[TEACHER] 🧠 Threshold reached ({} pairs). Spawning ORPO alignment...", current_count);

                            let python_bin = std::env::var("HIVE_PYTHON_BIN")
                                .unwrap_or_else(|_| "python3".to_string());
                            let backend = std::env::var("HIVE_TRAINING_BACKEND")
                                .unwrap_or_else(|_| "auto".to_string());

                            let in_docker = std::path::Path::new("/.dockerenv").exists();

                            tokio::spawn(async move {
                                if in_docker {
                                    // Docker: call host training server
                                    let train_url = std::env::var("HIVE_TRAINING_URL")
                                        .unwrap_or_else(|_| "http://host.docker.internal:8491".to_string());

                                    let payload = serde_json::json!({
                                        "micro": true,
                                        "stack": true,
                                        "backend": backend,
                                        "examples": current_count,
                                    });

                                    match reqwest::Client::new()
                                        .post(format!("{}/train", train_url))
                                        .json(&payload)
                                        .timeout(std::time::Duration::from_secs(3600))
                                        .send()
                                        .await
                                    {
                                        Ok(resp) => {
                                            let body: serde_json::Value = resp.json().await.unwrap_or_default();
                                            let status = body["status"].as_str().unwrap_or("unknown");
                                            if status == "success" {
                                                tracing::info!("[TEACHER] ✅ ORPO alignment complete (host):\n{}",
                                                    body["stdout"].as_str().unwrap_or(""));
                                            } else {
                                                tracing::error!("[TEACHER] ❌ ORPO alignment failed (host): {}",
                                                    body["stderr"].as_str().unwrap_or("unknown error"));
                                            }
                                        }
                                        Err(e) => tracing::error!("[TEACHER] ❌ Failed to reach host training server: {}", e),
                                    }
                                } else {
                                    // Native: run subprocess directly
                                    match tokio::process::Command::new(&python_bin)
                                        .arg("training/train_teacher.py")
                                        .arg("--micro")
                                        .arg("--stack")
                                        .arg("--backend")
                                        .arg(&backend)
                                        .arg("--examples")
                                        .arg(current_count.to_string())
                                        .output()
                                        .await
                                    {
                                        Ok(result) => {
                                            let stdout = String::from_utf8_lossy(&result.stdout);
                                            let stderr = String::from_utf8_lossy(&result.stderr);
                                            if result.status.success() {
                                                tracing::info!("[TEACHER] ✅ ORPO alignment complete:\n{}", stdout);
                                            } else {
                                                tracing::error!("[TEACHER] ❌ ORPO alignment failed (exit {}):\nstdout: {}\nstderr: {}",
                                                    result.status, stdout, stderr);
                                            }
                                        }
                                        Err(e) => tracing::error!("[TEACHER] ❌ Failed to spawn ORPO training: {}", e),
                                    }
                                }
                            });
                        }
                    }
                    Err(e) => tracing::error!("[TEACHER] ❌ Failed to open preference buffer '{}': {}", self.preference_path.display(), e),
                }
            }
            Err(e) => tracing::error!("[TEACHER] ❌ Failed to serialize preference pair: {}", e),
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
