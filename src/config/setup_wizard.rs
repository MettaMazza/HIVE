/// First-Time Setup Wizard — Guided onboarding for new HIVE users.
///
/// When no `memory/core/setup_complete.json` exists, this module intercepts
/// boot and walks the user through:
///   1. Apis introduction
///   2. Hardware scan (with permission)
///   3. Model tier recommendations (min/med/max for their RAM)
///   4. Model download via Ollama API
///   5. Full .env configuration walkthrough with teaching
///
/// The wizard writes `.env` and the sentinel file, then returns control
/// to `run_app()` for normal boot.

use std::io::{self, Write, BufRead};

// ═══════════════════════════════════════════════════════════════════
// ANSI Terminal Helpers
// ═══════════════════════════════════════════════════════════════════

const GOLD: &str = "\x1b[38;2;255;170;0m";
const GREEN: &str = "\x1b[38;2;100;220;100m";
const BLUE: &str = "\x1b[38;2;100;180;255m";
const RED: &str = "\x1b[38;2;255;100;100m";
const CYAN: &str = "\x1b[36m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";
const YELLOW: &str = "\x1b[33m";

fn print_banner() {
    println!();
    println!("{GOLD}╔══════════════════════════════════════════════════════════════╗{RESET}");
    println!("{GOLD}║                                                              ║{RESET}");
    println!("{GOLD}║    {BOLD}🐝  H I V E  —  First Time Setup{RESET}{GOLD}                          ║{RESET}");
    println!("{GOLD}║    {DIM}Human Internet Viable Ecosystem{RESET}{GOLD}                            ║{RESET}");
    println!("{GOLD}║                                                              ║{RESET}");
    println!("{GOLD}╚══════════════════════════════════════════════════════════════╝{RESET}");
    println!();
}

fn print_section(title: &str) {
    println!();
    println!("{GOLD}── {BOLD}{title}{RESET} {GOLD}─────────────────────────────────────────{RESET}");
    println!();
}

fn print_apis(msg: &str) {
    println!("  {GOLD}🐝 Apis:{RESET} {msg}");
}

fn print_info(msg: &str) {
    println!("  {BLUE}ℹ{RESET}  {DIM}{msg}{RESET}");
}

fn print_success(msg: &str) {
    println!("  {GREEN}✅{RESET} {msg}");
}

fn print_warn(msg: &str) {
    println!("  {YELLOW}⚠️{RESET}  {msg}");
}

fn print_error(msg: &str) {
    println!("  {RED}❌{RESET} {msg}");
}

fn read_line() -> String {
    print!("  {GOLD}▸{RESET} ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).unwrap_or_default();
    input.trim().to_string()
}

fn ask_yes_no(question: &str, default_yes: bool) -> bool {
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    print_apis(&format!("{question} {DIM}{hint}{RESET}"));
    let input = read_line().to_lowercase();
    if input.is_empty() {
        return default_yes;
    }
    input.starts_with('y')
}

fn ask_choice(prompt: &str, options: &[&str], default: usize) -> usize {
    print_apis(prompt);
    for (i, opt) in options.iter().enumerate() {
        let marker = if i == default { &format!("{GOLD}→{RESET}") } else { " " };
        println!("    {marker} {BOLD}{}{RESET}  {opt}", i + 1);
    }
    print_info(&format!("Press Enter for default [{}], or type a number:", default + 1));
    let input = read_line();
    if input.is_empty() {
        return default;
    }
    input.parse::<usize>().unwrap_or(default + 1).saturating_sub(1).min(options.len() - 1)
}

fn ask_input(prompt: &str, default: &str) -> String {
    print_apis(&format!("{prompt} {DIM}[default: {default}]{RESET}"));
    let input = read_line();
    if input.is_empty() { default.to_string() } else { input }
}

// ═══════════════════════════════════════════════════════════════════
// Hardware Profile
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct HardwareProfile {
    cpu_model: String,
    cpu_cores: usize,
    ram_gb: f64,
    vram_gb: f64,
    arch: String,
    os: String,
    disk_free_gb: f64,
    is_apple_silicon: bool,
}

impl HardwareProfile {
    fn detect() -> Self {
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_all();
        sys.refresh_memory();

        // Prefer host hardware info passed from launch.sh (Docker sees VM, not host)
        let cpu_model = std::env::var("HIVE_HOST_CPU_MODEL").ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                sys.cpus().first()
                    .map(|c| c.brand().to_string())
                    .unwrap_or_else(|| "Unknown CPU".to_string())
            });

        let cpu_cores = std::env::var("HIVE_HOST_CPU_CORES").ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or_else(|| sys.cpus().len());

        let ram_gb = std::env::var("HIVE_HOST_RAM_GB").ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or_else(|| sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0));

        let arch = std::env::consts::ARCH.to_string();
        let os = std::env::var("HIVE_HOST_CPU_MODEL").ok()
            .filter(|s| s.contains("Apple"))
            .map(|_| "macos".to_string())
            .unwrap_or_else(|| std::env::consts::OS.to_string());

        let is_apple_silicon = os == "macos" && arch == "aarch64"
            || std::env::var("HIVE_HOST_CPU_MODEL").ok()
                .map_or(false, |s| s.contains("Apple"));

        let vram_gb = if is_apple_silicon { ram_gb } else { 0.0 };

        let disk_free_gb = sysinfo::Disks::new_with_refreshed_list()
            .list()
            .first()
            .map(|d| d.available_space() as f64 / (1024.0 * 1024.0 * 1024.0))
            .unwrap_or(0.0);

        Self { cpu_model, cpu_cores, ram_gb, vram_gb, arch, os, disk_free_gb, is_apple_silicon }
    }

    fn display(&self) {
        print_section("Hardware Scan Results");
        println!("    {BOLD}CPU:{RESET}          {} ({} cores)", self.cpu_model, self.cpu_cores);
        println!("    {BOLD}RAM:{RESET}          {:.0} GB", self.ram_gb);
        if self.is_apple_silicon {
            println!("    {BOLD}GPU:{RESET}          Unified Memory ({:.0} GB shared)", self.vram_gb);
        } else if self.vram_gb > 0.0 {
            println!("    {BOLD}VRAM:{RESET}         {:.0} GB", self.vram_gb);
        }
        println!("    {BOLD}Disk Free:{RESET}    {:.0} GB", self.disk_free_gb);
        println!("    {BOLD}OS/Arch:{RESET}      {} / {}", self.os, self.arch);
    }
}

