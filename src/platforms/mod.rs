use async_trait::async_trait;
use tokio::sync::mpsc::Sender;

use crate::models::message::{Event, Response};

pub mod discord;
pub mod attachments;
pub mod telemetry;
pub mod cli;
pub mod glasses;

/// The foundational interface for any platform that HIVE connects to.
/// This ensures HIVE is entirely platform-neutral.
#[async_trait]
pub trait Platform: Send + Sync {
    /// The name of the platform (e.g., "discord", "cli")
    fn name(&self) -> &str;

    /// Starts the platform listener, turning external messages into HIVE `Event`s
    /// and pushing them down the `event_sender` channel.
    async fn start(&self, event_sender: Sender<Event>) -> Result<(), PlatformError>;

    /// Handles sending a HIVE `Response` back to the platform.
    async fn send(&self, response: Response) -> Result<(), PlatformError>;

    /// React to a message with an emoji. Default no-op for platforms that don't support it.
    async fn react(&self, _channel_id: u64, _message_id: u64, _emoji: &str) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Send a "Continue?" checkpoint prompt and wait for user response.
    /// Returns true if user wants to continue, false if they want to wrap up.
    /// Default: always continue (for non-interactive platforms).
    async fn ask_continue(&self, _channel_id: u64, _turn: usize) -> bool {
        true
    }
}

#[derive(thiserror::Error, Debug)]
pub enum PlatformError {

    #[error("Platform specific error: {0}")]
    Other(String),
}
