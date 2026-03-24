use std::sync::Arc;
use tokio::time::{sleep, Duration};
use serde::{Deserialize, Serialize};
use crate::memory::MemoryStore;
use crate::models::message::Event;
use chrono::{DateTime, Utc};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlarmPayload {
    pub id: String,
    pub trigger_time: DateTime<Utc>,
    pub message: String,
    pub status: String,
}

pub async fn spawn_chronos(memory: Arc<MemoryStore>) {
    tokio::spawn(async move {
        // Guarantee memory directory exists physically
        let alarms_path = Path::new("memory").join("alarms.json");
        let _ = std::fs::create_dir_all("memory");
        
        tracing::info!("[CHRONOS] ⏳ Started temporal synchronizer loop (tick = 20s)");

        loop {
            if alarms_path.exists() {
                if let Ok(contents) = tokio::fs::read_to_string(&alarms_path).await {
                    if let Ok(mut alarms) = serde_json::from_str::<Vec<AlarmPayload>>(&contents) {
                        let now = Utc::now();
                        let mut dirty = false;

                        for alarm in alarms.iter_mut() {
                            if alarm.status == "pending" && now >= alarm.trigger_time {
                                // Time has passed, fire the alarm natively into Working Memory
                                let event = Event {
                                    platform: "internal".into(),
                                    scope: crate::models::scope::Scope::Private { user_id: "apismeta".into() },
                                    author_name: "Chronos Daemon".into(),
                                    author_id: "system".into(),
                                    content: format!("⏰ **ALARM TRIGGERED**\n\n**Payload:** {}", alarm.message),
                                    timestamp: Some(Utc::now().to_rfc3339()),
                                    message_index: None,
                                };
                                
                                memory.working.add_event(event).await;
                                tracing::info!("[CHRONOS] ⏰ Fired temporal hook: {}", alarm.id);
                                
                                alarm.status = "triggered".into();
                                dirty = true;
                            }
                        }

                        // Write changes back to the JSON map
                        if dirty {
                            if let Ok(json) = serde_json::to_string_pretty(&alarms) {
                                let _ = tokio::fs::write(&alarms_path, json).await;
                            }
                        }
                    }
                }
            }
            sleep(Duration::from_secs(20)).await;
        }
    });
}
