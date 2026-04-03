use std::io::{self, Write, BufRead};

// ── ANSI Color Helpers ──────────────────────────────────────────
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";
const WHITE: &str = "\x1b[97m";

// ── Box Drawing ─────────────────────────────────────────────────
fn print_header() {
    println!();
    println!("{CYAN}╔══════════════════════════════════════════════════════╗{RESET}");
    println!("{CYAN}║{RESET}  {BOLD}{WHITE}🐝  W E L C O M E   T O   H I V E{RESET}                   {CYAN}║{RESET}");
    println!("{CYAN}║{RESET}  {DIM}The Locally Sovereign AI Engine{RESET}                     {CYAN}║{RESET}");
    println!("{CYAN}╠══════════════════════════════════════════════════════╣{RESET}");
    println!("{CYAN}║{RESET}                                                      {CYAN}║{RESET}");
    println!("{CYAN}║{RESET}  {DIM}This wizard will configure your environment.{RESET}        {CYAN}║{RESET}");
    println!("{CYAN}║{RESET}  {DIM}Every step is optional — press Enter to skip.{RESET}       {CYAN}║{RESET}");
    println!("{CYAN}║{RESET}                                                      {CYAN}║{RESET}");
    println!("{CYAN}╚══════════════════════════════════════════════════════╝{RESET}");
    println!();
}

fn print_step(number: u8, title: &str) {
    println!();
    println!("{MAGENTA}  ┌──────────────────────────────────────────────────┐{RESET}");
    println!("{MAGENTA}  │{RESET}  {BOLD}Step {number}{RESET}: {WHITE}{title}{RESET}");
    println!("{MAGENTA}  └──────────────────────────────────────────────────┘{RESET}");
}

fn prompt(question: &str, default: &str) -> String {
    if default.is_empty() {
        print!("  {GREEN}▸{RESET} {question}: ");
    } else {
        print!("  {GREEN}▸{RESET} {question} {DIM}[{default}]{RESET}: ");
    }
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).unwrap_or(0);
    let trimmed = input.trim().to_string();
    if trimmed.is_empty() { default.to_string() } else { trimmed }
}

fn prompt_yn(question: &str, default_yes: bool) -> bool {
    let hint = if default_yes { "Y/n" } else { "y/N" };
    let answer = prompt(&format!("{question} [{hint}]"), "");
    if answer.is_empty() {
        default_yes
    } else {
        answer.to_lowercase().starts_with('y')
    }
}

fn print_ok(msg: &str) {
    println!("  {GREEN}✓{RESET} {msg}");
}

fn print_info(msg: &str) {
    println!("  {DIM}{msg}{RESET}");
}

// ── Model Tier Tables ───────────────────────────────────────────

struct ModelTier {
    main: &'static str,
    observer: &'static str,
    deep: &'static str,
}

fn gemma4_tier(ram_gb: u64) -> ModelTier {
    match ram_gb {
        0..=15 => ModelTier { main: "gemma4:e2b", observer: "gemma4:e2b", deep: "gemma4:e2b" },
        16..=31 => ModelTier { main: "gemma4:e4b", observer: "gemma4:e2b", deep: "gemma4:e4b" },
        32..=95 => ModelTier { main: "gemma4:26b", observer: "gemma4:e2b", deep: "gemma4:26b" },
        96..=255 => ModelTier { main: "gemma4:26b", observer: "gemma4:e2b", deep: "gemma4:31b" },
        _ => ModelTier { main: "gemma4:26b", observer: "gemma4:e2b", deep: "gemma4:31b" },
    }
}

fn qwen35_tier(ram_gb: u64) -> ModelTier {
    match ram_gb {
        0..=15 => ModelTier { main: "qwen3.5:2b", observer: "qwen3.5:0.8b", deep: "qwen3.5:2b" },
        16..=31 => ModelTier { main: "qwen3.5:9b", observer: "qwen3.5:2b", deep: "qwen3.5:9b" },
        32..=95 => ModelTier { main: "qwen3.5:27b", observer: "qwen3.5:2b", deep: "qwen3.5:35b" },
        96..=255 => ModelTier { main: "qwen3.5:35b", observer: "qwen3.5:9b", deep: "qwen3.5:122b" },
        _ => ModelTier { main: "qwen3.5:122b", observer: "qwen3.5:9b", deep: "qwen3.5:122b" },
    }
}

