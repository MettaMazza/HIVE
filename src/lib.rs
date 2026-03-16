#![allow(unexpected_cfgs)]

pub mod engine;
pub mod memory;
pub mod models;
pub mod platforms;
pub mod prompts;
pub mod providers;
pub mod agent;
pub mod teacher;
pub mod computer;
pub mod voice;

use std::sync::Arc;
use tokio::io::AsyncBufRead;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use crate::engine::EngineBuilder;
use crate::models::capabilities::AgentCapabilities;
use crate::platforms::discord::DiscordPlatform;
use crate::platforms::cli::CliPlatform;
use crate::providers::ollama::OllamaProvider;

#[cfg(not(tarpaulin_include))]
#[cfg(not(test))]
pub fn get_reader() -> Box<dyn AsyncBufRead + Unpin + Send + Sync> {
    Box::new(tokio::io::BufReader::new(tokio::io::stdin()))
}

#[cfg(not(tarpaulin_include))]
#[cfg(test)]
pub fn get_reader() -> Box<dyn AsyncBufRead + Unpin + Send + Sync> {
    Box::new(std::io::Cursor::new(b""))
}

/// Returns the list of admin user IDs from ADMIN_USER_IDS env var.
/// Format: comma-separated list of user IDs
/// Falls back to the hardcoded defaults if not set.
fn get_admin_users() -> Vec<String> {
    std::env::var("ADMIN_USER_IDS")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(|id| id.trim().to_string()).filter(|id| !id.is_empty()).collect())
        .unwrap_or_else(|| vec![
            "1299810741984956449".into(),
            "1282286389953695745".into(),
            "local_admin".into(),
        ])
}

#[cfg(not(tarpaulin_include))]
pub async fn run() {
    let file_appender = tracing_appender::rolling::daily("logs", "hive.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,HIVE=debug"));

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stdout.and(non_blocking))
        .finish();
        
    let _ = tracing::subscriber::set_global_default(subscriber);

    tracing::info!("Starting HIVE initialization sequence...");
    let reader = get_reader();
    
    dotenv::dotenv().ok();

    let discord_token = std::env::var("DISCORD_TOKEN").unwrap_or_default();

    let memory_store = Arc::new(crate::memory::MemoryStore::default());
    let provider = Arc::new(OllamaProvider::new());
    
    let agent_manager = crate::agent::AgentManager::new(provider.clone(), memory_store.clone());
    let native_tools = agent_manager.get_tool_names();

    let capabilities = AgentCapabilities {
        admin_users: get_admin_users(),
        has_terminal_access: true,
        has_internet_access: true,
        admin_tools: vec![
            "run_bash_command".into(),
            "process_manager".into(),
            "file_system_operator".into(),
        ],
        default_tools: native_tools,
    };

    let engine = EngineBuilder::new()
        .with_platform(Box::new(DiscordPlatform::new(discord_token, memory_store.clone())))
        .with_platform(Box::new(CliPlatform::new(reader)))
        .with_provider(provider)
        .with_capabilities(capabilities)
        .build()
        .expect("Failed to build Engine");

    tokio::select! {
        _ = engine.run() => {
            tracing::info!("Engine shut down gracefully.");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::warn!("Received Ctrl-C, executing shutdown sequence...");
            tracing::info!("Shutting down HIVE... saving temporal state.");
            memory_store.temporal.write().await.record_shutdown();
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            tracing::info!("Shutdown complete.");
        }
    }
}
