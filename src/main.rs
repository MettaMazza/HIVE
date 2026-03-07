#![allow(unexpected_cfgs)]

mod engine;
mod memory;
mod models;
mod platforms;
mod providers;

use std::sync::Arc;
use tokio::io::AsyncBufRead;
use crate::engine::EngineBuilder;
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

    // Build the engine with our defined platforms
    let engine = EngineBuilder::new()
        .with_platform(Box::new(DiscordPlatform::new(discord_token)))
        .with_platform(Box::new(CliPlatform::new(reader)))
        .with_provider(Arc::new(OllamaProvider::new()))
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