// ═══════════════════════════════════════════════════════════════════
// Model Tier System
// ═══════════════════════════════════════════════════════════════════

/// Qwen 3.5 model sizes on Ollama (disk/VRAM in GB)
/// 0.8b=1.0, 2b=2.7, 4b=3.4, 9b=6.6, 27b=17, 35b=24, 122b=81
/// nomic-embed-text ≈ 0.3 GB

#[derive(Debug, Clone)]
struct ModelTier {
    name: String,
    emoji: String,
    description: String,
    main_model: String,
    observer_model: String,
    deep_model: String,
    glasses_model: String,
    embed_model: String,
    router_model: String,
    low_model: String,
    medium_model: String,
    high_model: String,
    estimated_vram_gb: f64,
    estimated_disk_gb: f64,
}

impl ModelTier {
    fn display(&self, index: usize) {
        println!();
        println!("    {BOLD}{} Option {} — {}{RESET}", self.emoji, index + 1, self.name);
        println!("    {DIM}{}:{RESET}", self.description);
        println!("      Main model:      {GREEN}{}{RESET}", self.main_model);
        println!("      Observer:         {CYAN}{}{RESET}", self.observer_model);
        println!("      Deep Think:      {BLUE}{}{RESET}", self.deep_model);
        println!("      Glasses (voice): {DIM}{}{RESET}", self.glasses_model);
        println!("      Embeddings:      {DIM}{}{RESET}", self.embed_model);
        println!("      {DIM}VRAM needed: ~{:.0} GB | Disk: ~{:.0} GB{RESET}", self.estimated_vram_gb, self.estimated_disk_gb);
    }
}

