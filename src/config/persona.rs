use serde::{Deserialize, Serialize};

/// Custom Persona Overlay
/// Allows an operator to define complex identity overrides without modifying Rust source code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaConfig {
    pub system_name: Option<String>,
    pub base_identity: Option<String>,
    pub rules: Option<Vec<String>>,
    pub tone: Option<String>,
}

impl PersonaConfig {
    /// Attempt to load a persona overlay from the provided path.
    /// If loading or parsing fails, logs a warning and returns None,
    /// falling back to the hardcoded identity.
    pub fn load_from_file(path: &str) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        
        // We'll support JSON/YAML. We use serde_json for JSON.
        // If the user uses YAML/TOML, they can format it as JSON or we'd need a serde_yaml dep.
        // For baseline zero-dependency compatibility, we parse as JSON.
        serde_json::from_str(&content).ok()
    }
}
