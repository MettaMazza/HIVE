use async_trait::async_trait;
use tokio::sync::mpsc::Sender;

use crate::models::message::{Event, Response};

pub mod discord;
pub mod cli;

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
}

#[derive(thiserror::Error, Debug)]
pub enum PlatformError {

    #[error("Platform specific error: {0}")]
    Other(String),
}