fn calculate_tiers(hw: &HardwareProfile) -> Vec<ModelTier> {
    let ram = hw.ram_gb;

    // Helper to build a tier
    let tier = |name: &str, emoji: &str, desc: &str,
                main: &str, observer: &str, deep: &str, glasses: &str,
                vram: f64, disk: f64| -> ModelTier {
        ModelTier {
            name: name.into(), emoji: emoji.into(), description: desc.into(),
            main_model: format!("qwen3.5:{main}"),
            observer_model: format!("qwen3.5:{observer}"),
            deep_model: format!("qwen3.5:{deep}"),
            glasses_model: format!("qwen3.5:{glasses}"),
            embed_model: "nomic-embed-text".into(),
            router_model: format!("qwen3.5:{observer}"),
            low_model: format!("qwen3.5:{}", if observer == "0.8b" { "0.8b" } else { observer }),
            medium_model: format!("qwen3.5:{main}"),
            high_model: format!("qwen3.5:{deep}"),
            estimated_vram_gb: vram,
            estimated_disk_gb: disk,
        }
    };

    if ram < 8.0 {
        // Under 8GB — very constrained
        vec![
            tier("Micro",   "🟢", "Tiny models — will run but limited capability",
                 "0.8b", "0.8b", "2b", "0.8b", 4.0, 5.0),
            tier("Light",   "🟡", "Small models — decent for basic tasks",
                 "2b", "0.8b", "4b", "0.8b", 6.0, 8.0),
            tier("Stretch", "🔴", "Pushing your RAM — may be slow",
                 "4b", "0.8b", "4b", "0.8b", 7.0, 8.0),
        ]
    } else if ram < 16.0 {
        // 8–15 GB
        vec![
            tier("Conservative", "🟢", "Comfortable with room for OS overhead",
                 "2b", "0.8b", "4b", "0.8b", 6.0, 8.0),
            tier("Balanced",     "🟡", "Good performance for everyday use",
                 "4b", "2b", "9b", "0.8b", 10.0, 14.0),
            tier("Full",         "🔴", "Uses most of your RAM — close to the limit",
                 "9b", "0.8b", "9b", "0.8b", 13.0, 14.0),
        ]
    } else if ram < 32.0 {
        // 16–31 GB
        vec![
            tier("Conservative", "🟢", "Smooth sailing with headroom",
                 "4b", "2b", "9b", "2b", 10.0, 16.0),
            tier("Balanced",     "🟡", "Strong performance for most tasks",
                 "9b", "4b", "9b", "2b", 14.0, 16.0),
            tier("Power",        "🔴", "Pushes your 16GB — best quality possible",
                 "9b", "4b", "27b", "2b", 20.0, 27.0),
        ]
    } else if ram < 64.0 {
        // 32–63 GB
        vec![
            tier("Conservative", "🟢", "Plenty of headroom for multitasking",
                 "9b", "4b", "9b", "4b", 14.0, 17.0),
            tier("Balanced",     "🟡", "Strong across all tasks",
                 "27b", "4b", "27b", "4b", 30.0, 38.0),
            tier("Power",        "🔴", "Near-max utilisation — excellent quality",
                 "35b", "9b", "27b", "4b", 42.0, 52.0),
        ]
    } else if ram < 128.0 {
        // 64–127 GB
        vec![
            tier("Conservative", "🟢", "Comfortable with large models",
                 "27b", "4b", "27b", "4b", 30.0, 38.0),
            tier("Balanced",     "🟡", "Premium quality for demanding work",
                 "35b", "9b", "35b", "4b", 48.0, 55.0),
            tier("Power",        "🔴", "Maximum quality your system supports",
                 "35b", "9b", "122b", "9b", 100.0, 112.0),
        ]
    } else if ram < 256.0 {
        // 128–255 GB
        vec![
            tier("Conservative", "🟢", "Plenty of headroom even with the big model",
                 "35b", "9b", "35b", "9b", 48.0, 55.0),
            tier("Balanced",     "🟡", "122B for deep thinking — very powerful",
                 "35b", "9b", "122b", "9b", 100.0, 112.0),
            tier("Power",        "🔴", "Full 122B as primary — flagship experience",
                 "122b", "9b", "122b", "9b", 162.0, 170.0),
        ]
    } else {
        // 256+ GB (e.g., M3 Ultra 512GB)
        vec![
            tier("Conservative", "🟢", "122B primary with lightweight support models",
                 "122b", "9b", "122b", "9b", 162.0, 170.0),
            tier("Balanced",     "🟡", "122B primary with 27B support chain",
                 "122b", "27b", "122b", "27b", 180.0, 200.0),
            tier("Maximum",      "🔴", "Flagship — 122B everywhere that matters",
                 "122b", "35b", "122b", "35b", 210.0, 215.0),
        ]
    }
}

/// Returns the unique set of models to pull for a given tier.
fn models_to_pull(tier: &ModelTier) -> Vec<String> {
    let mut models = vec![
        tier.main_model.clone(),
        tier.observer_model.clone(),
        tier.deep_model.clone(),
        tier.glasses_model.clone(),
        tier.embed_model.clone(),
    ];
    models.sort();
    models.dedup();
    models
}

// ═══════════════════════════════════════════════════════════════════
// Sentinel Check
// ═══════════════════════════════════════════════════════════════════

const SENTINEL_PATH: &str = "memory/core/setup_complete.json";

/// Check if first-time setup should run.
/// Beta software — everyone goes through setup if they haven't completed it.
pub fn should_run_setup() -> bool {
    if std::path::Path::new(SENTINEL_PATH).exists() {
        // Sentinel exists — offer re-run
        return offer_rerun();
    }
    // No sentinel = hasn't done setup yet. Run it.
    true
}

/// If the sentinel exists, ask the user if they want to re-run setup.
fn offer_rerun() -> bool {
    // Only offer if launched interactively (not in Docker)
    if std::path::Path::new("/.dockerenv").exists() {
        return false;
    }
    println!();
    print_apis("I see you've already completed setup before.");
    if ask_yes_no("Would you like to re-run the setup wizard?", false) {
        let _ = std::fs::remove_file(SENTINEL_PATH);
        print_success("Setup reset! Starting fresh...");
        true
    } else {
        false
    }
}

