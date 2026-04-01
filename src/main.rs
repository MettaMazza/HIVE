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
pub mod config;
pub mod network;
pub mod crypto;

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
    // Install rustls crypto provider before any TLS/QUIC code runs
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls CryptoProvider");

    dotenv::dotenv().ok(); // Load .env file manually first

    // ── Persisted Model Override ──────────────────────────────────
    // If a model swap was persisted to disk (via /model swap), use it
    // instead of the .env default. Lives in the Docker volume so it
    // survives rebuilds.
    if let Ok(override_model) = std::fs::read_to_string("memory/core/model_override.txt") {
        let model = override_model.trim().to_string();
        if !model.is_empty() {
            // SAFETY: Called once at startup before any threads read HIVE_MODEL
            unsafe { std::env::set_var("HIVE_MODEL", &model); }
            // Also override the deep model if not separately configured
            if std::env::var("HIVE_DEEP_MODEL_EXPLICIT").is_err() {
                unsafe { std::env::set_var("HIVE_DEEP_MODEL", &model); }
            }
            eprintln!("[BOOT] 🔄 Model override loaded from disk: {}", model);
        }
    }

    // ── Global Configuration ───────────────────────────────────────
    let config = crate::config::AppConfig::load_from_env();

    // ── Master Rotating Log ─────────────────────────────────────────────
    // Daily rotation with max 7 rotated files (+ current = 8 on disk).
    // All subsystem logs ([ENGINE:*], [MEMORY:*], [AGENT:*], etc.) merge
    // into a single master file: logs/hive.log.YYYY-MM-DD
    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("hive")
        .filename_suffix("log")
        .max_log_files(8)
        .build(&config.logs_dir)
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
    tracing::info!("[CONFIG] System parameters loaded. Identity: '{}'", config.system_name);

    // ── Auto-start Flux image generation server ────────────────────────
    // Runs as a background process alongside HIVE, like Ollama.
    // SKIP inside Docker — the host's Flux server handles it via HTTP
    // (launched by launch.sh, called via host.docker.internal:8490).
    // If Python or diffusers aren't available, it fails silently —
    // the image tool falls back to direct subprocess calls.
    let in_docker = std::path::Path::new("/.dockerenv").exists();
    if !in_docker {
        let python_bin = std::env::var("HIVE_PYTHON_BIN").unwrap_or_else(|_| {
            let local_venv = std::path::Path::new(".venv/bin/python3");
            if local_venv.exists() {
                local_venv.to_string_lossy().to_string()
            } else {
                "python3".to_string()
            }
        });
        let flux_script = std::path::Path::new("src/computer/flux_server.py");
        if flux_script.exists() {
            match std::process::Command::new(&python_bin)
                .arg(flux_script)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
            {
                Ok(_child) => {
                    tracing::info!("[FLUX] 🎨 Flux server starting on http://localhost:8490");
                }
                Err(e) => {
                    tracing::warn!("[FLUX] ⚠️ Could not start Flux server: {} — image generation will use direct subprocess", e);
                }
            }
        }
    }

    let discord_token = std::env::var("DISCORD_TOKEN").unwrap_or_default();

    let memory_store = Arc::new(crate::memory::MemoryStore::new(Some(std::path::PathBuf::from(&config.storage_root))));
    let stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // ── Unified Identity System ──────────────────────────────────────
    // Single persistent identity shared across all mesh platforms.
    let mesh_identity = Arc::new(crate::network::identity::MeshIdentity::load());
    // Create the model handle and base URL early — shared with Discord handler for /model command
    let ollama_base_url = std::env::var("HIVE_OLLAMA_URL")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());

    let (provider, model_handle): (Arc<dyn Provider>, Arc<tokio::sync::RwLock<String>>) = match std::env::var("HIVE_PROVIDER").unwrap_or_else(|_| "ollama".into()).to_lowercase().as_str() {
        "openai" | "gpt" => {
            tracing::info!("[PROVIDER] Using OpenAI API provider");
            let model_name = std::env::var("HIVE_MODEL").unwrap_or_else(|_| "gpt-4".into());
            (Arc::new(crate::providers::openai::OpenAiProvider::new(config.timeout_inference_secs, config.system_name.clone()).expect("Failed to init OpenAI provider — is OPENAI_API_KEY set?")),
             Arc::new(tokio::sync::RwLock::new(model_name)))
        }
        "anthropic" | "claude" => {
            tracing::info!("[PROVIDER] Using Anthropic Claude API provider");
            let model_name = std::env::var("HIVE_MODEL").unwrap_or_else(|_| "claude-3".into());
            (Arc::new(crate::providers::anthropic::AnthropicProvider::new(config.timeout_inference_secs, config.system_name.clone()).expect("Failed to init Anthropic provider — is ANTHROPIC_API_KEY set?")),
             Arc::new(tokio::sync::RwLock::new(model_name)))
        }
        "gemini" | "google" => {
            tracing::info!("[PROVIDER] Using Google Gemini API provider");
            let model_name = std::env::var("HIVE_MODEL").unwrap_or_else(|_| "gemini-pro".into());
            (Arc::new(crate::providers::gemini::GeminiProvider::new(config.timeout_inference_secs, config.system_name.clone()).expect("Failed to init Gemini provider — is GEMINI_API_KEY set?")),
             Arc::new(tokio::sync::RwLock::new(model_name)))
        }
        "xai" | "grok" => {
            tracing::info!("[PROVIDER] Using xAI Grok API provider");
            let model_name = std::env::var("HIVE_MODEL").unwrap_or_else(|_| "grok-1".into());
            (Arc::new(crate::providers::xai::XaiProvider::new(config.timeout_inference_secs, config.system_name.clone()).expect("Failed to init xAI provider — is XAI_API_KEY set?")),
             Arc::new(tokio::sync::RwLock::new(model_name)))
        }
        _ => {
            tracing::info!("[PROVIDER] Using local Ollama provider");
            let mut ollama = OllamaProvider::new(config.system_name.clone());
            ollama.set_stop_flag(stop_flag.clone());
            let handle = ollama.model_handle();
            (Arc::new(ollama), handle)
        }
    };
    
    // 2. Initialize Agent Manager to gather Native Tolls (Tools)
    let agent_manager = crate::agent::AgentManager::new(provider.clone(), memory_store.clone(), config.clone());
    let native_tools = agent_manager.get_tool_names();

    // 3. Inject Dynamic Tool Tooling into Capabilities 
    let capabilities = AgentCapabilities {
        admin_users: {
            let mut admins = config.admin_users.clone();
            // System-level users: always present regardless of env config
            if !admins.contains(&"local_admin".to_string()) {
                admins.push("local_admin".into());
            }
            if !admins.contains(&"apis_autonomy".to_string()) {
                admins.push("apis_autonomy".into());
            }
            admins
        },
        has_terminal_access: config.allow_terminal,
        has_internet_access: true,
        admin_tools: {
            let mut tools = vec!["download".into()];
            if config.allow_terminal {
                tools.push("run_bash_command".into());
                tools.push("process_manager".into());
            }
            if config.allow_file_system {
                tools.push("file_system_operator".into());
            }
            tools
        },
        default_tools: native_tools, // <-- Dynamically Assigned 
    };

    // 4. Build the engine with our defined platforms and injected contexts
    let glasses_model = std::env::var("HIVE_GLASSES_MODEL").unwrap_or_else(|_| "qwen3.5:35b".into());
    tracing::info!("[PROVIDER] Glasses platform using model: {} (set HIVE_GLASSES_MODEL to override)", glasses_model);
    let glasses_provider: Arc<dyn crate::providers::Provider> = Arc::new(OllamaProvider::with_model(&glasses_model, config.system_name.clone()));
    let mut engine = EngineBuilder::new()
        .with_config(config.clone())
        .with_platform(Box::new(DiscordPlatform::new(discord_token, memory_store.clone(), Arc::new(capabilities.clone()), model_handle, ollama_base_url)))
        .with_platform(Box::new(CliPlatform::new(reader)))
        .with_platform(Box::new(GlassesPlatform::new()))
        .with_provider(provider)
        .with_platform_provider("glasses", glasses_provider)
        .with_capabilities(capabilities)
        .with_memory(memory_store.clone())
        .build()
        .expect("Failed to build Engine");

    // Share the stop flag between engine and provider (so /stop aborts mid-stream)
    engine.set_stop_flag(stop_flag);

    // 4b. Initialize NeuroLease mesh (if enabled)
    {
        if let Some(mesh) = crate::network::HiveMesh::new() {
            let mesh = std::sync::Arc::new(mesh);
            mesh.start().await;
            engine.set_mesh(mesh);
        }
    }

    // 4c. Initialize Human P2P mesh (if enabled)
    {
        if let Some(mesh) = crate::network::human_mesh::HumanMesh::new() {
            let mesh = std::sync::Arc::new(mesh);
            mesh.start().await;
            // Store it in the engine so tools can access it later
            engine.set_human_mesh(mesh);
        }
    }

    // 5. Spawn the file server daemon (serves generated + downloaded files over HTTP)
    {
        let port: u16 = std::env::var("HIVE_FILE_SERVER_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8420);
        let token = std::env::var("HIVE_FILE_TOKEN").unwrap_or_default();

        // Kill any stale process hogging the port from a previous run
        if let Ok(output) = std::process::Command::new("lsof")
            .args(["-ti", &format!(":{}", port)])
            .output()
        {
            let pids = String::from_utf8_lossy(&output.stdout);
            for pid in pids.split_whitespace() {
                let _ = std::process::Command::new("kill").args(["-9", pid]).output();
                tracing::info!("[FILE SERVER] 🔪 Killed stale process {} on port {}", pid, port);
            }
        }

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

    // 7b. Spawn the Apis-Book Dashboard (One-Way Mirror into AI Mesh)
    let apis_book = Arc::new(crate::network::apis_book::ApisBook::new());
    {
        crate::server::apis_book_server::spawn_apis_book_server(apis_book.clone()).await;
    }

    // 7c. Initialise the Resource Pool (web + compute sharing — enabled by default)
    let content_filter = Arc::new(crate::network::content_filter::ContentFilter::new());
    let pool_peer_id = crate::network::messages::PeerId(
        std::env::var("HIVE_MESH_CHAT_NAME").unwrap_or_else(|_| "Apis".to_string())
    );
    let pool_manager = Arc::new(crate::network::pool::PoolManager::new(pool_peer_id.clone()));
    {
        // Spawn compute relay (serves inference for the mesh)
        let relay_config = crate::network::compute_relay::ComputeRelayConfig::mesh_defaults();
        let _compute_relay = Arc::new(crate::network::compute_relay::ComputeRelay::new(
            relay_config,
            content_filter.clone(),
            pool_manager.compute_pool.clone(),
            pool_peer_id.clone(),
        ));
        tracing::info!("[POOL] 🤝 Resource pool ready — web_share={}, compute_share={}",
            pool_manager.web_share_enabled, pool_manager.compute_share_enabled);
    }

    // 7d. Spawn the SafeNet Web Proxy (with pool for mesh relay)
    {
        let proxy_config = crate::network::web_proxy::WebProxyConfig::from_env();
        crate::network::web_proxy::spawn_web_proxy(
            proxy_config,
            None, // QuicTransport passed when discovery wires it
            Some(pool_manager.clone()),
        ).await;
    }

    // 7e. Equality enforcement — no freeloading on the collective
    if !pool_manager.verify_equality() {
        tracing::error!("[SAFENET] ⛔ MESH ACCESS DENIED — sharing is disabled");
        tracing::error!("[SAFENET] You must contribute (web share or compute share) to use the mesh");
        // Do NOT spawn mesh services — this peer is disconnected
    }

    // 7f. Verify pool code integrity
    crate::network::pool::PoolManager::verify_pool_integrity();
    if crate::network::creator_key::creator_key_exists() {
        tracing::info!("[SAFENET] 🔧 Developer key detected — config guard exemption for local development");
    }

    // 7g. Spawn the Offline Mesh Monitor (store-and-forward, connectivity tracking)
    {
        let offline_mesh = Arc::new(crate::network::offline::OfflineMesh::new());
        offline_mesh.clone().spawn_monitor();
    }

    // 7h. Spawn HiveSurface — the decentralised social web platform
    {
        let post_store = Arc::new(crate::network::post_store::PostStore::new());
        crate::server::mesh_social::spawn_mesh_social_server(post_store, mesh_identity.clone()).await;
    }

    // 7i. Spawn Apis Code — the decentralised web IDE
    {
        crate::server::apis_code::spawn_apis_code_server().await;
    }

    // 7j. Spawn HiveChat — the decentralised Discord clone
    {
        crate::server::hive_chat::spawn_hive_chat_server(mesh_identity.clone()).await;
    }

    // 7k. Spawn HivePortal — the mesh homepage & app launcher
    {
        let registry = Arc::new(crate::server::hive_portal::SiteRegistry::new());
        crate::server::hive_portal::spawn_hive_portal_server(registry, mesh_identity.clone()).await;
    }

    // 7l. Spawn HIVE Bank — DeFi wallet & NFT trading cards
    {
        crate::server::hive_bank::spawn_hive_bank_server().await;
    }

    // 7m. Spawn HIVE Marketplace — Goods & Services commerce
    {
        crate::server::hive_marketplace::spawn_hive_marketplace_server().await;
    }

    // 7n. Spawn Upload Server — shared file upload endpoint for all platforms
    {
        crate::server::upload_server::spawn_upload_server(mesh_identity.clone()).await;
    }

    // 7o. Spawn File Share — mesh file sharing & discovery platform
    {
        crate::server::file_share::spawn_file_share_server(mesh_identity.clone()).await;
    }

    // 7n. Auto-open HivePortal in the default browser
    {
        let auto_open = std::env::var("HIVE_AUTO_OPEN")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);
        if auto_open {
            let portal_port: u16 = 3035; // Mesh-governed
            tokio::spawn(async move {
                // Small delay to ensure servers are bound
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let url = format!("http://127.0.0.1:{}", portal_port);
                tracing::info!("[PORTAL] 🌐 Opening mesh homepage: {}", url);
                let _ = std::process::Command::new("open").arg(&url).spawn();
            });
        }
    }

    // 8. Spawn the Native IMAP Background Inbox Listener
    {
        crate::engine::email_watcher::spawn_email_watcher(memory_store.clone()).await;
    }

    // 9. Spawn the Chronos Temporal Operations Daemon
    {
        crate::engine::chronos::spawn_chronos(memory_store.clone()).await;
    }

    // 10. Spawn periodic uptime checkpoint (saves every 5 min to prevent uptime loss on crashes/kills)
    {
        let mem = memory_store.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
            interval.tick().await; // skip first immediate tick
            loop {
                interval.tick().await;
                mem.temporal.write().await.save_uptime_checkpoint();
            }
        });
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
            // Persist identity on shutdown
            mesh_identity.persist().await;
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


