use std::path::{Path, PathBuf};
use std::fs;

// ═══════════════════════════════════════════════════════════════════
//  🎭 PERSONA MODULE — Hot-Swappable Identity with Memory Isolation
//
//  Each persona is a completely separate entity with its own:
//  - Identity file (.hive/personas/<name>/persona.txt)
//  - Full 6-tier memory stack (memory/personas/<name>/)
//  No data crosses between personas. 4 personas = 4 separate minds.
// ═══════════════════════════════════════════════════════════════════

const ACTIVE_PERSONA_PATH: &str = ".hive/active_persona";
const PERSONAS_CONFIG_DIR: &str = ".hive/personas";
const PERSONAS_MEMORY_DIR: &str = "memory/personas";

/// Info about a persona for listing/display.
#[derive(Debug, Clone)]
pub struct PersonaInfo {
    pub name: String,
    pub is_active: bool,
    pub identity_size_bytes: u64,
    pub memory_exists: bool,
}

/// Get the currently active persona name. Defaults to "home".
pub fn get_active_persona() -> String {
    match fs::read_to_string(ACTIVE_PERSONA_PATH) {
        Ok(name) => {
            let name = name.trim().to_string();
            if name.is_empty() { "home".into() } else { name }
        }
        Err(_) => "home".into(),
    }
}

/// Get the memory directory for a given persona.
/// "home" → memory/ (the default root)
/// anything else → memory/personas/<name>/
pub fn get_persona_memory_dir(name: &str) -> PathBuf {
    if name == "home" {
        PathBuf::from("memory")
    } else {
        PathBuf::from(PERSONAS_MEMORY_DIR).join(name)
    }
}

/// Get the identity file path for a given persona.
/// "home" → .hive/persona.txt (or .hive/persona.toml)
/// anything else → .hive/personas/<name>/persona.txt
pub fn get_persona_identity_path(name: &str) -> PathBuf {
    if name == "home" {
        // Home uses the standard identity paths (persona.txt takes priority in identity.rs)
        PathBuf::from(".hive/persona.txt")
    } else {
        PathBuf::from(PERSONAS_CONFIG_DIR).join(name).join("persona.txt")
    }
}

/// List all available personas.
pub fn list_personas() -> Vec<PersonaInfo> {
    let active = get_active_persona();
    let mut personas = Vec::new();

    // Always include "home" (birth persona)
    let home_identity = if Path::new(".hive/persona.txt").exists() {
        fs::metadata(".hive/persona.txt").map(|m| m.len()).unwrap_or(0)
    } else if Path::new(".hive/persona.toml").exists() {
        fs::metadata(".hive/persona.toml").map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };

    personas.push(PersonaInfo {
        name: "home".into(),
        is_active: active == "home",
        identity_size_bytes: home_identity,
        memory_exists: Path::new("memory/core").exists(),
    });

    // Scan .hive/personas/ for custom personas
    let personas_dir = Path::new(PERSONAS_CONFIG_DIR);
    if personas_dir.exists() {
        if let Ok(entries) = fs::read_dir(personas_dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let identity_path = entry.path().join("persona.txt");
                    let memory_path = PathBuf::from(PERSONAS_MEMORY_DIR).join(&name);

                    personas.push(PersonaInfo {
                        is_active: active == name,
                        identity_size_bytes: fs::metadata(&identity_path)
                            .map(|m| m.len())
                            .unwrap_or(0),
                        memory_exists: memory_path.exists(),
                        name,
                    });
                }
            }
        }
    }

    personas
}

/// Validate a persona name.
fn validate_name(name: &str) -> Result<(), String> {
    if name == "home" {
        return Err("Cannot use 'home' — that's the birth persona.".into());
    }
    if name.is_empty() {
        return Err("Persona name cannot be empty.".into());
    }
    if name.len() > 32 {
        return Err("Persona name must be 32 characters or fewer.".into());
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err("Persona name can only contain letters, numbers, underscores, and hyphens.".into());
    }
    Ok(())
}