// ═══════════════════════════════════════════════════════════════════
// Model Pulling (Ollama API)
// ═══════════════════════════════════════════════════════════════════

async fn pull_model(model: &str) -> bool {
    let base_url = std::env::var("HIVE_OLLAMA_URL")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());

    // First check if model already exists
    if let Ok(resp) = reqwest::get(format!("{}/api/tags", base_url)).await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(models) = json.get("models").and_then(|m| m.as_array()) {
                if models.iter().any(|m| m.get("name").and_then(|n| n.as_str()) == Some(model)) {
                    print_success(&format!("{model} — already downloaded"));
                    return true;
                }
            }
        }
    }

    print_apis(&format!("Downloading {BOLD}{model}{RESET}..."));

    let client = reqwest::Client::new();
    let resp = client.post(format!("{}/api/pull", base_url))
        .json(&serde_json::json!({"name": model, "stream": true}))
        .send()
        .await;

    match resp {
        Ok(mut response) => {
            let mut last_pct: i64 = -1;
            let mut buf = String::new();
            while let Ok(Some(chunk)) = response.chunk().await {
                buf.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(nl) = buf.find('\n') {
                    let line: String = buf.drain(..=nl).collect();
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                        if let Some(total) = json.get("total").and_then(|t| t.as_u64()) {
                            if let Some(completed) = json.get("completed").and_then(|c| c.as_u64()) {
                                let pct = (completed as f64 / total as f64 * 100.0) as i64;
                                if pct != last_pct && pct % 5 == 0 {
                                    let bar_len = (pct as usize) / 2;
                                    let bar: String = "█".repeat(bar_len);
                                    let empty: String = "░".repeat(50 - bar_len);
                                    print!("\r    {GREEN}{bar}{DIM}{empty}{RESET} {pct}%");
                                    io::stdout().flush().ok();
                                    last_pct = pct;
                                }
                            }
                        }
                        if json.get("status").and_then(|s| s.as_str()) == Some("success") {
                            println!();
                            print_success(&format!("{model} — downloaded successfully"));
                            return true;
                        }
                        if let Some(err) = json.get("error").and_then(|e| e.as_str()) {
                            println!();
                            print_error(&format!("Failed to pull {model}: {err}"));
                            return false;
                        }
                    }
                }
            }
            println!();
            print_success(&format!("{model} — download complete"));
            true
        }
        Err(e) => {
            print_error(&format!("Could not connect to Ollama: {e}"));
            print_info("Make sure Ollama is running (try: ollama serve)");
            false
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Environment Walkthrough
// ═══════════════════════════════════════════════════════════════════

struct EnvVar {
    key: &'static str,
    description: &'static str,
    teaching: &'static str,
    default: &'static str,
    category: &'static str,
    required: bool,
    secret: bool,
}

fn env_questions() -> Vec<EnvVar> {
    vec![
        // ── Discord ──
        EnvVar {
            key: "DISCORD_TOKEN", category: "Discord",
            description: "Your Discord bot token",
            teaching: "This is the secret key that lets HIVE connect to Discord as a bot. You get it from https://discord.com/developers → New Application → Bot → Token. Without this, HIVE still works via CLI and the web mesh — Discord is optional.",
            default: "", required: false, secret: true,
        },
        EnvVar {
            key: "HIVE_ADMIN_USERS", category: "Discord",
            description: "Comma-separated Discord User IDs with admin access",
            teaching: "These users can use privileged commands like /clean, /stop, and admin-only tools. To find your ID: Discord Settings → Advanced → Developer Mode → right-click your name → Copy ID.",
            default: "", required: false, secret: false,
        },
        // ── Inference Parameters ──
        EnvVar {
            key: "HIVE_MODEL_TEMPERATURE", category: "Inference",
            description: "Model creativity (0.0–2.0)",
            teaching: "Temperature controls randomness. Lower values (0.2) = more focused and deterministic. Higher values (1.0) = more creative and varied. 0.7 is a great balance between accuracy and personality.",
            default: "0.7", required: false, secret: false,
        },
        EnvVar {
            key: "HIVE_MODEL_TOP_P", category: "Inference",
            description: "Nucleus sampling threshold (0.0–1.0)",
            teaching: "Top-P filters the model's word choices to the most likely ones. 0.9 means it considers tokens that make up the top 90% of probability mass. Lower = more focused, higher = more diverse.",
            default: "0.9", required: false, secret: false,
        },
        EnvVar {
            key: "HIVE_MODEL_TOP_K", category: "Inference",
            description: "Top-K sampling limit",
            teaching: "Limits the model to choosing from the top K most likely next tokens. 40 is a solid default. Lower = safer outputs, higher = more creative.",
            default: "40", required: false, secret: false,
        },
        EnvVar {
            key: "HIVE_MODEL_REPEAT_PENALTY", category: "Inference",
            description: "Repetition penalty (1.0 = none)",
            teaching: "Penalises the model for repeating itself. 1.0 = no penalty, 1.1 = mild (recommended), 1.5 = aggressive. Too high and the model starts avoiding common words.",
            default: "1.1", required: false, secret: false,
        },
        // ── Timeouts ──
        EnvVar {
            key: "HIVE_TIMEOUT_INFERENCE_SECS", category: "Timeouts",
            description: "Max seconds for a single inference call",
            teaching: "How long to wait for the model to respond before timing out. Larger models on slower hardware need more time. 300s (5 min) is safe for most setups.",
            default: "300", required: false, secret: false,
        },
        EnvVar {
            key: "HIVE_TIMEOUT_TOOL_SECS", category: "Timeouts",
            description: "Max seconds for a single tool execution",
            teaching: "How long tools (file operations, web searches, etc.) are allowed to run. 30s prevents tools from hanging indefinitely.",
            default: "30", required: false, secret: false,
        },
        EnvVar {
            key: "HIVE_LIMIT_CONTEXT_TOKENS", category: "Timeouts",
            description: "Max tokens in context window",
            teaching: "Controls how much conversation history the model can see at once. Qwen 3.5 supports up to 256K, but 8192 keeps responses fast and focused. Increase for long research sessions.",
            default: "8192", required: false, secret: false,
        },
        // ── Permissions ──
        EnvVar {
            key: "HIVE_ALLOW_TERMINAL", category: "Permissions",
            description: "Allow Apis to run shell commands",
            teaching: "When true, Apis can execute terminal commands on your machine. This is powerful but requires trust. Admin-gating means only approved users can trigger this.",
            default: "true", required: false, secret: false,
        },
        EnvVar {
            key: "HIVE_ALLOW_FILE_SYSTEM", category: "Permissions",
            description: "Allow Apis to read/write files",
            teaching: "When true, Apis can create, read, and edit files on your system. Essential for coding tasks and generating content.",
            default: "true", required: false, secret: false,
        },
        // ── Search ──
        EnvVar {
            key: "BRAVE_SEARCH_API_KEY", category: "Search",
            description: "Brave Search API key for web searches",
            teaching: "Gives Apis the ability to search the web. Get a free key at https://brave.com/search/api/ — the free tier gives 2,000 searches/month. Without this, web search is disabled.",
            default: "", required: false, secret: true,
        },
        // ── Training ──
        EnvVar {
            key: "HIVE_SLEEP_INTERVAL_SECS", category: "Training",
            description: "Seconds between self-training cycles",
            teaching: "HIVE can learn from its own conversations during 'sleep' cycles. This sets how often it checks for new training data. 300s (5 min) is the default. Set higher to reduce CPU usage.",
            default: "300", required: false, secret: false,
        },
    ]
}

// ═══════════════════════════════════════════════════════════════════
// Main Wizard Orchestrator
// ═══════════════════════════════════════════════════════════════════

pub async fn run_setup_wizard() {
    print_banner();

    // ── Phase 1: Hello ──────────────────────────────────────────
    print_section("Welcome");
    print_apis(&format!("Hello! {BOLD}I'm Apis{RESET} — your AI companion inside the HIVE engine."));
    println!();
    print_apis("I'm going to help you get everything set up. This will only take a few minutes.");
    print_apis("I'll scan your hardware, recommend the best AI models for your system,");
    print_apis("download them, and configure your environment — all while teaching you");
    print_apis("what each setting does.");
    println!();
    print_info("You can press Enter to accept defaults at any point. Minimal effort required!");
    println!();

    if !ask_yes_no("Ready to begin?", true) {
        print_apis("No worries! I'll be here when you're ready. Booting with defaults...");
        write_sentinel(None);
        return;
    }

    // ── Phase 2: Hardware Scan ──────────────────────────────────
    print_section("Hardware Scan");
    print_apis("First, I'd like to scan your hardware so I can recommend the best");
    print_apis("AI models for your system. This checks CPU, RAM, and disk space.");
    print_info("Nothing leaves your machine — this is 100% local.");
    println!();

    let hw = if ask_yes_no("May I scan your hardware?", true) {
        let hw = HardwareProfile::detect();
        hw.display();
        println!();
        print_success("Hardware scan complete!");
        Some(hw)
    } else {
        print_apis("No problem! I'll use conservative defaults.");
        None
    };

    // ── Phase 3: Model Recommendations ──────────────────────────
    print_section("Model Selection");

    let selected_tier = if let Some(ref hw) = hw {
        let tiers = calculate_tiers(hw);

        print_apis(&format!("Based on your {BOLD}{:.0} GB RAM{RESET}, here are three configurations:", hw.ram_gb));
        print_apis("Each uses different Qwen 3.5 model sizes — bigger = smarter but slower.");
        println!();

        for (i, t) in tiers.iter().enumerate() {
            t.display(i);
        }

        println!();
        let choice = ask_choice(
            "Which configuration would you like?",
            &["Conservative — safe, fast, plenty of headroom",
              "Balanced — recommended for most users",
              "Power — maximum quality, uses more resources"],
            1, // Default to balanced
        );

        print_success(&format!("Selected: {} — {}", tiers[choice].emoji, tiers[choice].name));
        Some(tiers[choice].clone())
    } else {
        // No hardware scan — use safe defaults
        let default = ModelTier {
            name: "Default".into(), emoji: "🟡".into(),
            description: "Safe defaults without hardware scan".into(),
            main_model: "qwen3.5:9b".into(),
            observer_model: "qwen3.5:4b".into(),
            deep_model: "qwen3.5:9b".into(),
            glasses_model: "qwen3.5:2b".into(),
            embed_model: "nomic-embed-text".into(),
            router_model: "qwen3.5:4b".into(),
            low_model: "qwen3.5:4b".into(),
            medium_model: "qwen3.5:9b".into(),
            high_model: "qwen3.5:9b".into(),
            estimated_vram_gb: 14.0,
            estimated_disk_gb: 16.0,
        };
        Some(default)
    };

    // ── Phase 4: Model Download ─────────────────────────────────
    if let Some(ref tier) = selected_tier {
        print_section("Model Download");
        let to_pull = models_to_pull(tier);

        print_apis(&format!("I need to download {} model(s). This may take a while depending", to_pull.len()));
        print_apis("on your internet speed — some models are quite large.");
        println!();

        if ask_yes_no("Shall I download the models now?", true) {
            for model in &to_pull {
                pull_model(model).await;
            }
            println!();
            print_success("All models ready!");
        } else {
            print_apis("No worries — you can pull them later with: ollama pull <model_name>");
        }
    }

    // ── Phase 5: Environment Walkthrough ────────────────────────
    print_section("Environment Configuration");
    print_apis("Now let's configure your HIVE environment. I'll walk you through");
    print_apis("each setting, explain what it does, and you can customise or accept defaults.");
    println!();

    let auto_mode = ask_yes_no("Want me to auto-configure everything? (You can still review each setting)", true);

    let mut env_values: Vec<(String, String)> = Vec::new();

    // Add model settings from the selected tier
    if let Some(ref tier) = selected_tier {
        env_values.push(("HIVE_PROVIDER".into(), "ollama".into()));
        env_values.push(("HIVE_MODEL".into(), tier.main_model.clone()));
        env_values.push(("HIVE_OBSERVER_MODEL".into(), tier.observer_model.clone()));
        env_values.push(("HIVE_DEEP_MODEL".into(), tier.deep_model.clone()));
        env_values.push(("HIVE_GLASSES_MODEL".into(), tier.glasses_model.clone()));
        env_values.push(("HIVE_EMBED_MODEL".into(), tier.embed_model.clone()));
        env_values.push(("HIVE_ROUTER_MODEL".into(), tier.router_model.clone()));
        env_values.push(("HIVE_LOW_MODEL".into(), tier.low_model.clone()));
        env_values.push(("HIVE_MEDIUM_MODEL".into(), tier.medium_model.clone()));
        env_values.push(("HIVE_HIGH_MODEL".into(), tier.high_model.clone()));
    }

    let questions = env_questions();
    let mut current_category = "";

    for q in &questions {
        if q.category != current_category {
            print_section(&format!("⚙️  {}", q.category));
            current_category = q.category;
        }

        println!("    {BOLD}{}{RESET}", q.key);
        println!("    {DIM}{}{RESET}", q.teaching);
        println!();

        let value = if auto_mode && !q.required && !q.secret {
            print_info(&format!("Auto-set to: {}", if q.default.is_empty() { "(skipped)" } else { q.default }));
            q.default.to_string()
        } else if q.secret && q.default.is_empty() {
            // Always ask for secrets — but make optional
            let v = ask_input(&format!("{} (leave blank to skip):", q.description), "");
            v
        } else {
            ask_input(q.description, q.default)
        };

        if !value.is_empty() {
            env_values.push((q.key.to_string(), value));
        }
    }

    // Add standard defaults that don't need user interaction
    let auto_defaults = vec![
        ("HIVE_SERIAL_INFERENCE", "true"),
        ("HIVE_STRICT_SAFEGUARDS", "true"),
        ("HIVE_ALLOW_REFUSAL", "true"),
        ("HIVE_STORAGE_ROOT", "memory"),
        ("HIVE_WORKING_MEMORY_CAP", "100"),
        ("HIVE_HISTORY_MSG_CAP", "1000000"),
        ("HIVE_LIMIT_GENERATION_TOKENS", "4096"),
        ("HIVE_FILE_SERVER_PORT", "8421"),
        ("HIVE_FILE_TOKEN", "hive_admin_2026"),
        ("HIVE_ROUTER_ENABLED", "false"),
        ("NEUROLEASE_ENABLED", "true"),
        ("HIVE_CRYPTO_SIMULATION", "true"),
        ("HIVE_TRAINING_BACKEND", "auto"),
    ];

    for (k, v) in &auto_defaults {
        if !env_values.iter().any(|(key, _)| key == k) {
            env_values.push((k.to_string(), v.to_string()));
        }
    }

    // ── Phase 6: Write .env & Finalize ──────────────────────────
    print_section("Finalizing");
    print_apis("Writing your configuration...");

    write_env_file(&env_values);
    write_sentinel(hw.as_ref());

    println!();
    print_success("Configuration saved to .env");
    print_success("Setup complete!");
    println!();
    print_apis(&format!("{BOLD}Welcome to the HIVE. 🐝{RESET}"));
    print_apis("Your engine is about to boot for the first time.");
    print_apis("If you ever want to re-run this wizard, just launch HIVE again —");
    print_apis("I'll ask if you'd like to start fresh.");
    println!();

    println!("{GOLD}═══════════════════════════════════════════════════════════════{RESET}");
    println!("{GOLD}  Summary:{RESET}");
    if let Some(ref tier) = selected_tier {
        println!("    Main Model:   {GREEN}{}{RESET}", tier.main_model);
        println!("    Observer:     {CYAN}{}{RESET}", tier.observer_model);
        println!("    Deep Think:   {BLUE}{}{RESET}", tier.deep_model);
        println!("    Glasses:      {DIM}{}{RESET}", tier.glasses_model);
        println!("    Embeddings:   {DIM}{}{RESET}", tier.embed_model);
    }
    println!("{GOLD}═══════════════════════════════════════════════════════════════{RESET}");
    println!();

    // ── Phase 7: Onboarding Platform Choice ─────────────────────
    // Ask the user where they'd like to do the interactive onboarding.
    // If Discord is configured, offer it as an option; otherwise default to CLI.
    let has_discord = env_values.iter().any(|(k, v)| k == "DISCORD_TOKEN" && !v.is_empty());

    if has_discord {
        print_section("Onboarding Location");
        print_apis("Next up is the onboarding experience — where I'll introduce myself,");
        print_apis("show you my systems, learn about you, and let you name me.");
        println!();
        print_apis("Since you've configured Discord, you can choose where to do this:");
        println!();

        let choice = ask_choice(
            "Where would you like to meet me?",
            &["Right here in the terminal — let's keep going",
              "On Discord — I'll message you there when I'm ready"],
            1, // Default to Discord (the richer experience)
        );

        let platform = if choice == 0 { "cli" } else { "discord" };

        // Persist the choice so the engine knows where to inject the welcome event
        let _ = std::fs::create_dir_all("memory/core");
        let platform_json = serde_json::json!({
            "platform": platform,
            "chosen_at": chrono::Utc::now().to_rfc3339(),
        });
        let _ = std::fs::write(
            "memory/core/onboarding_platform.json",
            serde_json::to_string_pretty(&platform_json).unwrap_or_default(),
        );

        if platform == "discord" {
            print_success("I'll meet you on Discord once I've finished booting. 🐝");
        } else {
            print_success("I'll greet you right here in the terminal. 🐝");
        }
    } else {
        // No Discord — default to CLI silently
        let _ = std::fs::create_dir_all("memory/core");
        let platform_json = serde_json::json!({
            "platform": "cli",
            "chosen_at": chrono::Utc::now().to_rfc3339(),
        });
        let _ = std::fs::write(
            "memory/core/onboarding_platform.json",
            serde_json::to_string_pretty(&platform_json).unwrap_or_default(),
        );
    }

    println!();
    print_apis(&format!("{BOLD}Booting the HIVE Engine...{RESET}"));
    println!();
}

fn write_env_file(values: &[(String, String)]) {
    let mut content = String::new();
    content.push_str("# ═══════════════════════════════════════════════════════════════\n");
    content.push_str("# HIVE Engine — Generated by Apis Setup Wizard\n");
    content.push_str(&format!("# Generated: {}\n", chrono::Utc::now().to_rfc3339()));
    content.push_str("# ═══════════════════════════════════════════════════════════════\n\n");

    // Group by known categories
    let groups: Vec<(&str, Vec<&str>)> = vec![
        ("Discord", vec!["DISCORD_TOKEN", "HIVE_ADMIN_USERS", "HIVE_TARGET_CHANNEL", "HIVE_CHAT_CHANNEL"]),
        ("Search", vec!["BRAVE_SEARCH_API_KEY"]),
        ("Provider & Models", vec!["HIVE_PROVIDER", "HIVE_MODEL", "HIVE_OBSERVER_MODEL", "HIVE_DEEP_MODEL",
            "HIVE_GLASSES_MODEL", "HIVE_EMBED_MODEL"]),
        ("Reasoning Router", vec!["HIVE_ROUTER_ENABLED", "HIVE_ROUTER_MODEL", "HIVE_LOW_MODEL",
            "HIVE_MEDIUM_MODEL", "HIVE_HIGH_MODEL"]),
        ("Inference Parameters", vec!["HIVE_MODEL_TEMPERATURE", "HIVE_MODEL_TOP_P",
            "HIVE_MODEL_TOP_K", "HIVE_MODEL_REPEAT_PENALTY", "HIVE_SERIAL_INFERENCE"]),
        ("Engine Limits & Timeouts", vec!["HIVE_TIMEOUT_INFERENCE_SECS", "HIVE_TIMEOUT_TOOL_SECS",
            "HIVE_TIMEOUT_COMPILE_SECS", "HIVE_LIMIT_CONTEXT_TOKENS", "HIVE_LIMIT_GENERATION_TOKENS",
            "HIVE_WORKING_MEMORY_CAP", "HIVE_HISTORY_MSG_CAP"]),
        ("Permissions", vec!["HIVE_ALLOW_TERMINAL", "HIVE_ALLOW_FILE_SYSTEM",
            "HIVE_ALLOW_REFUSAL", "HIVE_STRICT_SAFEGUARDS"]),
        ("Storage", vec!["HIVE_STORAGE_ROOT"]),
        ("Training", vec!["HIVE_TRAINING_BACKEND", "HIVE_SLEEP_INTERVAL_SECS"]),
        ("File Server", vec!["HIVE_FILE_SERVER_PORT", "HIVE_FILE_TOKEN"]),
        ("SafeNet", vec!["NEUROLEASE_ENABLED", "HIVE_CRYPTO_SIMULATION"]),
    ];

    let mut written: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (section, keys) in &groups {
        let section_vals: Vec<_> = keys.iter()
            .filter_map(|k| values.iter().find(|(key, _)| key == k).map(|(key, val)| (key.clone(), val.clone())))
            .collect();

        if section_vals.is_empty() { continue; }

        content.push_str(&format!("# ── {} ──────────────────────────────────────────────────\n", section));
        for (k, v) in &section_vals {
            if v.contains(' ') || v.contains('#') || v.contains('\"') {
                content.push_str(&format!("{}=\"{}\"\n", k, v));
            } else {
                content.push_str(&format!("{}={}\n", k, v));
            }
            written.insert(k.clone());
        }
        content.push('\n');
    }

    // Write any remaining values not in groups
    let remaining: Vec<_> = values.iter().filter(|(k, _)| !written.contains(k)).collect();
    if !remaining.is_empty() {
        content.push_str("# ── Other ──────────────────────────────────────────────────\n");
        for (k, v) in remaining {
            content.push_str(&format!("{}={}\n", k, v));
        }
        content.push('\n');
    }

    // Append mesh-governed notice
    content.push_str("# ═══════════════════════════════════════════════════════════════\n");
    content.push_str("# MESH-GOVERNED (HARDCODED — DO NOT ADD ENV VARS FOR THESE)\n");
    content.push_str("# ═══════════════════════════════════════════════════════════════\n");
    content.push_str("# Ports, economy, queue, pooling, content, offline settings are\n");
    content.push_str("# compiled into the binary. Changes require source code edit.\n");

    std::fs::write(".env", &content).unwrap_or_else(|e| {
        print_error(&format!("Failed to write .env: {e}"));
    });
}

fn write_sentinel(hw: Option<&HardwareProfile>) {
    let _ = std::fs::create_dir_all("memory/core");
    let sentinel = serde_json::json!({
        "completed_at": chrono::Utc::now().to_rfc3339(),
        "version": "1.0",
        "hardware": hw.map(|h| serde_json::json!({
            "cpu": h.cpu_model,
            "cores": h.cpu_cores,
            "ram_gb": h.ram_gb,
            "arch": h.arch,
            "os": h.os,
        })),
    });
    std::fs::write(SENTINEL_PATH, serde_json::to_string_pretty(&sentinel).unwrap_or_default())
        .unwrap_or_else(|e| {
            print_error(&format!("Failed to write sentinel: {e}"));
        });
}
