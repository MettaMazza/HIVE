#![allow(unexpected_cfgs)]

mod engine;
mod memory;
mod models;
mod platforms;
pub mod prompts;
mod providers;
pub mod agent;
pub mod teacher;
pub mod computer;
pub mod voice;
pub mod server;

use std::sync::Arc;
use tokio::io::AsyncBufRead;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use crate::engine::EngineBuilder;
use crate::models::capabilities::AgentCapabilities;
use crate::platforms::discord::DiscordPlatform;
use crate::platforms::cli::CliPlatform;
use crate::platforms::glasses::GlassesPlatform;
use crate::providers::ollama::OllamaProvider;
use crate::providers::Provider;

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
    // ── Master Rotating Log ─────────────────────────────────────────────
    // Daily rotation with max 7 rotated files (+ current = 8 on disk).
    // All subsystem logs ([ENGINE:*], [MEMORY:*], [AGENT:*], etc.) merge
    // into a single master file: logs/hive.log.YYYY-MM-DD
    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("hive")
        .filename_suffix("log")
        .max_log_files(8)
        .build("logs")
        .expect("Failed to create log appender");

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Dynamic verbosity via RUST_LOG env var (default: info globally and for HIVE)
    // The user requested cleaner logs, so we drop debug chunk telemetry from the CLI console natively.
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,HIVE=info"));

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stdout.and(non_blocking))
        .finish();

    let _ = tracing::subscriber::set_global_default(subscriber);

    tracing::info!("Starting HIVE initialization sequence...");
    let reader = get_reader();
    
    dotenv::dotenv().ok(); // Load .env file manually

    let discord_token = std::env::var("DISCORD_TOKEN").unwrap_or_default();

    // 1. Initialize Core Storage Systems First
    let memory_store = Arc::new(crate::memory::MemoryStore::default());
    let provider: Arc<dyn Provider> = match std::env::var("HIVE_PROVIDER").unwrap_or_else(|_| "ollama".into()).to_lowercase().as_str() {
        "openai" | "gpt" => {
            tracing::info!("[PROVIDER] Using OpenAI API provider");
            Arc::new(crate::providers::openai::OpenAiProvider::new().expect("Failed to init OpenAI provider — is OPENAI_API_KEY set?"))
        }
        "anthropic" | "claude" => {
            tracing::info!("[PROVIDER] Using Anthropic Claude API provider");
            Arc::new(crate::providers::anthropic::AnthropicProvider::new().expect("Failed to init Anthropic provider — is ANTHROPIC_API_KEY set?"))
        }
        "gemini" | "google" => {
            tracing::info!("[PROVIDER] Using Google Gemini API provider");
            Arc::new(crate::providers::gemini::GeminiProvider::new().expect("Failed to init Gemini provider — is GEMINI_API_KEY set?"))
        }
        "xai" | "grok" => {
            tracing::info!("[PROVIDER] Using xAI Grok API provider");
            Arc::new(crate::providers::xai::XaiProvider::new().expect("Failed to init xAI provider — is XAI_API_KEY set?"))
        }
        _ => {
            tracing::info!("[PROVIDER] Using local Ollama provider");
            Arc::new(OllamaProvider::new())
        }
    };
    
    // 2. Initialize Agent Manager to gather Native Tolls (Tools)
    let agent_manager = crate::agent::AgentManager::new(provider.clone(), memory_store.clone());
    let native_tools = agent_manager.get_tool_names();

    // 3. Inject Dynamic Tool Tooling into Capabilities 
    let capabilities = AgentCapabilities {
        admin_users: vec![
            "1299810741984956449".into(), // metta_mazza
            "1282286389953695745".into(), // afreakyfrog
            "local_admin".into(),         // CLI access
            "apis_autonomy".into(),       // Autonomy loop — full tool access
        ],
        has_terminal_access: true,
        has_internet_access: true,
        admin_tools: vec![
            "run_bash_command".into(),
            "process_manager".into(),
            "file_system_operator".into(),
            "download".into(),
        ],
        default_tools: native_tools, // <-- Dynamically Assigned 
    };

    // 4. Build the engine with our defined platforms and injected contexts
    let glasses_provider: Arc<dyn crate::providers::Provider> = Arc::new(OllamaProvider::with_model("qwen3.5:35b"));
    let engine = EngineBuilder::new()
        .with_platform(Box::new(DiscordPlatform::new(discord_token, memory_store.clone(), Arc::new(capabilities.clone()))))
        .with_platform(Box::new(CliPlatform::new(reader)))
        .with_platform(Box::new(GlassesPlatform::new()))
        .with_provider(provider)
        .with_platform_provider("glasses", glasses_provider)
        .with_capabilities(capabilities)
        .build()
        .expect("Failed to build Engine");

    // 5. Spawn the file server daemon (serves generated + downloaded files over HTTP)
    {
        let port: u16 = std::env::var("HIVE_FILE_SERVER_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8420);
        let token = std::env::var("HIVE_FILE_TOKEN").unwrap_or_default();
        tokio::spawn(async move {
            // Retry loop — handles port conflict on rapid restarts
            loop {
                let server = crate::computer::file_server::FileServer::new(port, token.clone());
                match server.run().await {
                    Ok(_) => break,
                    Err(e) => {
                        tracing::warn!("[FILE SERVER] Port {} unavailable: {} — retrying in 3s", port, e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    }
                }
            }
        });
    }

    // 6. Spawn the Cloudflare quick tunnel (auto-reconnects, writes public URL to disk)
    {
        let port: u16 = std::env::var("HIVE_FILE_SERVER_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8420);
        tokio::spawn(async move {
            loop {
                tracing::info!("[TUNNEL] Starting Cloudflare tunnel on port {}...", port);
                let child = tokio::process::Command::new("cloudflared")
                    .args([
                        "tunnel",
                        "--url", &format!("http://localhost:{}", port),
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::piped()) // cloudflared outputs URL to stderr
                    .kill_on_drop(true)
                    .spawn();

                match child {
                    Ok(mut proc) => {
                        if let Some(stderr) = proc.stderr.take() {
                            use tokio::io::{AsyncBufReadExt, BufReader};
                            let reader = BufReader::new(stderr);
                            let mut lines = reader.lines();
                            while let Ok(Some(line)) = lines.next_line().await {
                                // Only log non-error lines at debug to avoid noise
                                if line.contains("ERR") {
                                    tracing::trace!("[TUNNEL] {}", line);
                                } else {
                                    tracing::debug!("[TUNNEL] {}", line);
                                }
                                // Capture the public URL (e.g. https://xxx.trycloudflare.com)
                                if line.contains("trycloudflare.com")
                                    && let Some(url) = line.split_whitespace()
                                        .find(|s| s.starts_with("https://") && s.contains("trycloudflare.com"))
                                    {
                                        let url = url.trim_end_matches(['|', ' ']);
                                        let _ = tokio::fs::create_dir_all("memory/core").await;
                                        let _ = tokio::fs::write("memory/core/tunnel_url.txt", url).await;
                                        tracing::info!("[TUNNEL] ✅ Public URL: {}", url);
                                    }
                            }
                        }
                        let _ = proc.wait().await;
                    }
                    Err(e) => {
                        tracing::warn!("[TUNNEL] cloudflared not found or failed: {} — retrying in 30s", e);
                    }
                }
                tracing::info!("[TUNNEL] Connection lost, reconnecting in 10s...");
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            }
        });
    }

    // 7. Spawn the Local Memory Visualizer Server (Axum)
    {
        crate::server::visualizer_server::spawn_visualizer_server(memory_store.clone()).await;
    }

    // 8. Spawn the Native IMAP Background Inbox Listener
    {
        crate::engine::email_watcher::spawn_email_watcher(memory_store.clone()).await;
    }

    // 9. Spawn the Chronos Temporal Operations Daemon
    {
        crate::engine::chronos::spawn_chronos(memory_store.clone()).await;
    }

    // Run the engine indefinitely
    tokio::select! {
        _ = engine.run() => {
            tracing::info!("Engine shut down gracefully.");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::warn!("Received Ctrl-C, executing shutdown sequence...");
            tracing::info!("Shutting down HIVE... saving temporal state.");
            memory_store.temporal.write().await.record_shutdown();
            // Allow disk flushes
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            tracing::info!("Shutdown complete.");
        }
    }
}

#[cfg(not(tarpaulin_include))]
#[tokio::main]
async fn main() {
    run_app().await;
}