/// Create a new persona with the given identity text.
/// Creates both the config directory (.hive/personas/<name>/) and
/// the memory directory (memory/personas/<name>/).
pub fn create_persona(name: &str, identity_text: &str) -> Result<String, String> {
    validate_name(name)?;

    let config_dir = PathBuf::from(PERSONAS_CONFIG_DIR).join(name);
    if config_dir.exists() {
        return Err(format!("Persona '{}' already exists. Use `/persona edit {}` to modify it.", name, name));
    }

    // Create config directory + identity file
    fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Failed to create persona config dir: {}", e))?;

    let identity_path = config_dir.join("persona.txt");
    fs::write(&identity_path, identity_text)
        .map_err(|e| format!("Failed to write persona identity: {}", e))?;

    // Create memory directory structure
    let memory_dir = PathBuf::from(PERSONAS_MEMORY_DIR).join(name);
    for subdir in &["core", "working", "timeline", "synaptic", "scratch",
                     "preferences", "lessons", "moderation", "vectors",
                     "computer_runtime"] {
        fs::create_dir_all(memory_dir.join(subdir))
            .map_err(|e| format!("Failed to create memory dir {}: {}", subdir, e))?;
    }

    tracing::info!("[PERSONA] 🎭 Created persona '{}' ({} bytes identity)", name, identity_text.len());

    Ok(format!(
        "✅ Persona '{}' created.\n\
        Identity: {} bytes\n\
        Memory: {} (empty — clean slate)\n\
        Switch with: `/persona {}`",
        name, identity_text.len(), memory_dir.display(), name
    ))
}

/// Delete a persona and ALL its data (identity + memory).
/// If "home" is deleted, the birth persona files are removed and the system
/// falls back to the default HIVE CORE Apis identity.
pub fn delete_persona(name: &str) -> Result<String, String> {
    if name == "home" {
        // Delete the birth persona files — system will fall back to default Apis
        let mut deleted = Vec::new();
        if Path::new(".hive/persona.txt").exists() {
            let _ = fs::remove_file(".hive/persona.txt");
            deleted.push("persona.txt");
        }
        if Path::new(".hive/persona.toml").exists() {
            let _ = fs::remove_file(".hive/persona.toml");
            deleted.push("persona.toml");
        }
        let _ = fs::write(ACTIVE_PERSONA_PATH, "home");
        tracing::info!("[PERSONA] 🗑️ Deleted birth persona — falling back to default Apis");
        return Ok(format!(
            "🗑️ Birth persona deleted (removed: {}).\nFalling back to default HIVE CORE (Apis).\n\
            Create a new identity with `/persona create <name>` or go through onboarding again.",
            if deleted.is_empty() { "nothing to remove".into() } else { deleted.join(", ") }
        ));
    }

    validate_name(name)?;

    let config_dir = PathBuf::from(PERSONAS_CONFIG_DIR).join(name);
    let memory_dir = PathBuf::from(PERSONAS_MEMORY_DIR).join(name);

    if !config_dir.exists() && !memory_dir.exists() {
        return Err(format!("Persona '{}' does not exist.", name));
    }

    // If this persona is currently active, switch back to home first
    if get_active_persona() == name {
        let _ = fs::write(ACTIVE_PERSONA_PATH, "home");
        tracing::info!("[PERSONA] 🏠 Switched to 'home' before deleting '{}'", name);
    }

    let mut deleted = Vec::new();

    if config_dir.exists() {
        fs::remove_dir_all(&config_dir)
            .map_err(|e| format!("Failed to remove config dir: {}", e))?;
        deleted.push(format!("config ({})", config_dir.display()));
    }

    if memory_dir.exists() {
        fs::remove_dir_all(&memory_dir)
            .map_err(|e| format!("Failed to remove memory dir: {}", e))?;
        deleted.push(format!("memory ({})", memory_dir.display()));
    }

    tracing::info!("[PERSONA] 🗑️ Deleted persona '{}': {}", name, deleted.join(", "));

    Ok(format!(
        "🗑️ Persona '{}' deleted.\nRemoved: {}\nSwitched to: home",
        name, deleted.join(", ")
    ))
}

