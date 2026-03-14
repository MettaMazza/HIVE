#![allow(unexpected_cfgs)]

mod engine;
mod memory;
mod models;
mod platforms;
pub mod prompts;
mod providers;
pub mod agent;
pub mod teacher;

use std::sync::Arc;
use tokio::io::AsyncBufRead;
use crate::engine::EngineBuilder;
use crate::models::capabilities::AgentCapabilities;
use crate::platforms::discord::DiscordPlatform;
use crate::platforms::cli::CliPlatform;
use crate::providers::ollama::OllamaProvider;

#[cfg(not(tarpaulin_include))]
#[cfg(not(test))]
fn get_reader() -> Box<dyn AsyncBufRead + Unpin + Send + Sync> {
    Box::new(tokio::io::BufReader::new(tokio::io::stdin()))
}

#[cfg(not(tarpaulin_include))]
#[cfg(test)]
fn get_reader() -> Box<dyn AsyncBufRead + Unpin + Send + Sync> {
    Box::new(std::io::Cursor::new(b""))
}

#[cfg(not(tarpaulin_include))]
pub async fn run_app() {
    println!("Starting HIVE...");
    let reader = get_reader();

    let discord_token = std::env::var("DISCORD_TOKEN").unwrap_or_default();

    // 1. Initialize Core Storage Systems First
    let memory_store = Arc::new(crate::memory::MemoryStore::default());
    let provider = Arc::new(OllamaProvider::new());
    
    // 2. Initialize Agent Manager to gather Native Tolls (Tools)
    let agent_manager = crate::agent::AgentManager::new(provider.clone(), memory_store.clone());
    let native_tools = agent_manager.get_tool_names();

    // 3. Inject Dynamic Tool Tooling into Capabilities 
    let capabilities = AgentCapabilities {
        admin_users: vec![
            "1299810741984956449".into(), // metta_mazza
            "1282286389953695745".into(), // afreakyfrog
            "local_admin".into(),         // CLI access
        ],
        has_terminal_access: true,
        has_internet_access: true,
        admin_tools: vec![
            "run_bash_command".into(),
            "write_file".into(),
            "delete_file".into(),
        ],
        default_tools: native_tools, // <-- Dynamically Assigned 
    };

    // 4. Build the engine with our defined platforms and injected contexts
    let engine = EngineBuilder::new()
        .with_platform(Box::new(DiscordPlatform::new(discord_token)))
        .with_platform(Box::new(CliPlatform::new(reader)))
        .with_provider(provider)
        .with_capabilities(capabilities)
        .with_agent(Arc::new(agent_manager))
        .build()
        .expect("Failed to build Engine");

    // Run the engine indefinitely
    engine.run().await;
}

#[cfg(not(tarpaulin_include))]
#[tokio::main]
async fn main() {
    run_app().await;
}


