use async_trait::async_trait;
use mockall::automock;

use crate::models::message::Event;

pub mod lmstudio;
pub mod ollama;

#[derive(thiserror::Error, Debug)]
pub enum ProviderError {
    #[error("Failed to connect to provider: {0}")]
    ConnectionError(String),
    #[error("Failed to parse provider response: {0}")]
    ParseError(String),
}

/// The core trait for any LLM Provider powering the HIVE system persona (Apis).
/// Generating responses requires:
/// - The strict system prompt defining Apis
/// - The securely scoped contextual history of events
/// - The specific triggering event
#[automock]
#[async_trait]
pub trait Provider: Send + Sync {
    /// Generate a response block.
    /// `agent_context` contains the stringified tool execution results accumulated in inner loops.
    async fn generate(
        &self,
        system_prompt: &str,
        history: &[Event],
        new_event: &Event,
        agent_context: &str,
        telemetry_tx: Option<tokio::sync::mpsc::Sender<String>>,
    ) -> Result<String, ProviderError>;
}