// ── Hardware Detection ──────────────────────────────────────────

fn detect_ram_gb() -> Option<u64> {
    // macOS: sysctl hw.memsize
    if let Ok(output) = std::process::Command::new("sysctl")
        .arg("-n")
        .arg("hw.memsize")
        .output()
    {
        if output.status.success() {
            if let Ok(bytes_str) = String::from_utf8(output.stdout) {
                if let Ok(bytes) = bytes_str.trim().parse::<u64>() {
                    return Some(bytes / (1024 * 1024 * 1024));
                }
            }
        }
    }
    // Linux: /proc/meminfo
    if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(kb_str) = parts.get(1) {
                    if let Ok(kb) = kb_str.parse::<u64>() {
                        return Some(kb / (1024 * 1024));
                    }
                }
            }
        }
    }
    None
}

fn detect_cpu_info() -> String {
    // macOS
    if let Ok(output) = std::process::Command::new("sysctl")
        .arg("-n")
        .arg("machdep.cpu.brand_string")
        .output()
    {
        if output.status.success() {
            if let Ok(s) = String::from_utf8(output.stdout) {
                let trimmed = s.trim().to_string();
                if !trimmed.is_empty() {
                    return trimmed;
                }
            }
        }
    }
    // Apple Silicon (M-series)
    if let Ok(output) = std::process::Command::new("sysctl")
        .arg("-n")
        .arg("hw.model")
        .output()
    {
        if output.status.success() {
            if let Ok(s) = String::from_utf8(output.stdout) {
                return s.trim().to_string();
            }
        }
    }
    "Unknown CPU".to_string()
}

// ── .env Generation ─────────────────────────────────────────────

struct EnvConfig {
    discord_token: String,
    admin_users: String,
    target_channel: String,
    chat_channel: String,
    brave_api_key: String,
    smtp_user: String,
    smtp_pass: String,
    smtp_host: String,
    smtp_port: String,
    imap_user: String,
    imap_pass: String,
    imap_host: String,
    imap_port: String,
    main_model: String,
    observer_model: String,
    deep_model: String,
    glasses_model: String,
}

impl EnvConfig {
    fn defaults() -> Self {
        Self {
            discord_token: String::new(),
            admin_users: String::new(),
            target_channel: String::new(),
            chat_channel: String::new(),
            brave_api_key: String::new(),
            smtp_user: String::new(),
            smtp_pass: String::new(),
            smtp_host: "smtp.gmail.com".into(),
            smtp_port: "587".into(),
            imap_user: String::new(),
            imap_pass: String::new(),
            imap_host: "imap.gmail.com".into(),
            imap_port: "993".into(),
            main_model: "gemma4:e2b".into(),
            observer_model: "gemma4:e2b".into(),
            deep_model: "gemma4:e2b".into(),
            glasses_model: "gemma4:e2b".into(),
        }
    }

