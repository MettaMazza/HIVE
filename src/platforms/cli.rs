use async_trait::async_trait;
use tokio::sync::mpsc::Sender;
use tokio::io::{AsyncBufReadExt, AsyncBufRead};
use tokio::sync::Mutex;
use std::sync::Arc;

use crate::models::message::{Event, Response};
use crate::models::scope::Scope;
use super::{Platform, PlatformError};

/// A local terminal interface for interacting with Apis directly.
pub struct CliPlatform {
    reader: Arc<Mutex<Option<Box<dyn AsyncBufRead + Unpin + Send + Sync>>>>,
}

impl CliPlatform {
    pub fn new<R>(reader: R) -> Self 
    where R: AsyncBufRead + Unpin + Send + Sync + 'static 
    {
        Self {
            reader: Arc::new(Mutex::new(Some(Box::new(reader))))
        }
    }
}

#[async_trait]
impl Platform for CliPlatform {
    fn name(&self) -> &str {
        "cli"
    }

    #[cfg(not(tarpaulin_include))]
    async fn start(&self, event_sender: Sender<Event>) -> Result<(), PlatformError> {
        let sender = event_sender.clone();
        let reader = self.reader.lock().await.take().ok_or(PlatformError::Other("Already started".into()))?;
        
        tokio::spawn(async move {
            let mut lines = reader.lines();
            tracing::info!("HIVE CLI initialized. Type your message to Apis. (Prefix with /dm to test private scope)");

            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }

                let (scope, content) = if line.starts_with("/dm ") {
                    (Scope::Private { user_id: "local_admin".to_string() }, line.trim_start_matches("/dm ").to_string())
                } else {
                    (Scope::Public { channel_id: "cli_local".into(), user_id: "local_admin".into() }, line.clone())
                };

                let event = Event {
                    platform: "cli".to_string(),
                    scope,
                    author_name: "Admin".to_string(),
                    author_id: "local_admin".to_string(),
                    content,
                    timestamp: Some(chrono::Utc::now().to_rfc3339()),
                    message_index: None,
                };

                if sender.send(event).await.is_err() {
                    tracing::error!("Failed to send event to engine");
                    break;
                }
            }
        });

        Ok(())
    }

    #[cfg(not(tarpaulin_include))]
    async fn send(&self, response: Response) -> Result<(), PlatformError> {
        match response.target_scope {
            Scope::Public { .. } => {
                println!("\x1b[36m🐝 {}\x1b[0m", response.text);
            }
            Scope::Private { user_id } => {
                println!("\x1b[35m🐝 (DM → {}) {}\x1b[0m", user_id, response.text);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokio::sync::mpsc;
    use crate::models::scope::Scope;

    #[tokio::test]
    async fn test_cli_name() {
        let cursor = Cursor::new(b"");
        let cli = CliPlatform::new(cursor);
        assert_eq!(cli.name(), "cli");
    }

    #[tokio::test]
    async fn test_cli_send_public() {
        let cursor = Cursor::new(b"");
        let cli = CliPlatform::new(cursor);
        let res = Response {
            platform: "cli".to_string(),
            target_scope: Scope::Public { channel_id: "cli".into(), user_id: "tester".into() },
            text: "Public test".to_string(),
            is_telemetry: false,
        };
        assert!(cli.send(res).await.is_ok());
    }

    #[tokio::test]
    async fn test_cli_send_private() {
        let cursor = Cursor::new(b"");
        let cli = CliPlatform::new(cursor);
        let res = Response {
            platform: "cli".to_string(),
            target_scope: Scope::Private { user_id: "u1".to_string() },
            text: "Private test".to_string(),
            is_telemetry: false,
        };
        assert!(cli.send(res).await.is_ok());
    }

    #[tokio::test]
    async fn test_cli_start_and_read() {
        let data = "Hello\n\n/dm Secret\n";
        let cursor = Cursor::new(data.as_bytes().to_vec());
        let cli = CliPlatform::new(cursor);
        
        let (tx, mut rx) = mpsc::channel(10);
        cli.start(tx.clone()).await.unwrap();

        let ev1 = rx.recv().await.unwrap();
        assert_eq!(ev1.content, "Hello");
        assert!(matches!(ev1.scope, Scope::Public { .. }));

        let ev2 = rx.recv().await.unwrap();
        assert_eq!(ev2.content, "Secret");
        assert!(matches!(ev2.scope, Scope::Private { .. }));
    }

    #[tokio::test]
    async fn test_cli_start_send_failure() {
        let data = "Hello\n";
        let cursor = Cursor::new(data.as_bytes().to_vec());
        let cli = CliPlatform::new(cursor);
        
        let (tx, rx) = mpsc::channel(1);
        drop(rx); // Immediately close receiver

        // start will spawn, it will read "Hello", try to send, and fail!
        cli.start(tx).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }
}
