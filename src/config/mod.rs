pub mod persona;
pub mod setup_wizard;

use std::sync::Arc;
use serde::{Deserialize, Serialize};

/// The centralized Configuration Matrix for the HIVE Engine.
/// Parses all environment variables to determine storage paths,
/// agent behaviors, hyperparameters, and tool governance limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    // ── Dimension 1: Storage & Persistance ──
    pub storage_root: String,
    pub workspace_dir: String,
    pub artifacts_dir: String,
    pub logs_dir: String,
    pub downloads_dir: String,

    // ── Dimension 2: Agent Loops & Thresholds ──
    pub timeout_inference_secs: u64,
    pub timeout_tool_secs: u64,
    pub timeout_compile_secs: u64,
    pub limit_context_tokens: u32,
    pub limit_generation_tokens: u32,
    pub limit_loop_iters: u32,
    pub sleep_interval_secs: u64,

    // ── Dimension 3: Model Hyperparameters ──
    pub model_temperature: f32,
    pub model_top_p: f32,
    pub model_top_k: u32,
    pub model_repeat_penalty: f32,

    // ── Dimension 4: Governance & Toggles ──
    pub allow_terminal: bool,
    pub allow_file_system: bool,
    pub allow_refusal: bool,
    pub strict_safeguards: bool,
    pub admin_users: Vec<String>,

    // ── Dimension 5: Persona & Identity ──
    pub system_name: String,
    pub persona_file: Option<String>,
    pub system_prompt: Option<String>,

    // ── Dimension 6: UI, UX, & Dashboards ──
    pub ui_title: String,
    pub ui_theme_color: String,
    pub opencode_port: u16,
    pub mesh_discovery_interval: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            // These absolute defaults mirror the original hardcoded settings exactly (1:1 parity)
            storage_root: "memory".into(),
            workspace_dir: ".".into(),
            artifacts_dir: "artifacts".into(),
            logs_dir: "logs".into(),
            downloads_dir: "/tmp/hive".into(),

            timeout_inference_secs: 300,
            timeout_tool_secs: 30,
            timeout_compile_secs: 15,
            limit_context_tokens: 8192,
            limit_generation_tokens: 4096,
            limit_loop_iters: 15,
            sleep_interval_secs: 300,

            model_temperature: 0.7,
            model_top_p: 0.9,
            model_top_k: 40,
            model_repeat_penalty: 1.1,

            allow_terminal: true, // Legacy baseline: admin gating protects this
            allow_file_system: true,
            allow_refusal: true,
            strict_safeguards: true,
            admin_users: vec![],

            system_name: "Apis".into(),
            persona_file: None,
            system_prompt: None,

            ui_title: "HIVE Mesh".into(),
            ui_theme_color: "#ff9900".into(),
            opencode_port: 4096,
            mesh_discovery_interval: 300,
        }
    }
}

impl AppConfig {
    /// Loads the configuration block by inspecting environment variables,
    /// falling back to the 1:1 legacy hardcoded parameters.
    pub fn load_from_env() -> Arc<Self> {
        let mut cfg = AppConfig::default();

        if let Ok(val) = std::env::var("HIVE_STORAGE_ROOT") { cfg.storage_root = val; }
        if let Ok(val) = std::env::var("HIVE_WORKSPACE_DIR") { cfg.workspace_dir = val; }
        if let Ok(val) = std::env::var("HIVE_ARTIFACTS_DIR") { cfg.artifacts_dir = val; }
        if let Ok(val) = std::env::var("HIVE_LOGS_DIR") { cfg.logs_dir = val; }
        if let Ok(val) = std::env::var("HIVE_DOWNLOADS_DIR") { cfg.downloads_dir = val; }

        if let Ok(val) = std::env::var("HIVE_TIMEOUT_INFERENCE_SECS") { if let Ok(parsed) = val.parse() { cfg.timeout_inference_secs = parsed; } }
        if let Ok(val) = std::env::var("HIVE_TIMEOUT_TOOL_SECS") { if let Ok(parsed) = val.parse() { cfg.timeout_tool_secs = parsed; } }
        if let Ok(val) = std::env::var("HIVE_TIMEOUT_COMPILE_SECS") { if let Ok(parsed) = val.parse() { cfg.timeout_compile_secs = parsed; } }
        if let Ok(val) = std::env::var("HIVE_LIMIT_CONTEXT_TOKENS") { if let Ok(parsed) = val.parse() { cfg.limit_context_tokens = parsed; } }
        if let Ok(val) = std::env::var("HIVE_LIMIT_GENERATION_TOKENS") { if let Ok(parsed) = val.parse() { cfg.limit_generation_tokens = parsed; } }
        if let Ok(val) = std::env::var("HIVE_LIMIT_LOOP_ITERS") { if let Ok(parsed) = val.parse() { cfg.limit_loop_iters = parsed; } }
        if let Ok(val) = std::env::var("HIVE_SLEEP_INTERVAL_SECS") { if let Ok(parsed) = val.parse() { cfg.sleep_interval_secs = parsed; } }

        if let Ok(val) = std::env::var("HIVE_MODEL_TEMPERATURE") { if let Ok(parsed) = val.parse() { cfg.model_temperature = parsed; } }
        if let Ok(val) = std::env::var("HIVE_MODEL_TOP_P") { if let Ok(parsed) = val.parse() { cfg.model_top_p = parsed; } }
        if let Ok(val) = std::env::var("HIVE_MODEL_TOP_K") { if let Ok(parsed) = val.parse() { cfg.model_top_k = parsed; } }
        if let Ok(val) = std::env::var("HIVE_MODEL_REPEAT_PENALTY") { if let Ok(parsed) = val.parse() { cfg.model_repeat_penalty = parsed; } }

        if let Ok(val) = std::env::var("HIVE_ALLOW_TERMINAL") { cfg.allow_terminal = val.to_lowercase() == "true" || val == "1"; }
        if let Ok(val) = std::env::var("HIVE_ALLOW_FILE_SYSTEM") { cfg.allow_file_system = val.to_lowercase() == "true" || val == "1"; }
        if let Ok(val) = std::env::var("HIVE_ALLOW_REFUSAL") { cfg.allow_refusal = val.to_lowercase() == "true" || val == "1"; }
        if let Ok(val) = std::env::var("HIVE_STRICT_SAFEGUARDS") { cfg.strict_safeguards = val.to_lowercase() == "true" || val == "1"; }

        if let Ok(val) = std::env::var("HIVE_ADMIN_USERS") {
            cfg.admin_users = val.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }

        if let Ok(val) = std::env::var("HIVE_SYSTEM_NAME") { cfg.system_name = val; }
        if let Ok(val) = std::env::var("HIVE_PERSONA_FILE") { cfg.persona_file = Some(val); }
        if let Ok(val) = std::env::var("HIVE_SYSTEM_PROMPT") { cfg.system_prompt = Some(val); }

        if let Ok(val) = std::env::var("HIVE_UI_TITLE") { cfg.ui_title = val; }
        if let Ok(val) = std::env::var("HIVE_UI_THEME_COLOR") { cfg.ui_theme_color = val; }
        if let Ok(val) = std::env::var("HIVE_OPENCODE_PORT") { if let Ok(parsed) = val.parse() { cfg.opencode_port = parsed; } }
        if let Ok(val) = std::env::var("HIVE_MESH_DISCOVERY_INTERVAL") { if let Ok(parsed) = val.parse() { cfg.mesh_discovery_interval = parsed; } }

        Arc::new(cfg)
    }
}