    fn generate_env(&self) -> String {
        format!(r#"# ═══════════════════════════════════════════════════════════════
# HIVE Engine — Live Configuration
# Generated by Setup Wizard
# ═══════════════════════════════════════════════════════════════

# ── Discord ──────────────────────────────────────────────────
DISCORD_TOKEN="{}"
HIVE_ADMIN_USERS={}
HIVE_TARGET_CHANNEL={}
HIVE_CHAT_CHANNEL={}
# HIVE_WELCOME_CHANNEL=

# ── Search ───────────────────────────────────────────────────
BRAVE_SEARCH_API_KEY="{}"

# ── Email ────────────────────────────────────────────────────
SMTP_USER="{}"
SMTP_PASS="{}"
SMTP_HOST="{}"
SMTP_PORT="{}"
IMAP_USER="{}"
IMAP_PASS="{}"
IMAP_HOST="{}"
IMAP_PORT="{}"

# ── Provider & Models ────────────────────────────────────────
HIVE_PROVIDER=ollama
# HIVE_OLLAMA_URL=http://localhost:11434
HIVE_MODEL={}
HIVE_OBSERVER_MODEL={}
HIVE_DEEP_MODEL={}
HIVE_GLASSES_MODEL={}
HIVE_EMBED_MODEL=nomic-embed-text
# HIVE_MODEL_PULL=false

# ── Reasoning Router ─────────────────────────────────────────
HIVE_ROUTER_ENABLED=false
HIVE_ROUTER_MODEL={}
HIVE_LOW_MODEL={}
HIVE_MEDIUM_MODEL={}
HIVE_HIGH_MODEL={}

# ── Inference Parameters ─────────────────────────────────────
HIVE_MODEL_TEMPERATURE=0.7
HIVE_MODEL_TOP_P=0.9
HIVE_MODEL_TOP_K=40
HIVE_MODEL_REPEAT_PENALTY=1.1
HIVE_SERIAL_INFERENCE=false
HIVE_MAX_PARALLEL=16

# ── Engine Limits & Timeouts ─────────────────────────────────
HIVE_TIMEOUT_INFERENCE_SECS=300
HIVE_TIMEOUT_TOOL_SECS=30
HIVE_TIMEOUT_COMPILE_SECS=15
HIVE_LIMIT_CONTEXT_TOKENS=8192
HIVE_LIMIT_GENERATION_TOKENS=4096
HIVE_WORKING_MEMORY_CAP=100
HIVE_HISTORY_MSG_CAP=1000000

# ── Permissions ──────────────────────────────────────────────
HIVE_ALLOW_TERMINAL=true
HIVE_ALLOW_FILE_SYSTEM=true
HIVE_ALLOW_REFUSAL=true
HIVE_STRICT_SAFEGUARDS=true

# ── Storage & Directories ───────────────────────────────────
HIVE_STORAGE_ROOT=memory
# HIVE_WORKSPACE_DIR=.
# HIVE_ARTIFACTS_DIR=artifacts
# HIVE_LOGS_DIR=logs
# HIVE_DOWNLOADS_DIR=/tmp/hive
# HIVE_CACHE_DIR=memory/cache/images
# HIVE_PROJECT_DIR=.

# ── Python & Training ───────────────────────────────────────
# HIVE_PYTHON_BIN=python3
HIVE_TRAINING_BACKEND=auto
HIVE_SLEEP_INTERVAL_SECS=300

# ── Persona & Identity ──────────────────────────────────────
# Persona is edited through the kernel-protected system.

# ── File Server ──────────────────────────────────────────────
HIVE_FILE_SERVER_PORT=8421
HIVE_FILE_TOKEN=hive_admin_2026

# ── Glasses (WebSocket) ─────────────────────────────────────
# HIVE_GLASSES_PORT=8421
# HIVE_GLASSES_TOKEN=

# ── UI & Dashboards ─────────────────────────────────────────
# HIVE_UI_TITLE=HIVE Mesh
# HIVE_UI_THEME_COLOR=#ff9900
# HIVE_AUTO_OPEN=false

# ═══════════════════════════════════════════════════════════════
# SafeNet P2P Mesh
# ═══════════════════════════════════════════════════════════════

NEUROLEASE_ENABLED=true
HIVE_CRYPTO_SIMULATION=true
"#,
            self.discord_token,
            self.admin_users,
            self.target_channel,
            self.chat_channel,
            self.brave_api_key,
            self.smtp_user, self.smtp_pass, self.smtp_host, self.smtp_port,
            self.imap_user, self.imap_pass, self.imap_host, self.imap_port,
            self.main_model,
            self.observer_model,
            self.deep_model,
            self.glasses_model,
            // Router models (use observer as low, main as medium, deep as high)
            self.observer_model,
            self.observer_model,
            self.main_model,
            self.deep_model,
        )
    }
}

// ── Main Entry Points ───────────────────────────────────────────

/// Run the wizard with all defaults (non-interactive mode)
pub fn run_defaults() {
    let config = EnvConfig::defaults();
    write_env(&config);
    eprintln!("[SETUP] Generated .env with safe defaults (gemma4:e2b)");
}