/// Switch to a different persona. Returns the new memory base path.
pub fn switch_persona(name: &str) -> Result<PathBuf, String> {
    if name != "home" {
        validate_name(name)?;

        // Check persona exists
        let config_dir = PathBuf::from(PERSONAS_CONFIG_DIR).join(name);
        if !config_dir.exists() {
            return Err(format!(
                "Persona '{}' does not exist. Create it first with `/persona create {}`.",
                name, name
            ));
        }
    }

    // Write active persona
    let _ = fs::create_dir_all(".hive");
    fs::write(ACTIVE_PERSONA_PATH, name)
        .map_err(|e| format!("Failed to write active persona: {}", e))?;

    let memory_dir = get_persona_memory_dir(name);

    // Ensure memory directory exists
    if name != "home" {
        let _ = fs::create_dir_all(&memory_dir);
    }

    tracing::info!("[PERSONA] 🔄 Switched to persona '{}' (memory: {})", name, memory_dir.display());

    Ok(memory_dir)
}

/// Edit an existing persona's identity text.
pub fn edit_persona(name: &str, new_identity: &str) -> Result<String, String> {
    if name == "home" {
        // For home, edit .hive/persona.txt
        let _ = fs::create_dir_all(".hive");
        fs::write(".hive/persona.txt", new_identity)
            .map_err(|e| format!("Failed to write home persona: {}", e))?;
        tracing::info!("[PERSONA] ✏️ Edited home persona ({} bytes)", new_identity.len());
        return Ok(format!("✅ Home persona updated ({} bytes).", new_identity.len()));
    }

    validate_name(name)?;

    let config_dir = PathBuf::from(PERSONAS_CONFIG_DIR).join(name);
    if !config_dir.exists() {
        return Err(format!("Persona '{}' does not exist.", name));
    }

    let identity_path = config_dir.join("persona.txt");
    fs::write(&identity_path, new_identity)
        .map_err(|e| format!("Failed to write persona identity: {}", e))?;

    tracing::info!("[PERSONA] ✏️ Edited persona '{}' ({} bytes)", name, new_identity.len());

    Ok(format!("✅ Persona '{}' updated ({} bytes). Identity refreshes on next message.", name, new_identity.len()))
}

/// Format a listing of all personas for display.
pub fn format_persona_list() -> String {
    let personas = list_personas();
    let mut output = String::from("🎭 **Available Personas:**\n\n");

    for p in &personas {
        let active_marker = if p.is_active { " ← active" } else { "" };
        let identity_info = if p.identity_size_bytes > 0 {
            format!("({} bytes)", p.identity_size_bytes)
        } else {
            "(no identity file)".into()
        };
        let memory_info = if p.memory_exists { "has memory" } else { "no memory yet" };

        output.push_str(&format!(
            "  {} **{}** — {} | {}{}\n",
            if p.is_active { "▶" } else { "•" },
            p.name,
            identity_info,
            memory_info,
            active_marker,
        ));
    }

    output.push_str(&format!("\n📊 {} persona(s) total.", personas.len()));
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_name() {
        assert!(validate_name("nova").is_ok());
        assert!(validate_name("my-persona").is_ok());
        assert!(validate_name("test_123").is_ok());
        assert!(validate_name("home").is_err());
        assert!(validate_name("").is_err());
        assert!(validate_name("has spaces").is_err());
        assert!(validate_name("a".repeat(33).as_str()).is_err());
    }

    #[test]
    fn test_get_persona_memory_dir() {
        assert_eq!(get_persona_memory_dir("home"), PathBuf::from("memory"));
        assert_eq!(get_persona_memory_dir("nova"), PathBuf::from("memory/personas/nova"));
    }

    #[test]
    fn test_get_persona_identity_path() {
        assert_eq!(get_persona_identity_path("home"), PathBuf::from(".hive/persona.txt"));
        assert_eq!(get_persona_identity_path("nova"), PathBuf::from(".hive/personas/nova/persona.txt"));
    }

    #[test]
    fn test_get_active_persona_default() {
        // When file doesn't exist, should default to "home"
        assert_eq!(get_active_persona(), "home");
    }
}