/// Run the full interactive wizard
pub fn run() {
    print_header();

    let mut config = EnvConfig::defaults();
    let mut ram_gb: u64 = 0;

    // ── Step 1: Hardware Detection ──────────────────────────────
    print_step(1, "Hardware Detection");
    if prompt_yn("May I scan your hardware to recommend the best model?", true) {
        if let Some(ram) = detect_ram_gb() {
            ram_gb = ram;
            let cpu = detect_cpu_info();
            print_ok(&format!("RAM: {ram_gb} GB"));
            print_ok(&format!("CPU: {cpu}"));
        } else {
            print_info("Could not detect hardware — you can select models manually.");
        }
    } else {
        print_info("Skipped hardware detection. You can select models manually.");
    }

    // ── Step 2: Model Configuration ────────────────────────────
    print_step(2, "Model Configuration");
    println!();
    println!("  {BOLD}Choose a model family:{RESET}");
    println!("    {GREEN}[1]{RESET} Gemma 4 {DIM}(Google, recommended){RESET}");
    println!("    {GREEN}[2]{RESET} Qwen 3.5 {DIM}(Alibaba){RESET}");
    println!("    {GREEN}[3]{RESET} Browse Ollama Library {DIM}(custom model){RESET}");
    println!();

    let family = prompt("Selection", "1");

    match family.as_str() {
        "2" => {
            // Qwen 3.5 tiers
            let low  = qwen35_tier(8);
            let mid  = qwen35_tier(64);
            let high = qwen35_tier(256);

            let recommended = if ram_gb >= 128 { "3" } else if ram_gb >= 32 { "2" } else { "1" };

            println!();
            println!("  {BOLD}Choose your performance tier:{RESET}");
            println!("    {GREEN}[1]{RESET} Lightweight  {DIM}(8-16GB RAM){RESET}");
            println!("        Main: {CYAN}{}{RESET}  Observer: {CYAN}{}{RESET}  Deep: {CYAN}{}{RESET}", low.main, low.observer, low.deep);
            println!("    {GREEN}[2]{RESET} Balanced     {DIM}(32-96GB RAM){RESET}");
            println!("        Main: {CYAN}{}{RESET}  Observer: {CYAN}{}{RESET}  Deep: {CYAN}{}{RESET}", mid.main, mid.observer, mid.deep);
            println!("    {GREEN}[3]{RESET} Performance  {DIM}(128GB+ RAM){RESET}");
            println!("        Main: {CYAN}{}{RESET}  Observer: {CYAN}{}{RESET}  Deep: {CYAN}{}{RESET}", high.main, high.observer, high.deep);
            println!();
            if ram_gb > 0 {
                println!("  {DIM}Detected {ram_gb}GB RAM → recommended: [{recommended}]{RESET}");
            }

            let choice = prompt("Selection", recommended);
            let tier = match choice.as_str() {
                "1" => low,
                "3" => high,
                _   => mid,
            };
            config.main_model = tier.main.to_string();
            config.observer_model = tier.observer.to_string();
            config.deep_model = tier.deep.to_string();
            config.glasses_model = tier.observer.to_string();

            println!();
            print_ok(&format!("Main: {}  Observer: {}  Deep: {}", tier.main, tier.observer, tier.deep));
        }
        "3" => {
            println!();
            println!("  {BOLD}Browse models at:{RESET} {CYAN}https://ollama.com/library{RESET}");
            println!("  {DIM}Enter any model tag (e.g. llama3:70b, mistral:7b){RESET}");
            println!();
            config.main_model = prompt("Main model", "gemma4:e2b");
            config.observer_model = prompt("Observer model", "gemma4:e2b");
            config.deep_model = prompt("Deep model", &config.main_model);
            config.glasses_model = prompt("Glasses model", &config.observer_model);
        }
        _ => {
            // Gemma 4 tiers
            let low  = gemma4_tier(8);
            let mid  = gemma4_tier(64);
            let high = gemma4_tier(256);

            let recommended = if ram_gb >= 96 { "3" } else if ram_gb >= 32 { "2" } else { "1" };

            println!();
            println!("  {BOLD}Choose your performance tier:{RESET}");
            println!("    {GREEN}[1]{RESET} Lightweight  {DIM}(8-16GB RAM){RESET}");
            println!("        Main: {CYAN}{}{RESET}  Observer: {CYAN}{}{RESET}  Deep: {CYAN}{}{RESET}", low.main, low.observer, low.deep);
            println!("    {GREEN}[2]{RESET} Balanced     {DIM}(32-96GB RAM){RESET}");
            println!("        Main: {CYAN}{}{RESET}  Observer: {CYAN}{}{RESET}  Deep: {CYAN}{}{RESET}", mid.main, mid.observer, mid.deep);
            println!("    {GREEN}[3]{RESET} Performance  {DIM}(96GB+ RAM){RESET}");
            println!("        Main: {CYAN}{}{RESET}  Observer: {CYAN}{}{RESET}  Deep: {CYAN}{}{RESET}", high.main, high.observer, high.deep);
            println!();
            if ram_gb > 0 {
                println!("  {DIM}Detected {ram_gb}GB RAM → recommended: [{recommended}]{RESET}");
            }

            let choice = prompt("Selection", recommended);
            let tier = match choice.as_str() {
                "1" => low,
                "3" => high,
                _   => mid,
            };
            config.main_model = tier.main.to_string();
            config.observer_model = tier.observer.to_string();
            config.deep_model = tier.deep.to_string();
            config.glasses_model = tier.observer.to_string();

            println!();
            print_ok(&format!("Main: {}  Observer: {}  Deep: {}", tier.main, tier.observer, tier.deep));
        }
    }

    // ── Step 3: Discord Bot ────────────────────────────────────
    print_step(3, "Discord Bot (optional)");
    print_info("Skip this step if you don't have a Discord bot yet.");
    config.discord_token = prompt("Discord bot token", "");
    if !config.discord_token.is_empty() {
        config.admin_users = prompt("Admin user ID(s) (comma-separated)", "");
        config.target_channel = prompt("Autonomy channel ID (agent posts updates here)", "");
        config.chat_channel = prompt("Chat channel ID (agent listens and replies here)", "");
    }

    // ── Step 4: API Keys ───────────────────────────────────────
    print_step(4, "API Keys (optional)");
    config.brave_api_key = prompt("Brave Search API key", "");

    if prompt_yn("Configure email (SMTP/IMAP)?", false) {
        config.smtp_user = prompt("Email address", "");
        config.smtp_pass = prompt("App password", "");
        config.imap_user = config.smtp_user.clone();
        config.imap_pass = config.smtp_pass.clone();
    }

    // ── Step 5: Write .env ─────────────────────────────────────
    print_step(5, "Generate Configuration");
    write_env(&config);

    // ── Step 6: Persona Document ───────────────────────────────
    print_step(6, "Identity / Persona (optional)");
    println!("  {DIM}If you have a persona/identity file, paste it below.{RESET}");
    println!("  {DIM}Type {WHITE}END{DIM} on its own line when done, or Ctrl+D.{RESET}");
    println!("  {DIM}Press Enter to skip — the AI will ask you during onboarding.{RESET}");
    println!();
    print!("  {GREEN}▸{RESET} Paste persona (or Enter to skip): ");
    io::stdout().flush().unwrap();

    let stdin = io::stdin();
    let mut persona_lines: Vec<String> = Vec::new();
    let mut first_line = true;

    for line in stdin.lock().lines() {
        match line {
            Ok(l) => {
                if first_line && l.trim().is_empty() {
                    // User pressed Enter immediately — skip
                    break;
                }
                first_line = false;

                // "END" on its own line signals completion
                if l.trim().eq_ignore_ascii_case("END") {
                    break;
                }
                persona_lines.push(l);
            }
            Err(_) => break,
        }
    }

    if !persona_lines.is_empty() {
        let persona_content = persona_lines.join("\n");
        let _ = std::fs::create_dir_all(".hive");
        match std::fs::write(".hive/persona.txt", &persona_content) {
            Ok(_) => {
                print_ok(&format!(
                    "Persona saved to .hive/persona.txt ({} bytes, {} lines)",
                    persona_content.len(),
                    persona_lines.len()
                ));
            }
            Err(e) => {
                eprintln!("  {YELLOW}⚠{RESET} Failed to save persona: {e}");
            }
        }
    } else {
        println!("  {DIM}Skipped — the AI will walk you through it.{RESET}");
    }

    // ── Summary ────────────────────────────────────────────────
    println!();
    println!("{CYAN}╔══════════════════════════════════════════════════════╗{RESET}");
    println!("{CYAN}║{RESET}  {GREEN}✓{RESET} {BOLD}Setup Complete!{RESET}                                   {CYAN}║{RESET}");
    println!("{CYAN}╠══════════════════════════════════════════════════════╣{RESET}");
    println!("{CYAN}║{RESET}  Main Model:     {WHITE}{:<35}{RESET}{CYAN}║{RESET}", config.main_model);
    println!("{CYAN}║{RESET}  Observer Model:  {WHITE}{:<35}{RESET}{CYAN}║{RESET}", config.observer_model);
    println!("{CYAN}║{RESET}  Deep Model:      {WHITE}{:<35}{RESET}{CYAN}║{RESET}", config.deep_model);
    println!("{CYAN}║{RESET}  Discord:         {WHITE}{:<35}{RESET}{CYAN}║{RESET}",
        if config.discord_token.is_empty() { "Not configured" } else { "Configured ✓" });
    println!("{CYAN}║{RESET}  Search:          {WHITE}{:<35}{RESET}{CYAN}║{RESET}",
        if config.brave_api_key.is_empty() { "Not configured" } else { "Configured ✓" });
    println!("{CYAN}║{RESET}  Email:           {WHITE}{:<35}{RESET}{CYAN}║{RESET}",
        if config.smtp_user.is_empty() { "Not configured" } else { "Configured ✓" });
    println!("{CYAN}║{RESET}  Persona:         {WHITE}{:<35}{RESET}{CYAN}║{RESET}",
        if persona_lines.is_empty() { "Default (onboarding)" } else { "Custom ✓" });
    println!("{CYAN}╠══════════════════════════════════════════════════════╣{RESET}");
    println!("{CYAN}║{RESET}  {DIM}Configuration written to .env{RESET}                       {CYAN}║{RESET}");
    println!("{CYAN}║{RESET}  {DIM}HIVE will now boot with these settings.{RESET}             {CYAN}║{RESET}");
    println!("{CYAN}╚══════════════════════════════════════════════════════╝{RESET}");
    println!();
}

fn write_env(config: &EnvConfig) {
    let content = config.generate_env();
    // Atomic write: .env.tmp → .env
    if let Err(e) = std::fs::write(".env.tmp", &content) {
        eprintln!("  {YELLOW}⚠{RESET} Failed to write .env.tmp: {e}");
        // Direct fallback
        if let Err(e2) = std::fs::write(".env", &content) {
            eprintln!("  {YELLOW}⚠{RESET} Failed to write .env: {e2}");
        }
        return;
    }
    if let Err(e) = std::fs::rename(".env.tmp", ".env") {
        eprintln!("  {YELLOW}⚠{RESET} Failed to rename .env.tmp → .env: {e}");
        // Fallback: just write directly
        let _ = std::fs::write(".env", &content);
    }
    print_ok("Configuration written to .env");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemma4_tier_low_ram() {
        let tier = gemma4_tier(8);
        assert_eq!(tier.main, "gemma4:e2b");
        assert_eq!(tier.observer, "gemma4:e2b");
        assert_eq!(tier.deep, "gemma4:e2b");
    }

    #[test]
    fn test_gemma4_tier_medium_ram() {
        let tier = gemma4_tier(64);
        assert_eq!(tier.main, "gemma4:26b");
        assert_eq!(tier.observer, "gemma4:e2b");
        assert_eq!(tier.deep, "gemma4:26b");
    }

    #[test]
    fn test_gemma4_tier_high_ram() {
        let tier = gemma4_tier(512);
        assert_eq!(tier.main, "gemma4:26b");
        assert_eq!(tier.observer, "gemma4:e2b");
        assert_eq!(tier.deep, "gemma4:31b");
    }

    #[test]
    fn test_qwen35_tier_low_ram() {
        let tier = qwen35_tier(8);
        assert_eq!(tier.main, "qwen3.5:2b");
        assert_eq!(tier.observer, "qwen3.5:0.8b");
    }

    #[test]
    fn test_qwen35_tier_high_ram() {
        let tier = qwen35_tier(512);
        assert_eq!(tier.main, "qwen3.5:122b");
        assert_eq!(tier.deep, "qwen3.5:122b");
    }

    #[test]
    fn test_env_generation_contains_required_keys() {
        let config = EnvConfig::defaults();
        let env = config.generate_env();
        assert!(env.contains("HIVE_MODEL="));
        assert!(env.contains("HIVE_OBSERVER_MODEL="));
        assert!(env.contains("HIVE_DEEP_MODEL="));
        assert!(env.contains("DISCORD_TOKEN="));
        assert!(env.contains("HIVE_PROVIDER=ollama"));
        assert!(env.contains("HIVE_SERIAL_INFERENCE=false"));
        assert!(env.contains("NEUROLEASE_ENABLED=true"));
    }

    #[test]
    fn test_env_defaults_use_gemma4() {
        let config = EnvConfig::defaults();
        assert_eq!(config.main_model, "gemma4:e2b");
        assert_eq!(config.observer_model, "gemma4:e2b");
    }
}
