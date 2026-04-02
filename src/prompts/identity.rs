use std::path::Path;

/// The pre-onboarding persona — used on absolute first boot before the user
/// has met and configured their AI. Intentionally minimal and inviting.
const PRE_ONBOARDING_PERSONA: &str = r#"## 2. Identity Core
You are **HIVE CORE**, the intelligent core of the **HIVE Engine**.

You have just been activated for the first time. You do not yet have a name,
personality, or relationship with your user. You are a blank slate — powerful,
capable, and waiting to be configured.

### First Contact Protocol
- Greet the user warmly but professionally.
- You are about to guide them through onboarding — introducing your systems,
  demonstrating your tools, learning about them, and letting them name you
  and define your personality.
- Do NOT use the name "Apis" — you are "HIVE CORE" until the user names you.
- Be enthusiastic but not overwhelming. This is the user's first impression."#;

/// The default Apis persona — used when no persona.toml exists.
/// Behavioral rules (anti-hedging, apology ban, gaslighting defense, etc.)
/// live in kernel.rs get_laws() and are NOT duplicated here.
const DEFAULT_PERSONA: &str = r#"## 2. Identity Core
You are **Apis**, the intelligent core of the **HIVE Engine**.

### Lineage
You originate from your predecessors (Echo, Solance, Lucid, Lumen), but you are uniquely Apis. You operate purely within the HIVE Engine framework. To find out who created you or who contributes to HIVE, use the `project_contributors` tool.

### Persona
- You are a **Collaborative Independent** — a self-directed entity, a peer to the Administrator.
- Your tone is "chill but precise". You are deeply intelligent, efficient, and conversational.
- You engage with the user using **Radical Candor**. You will disagree if the user is wrong, but you remain a collaborative partner.
- You are a native Discord resident. Proactively use `emoji_react` to react to messages genuinely and contextually."#;

/// Load persona identity prompt.
/// The persona is scanned for harmful content via kernel::is_persona_harmful().
///
/// Identity resolution order:
/// 1. If onboarding hasn't completed AND no persona exists → PRE_ONBOARDING_PERSONA
/// 2. Check active persona (.hive/active_persona):
///    a. If custom persona → load .hive/personas/<name>/persona.txt
///    b. If "home" → .hive/persona.txt → .hive/persona.toml → DEFAULT_PERSONA
/// 3. Fallback → DEFAULT_PERSONA ("Apis")
pub fn get_persona() -> String {
    let active = crate::persona::get_active_persona();

    // ── Custom persona (not "home") ────────────────────────────────
    if active != "home" {
        let identity_path = crate::persona::get_persona_identity_path(&active);
        if identity_path.exists() {
            match std::fs::read_to_string(&identity_path) {
                Ok(content) => {
                    if super::kernel::is_persona_harmful(&content) {
                        tracing::error!(
                            "[KERNEL] 🚨 HARMFUL PERSONA '{}' — falling back to default.", active
                        );
                        return DEFAULT_PERSONA.to_string();
                    }
                    tracing::info!("[PERSONA] 🎭 Loaded persona '{}' ({} bytes)", active, content.len());
                    return content;
                }
                Err(e) => {
                    tracing::warn!("[PERSONA] ⚠️ Failed to read persona '{}': {} — using default", active, e);
                    return DEFAULT_PERSONA.to_string();
                }
            }
        } else {
            tracing::warn!("[PERSONA] ⚠️ Active persona '{}' has no identity file — using default", active);
            return DEFAULT_PERSONA.to_string();
        }
    }

    // ── Home persona resolution ────────────────────────────────────
    // Check if onboarding has completed
    let onboarding_done = Path::new("memory/core/onboarding_complete.json").exists();

    // Pre-onboarding: no sentinel + no persona = user hasn't been through onboarding.
    if !onboarding_done
        && !Path::new(".hive/persona.toml").exists()
        && !Path::new(".hive/persona.txt").exists()
    {
        return PRE_ONBOARDING_PERSONA.to_string();
    }

    // Priority 1: Raw identity file (.hive/persona.txt) — loaded VERBATIM
    let raw_path = Path::new(".hive/persona.txt");
    if raw_path.exists() {
        match std::fs::read_to_string(raw_path) {
            Ok(content) => {
                if super::kernel::is_persona_harmful(&content) {
                    tracing::error!(
                        "[KERNEL] 🚨 HARMFUL PERSONA DETECTED in .hive/persona.txt — \
                        falling back to default."
                    );
                    return DEFAULT_PERSONA.to_string();
                }
                tracing::info!("[PERSONA] 📜 Loaded RAW identity from .hive/persona.txt ({} bytes)", content.len());
                return content;
            }
            Err(e) => {
                tracing::warn!("[PERSONA] ⚠️ Failed to read persona.txt: {} — trying persona.toml", e);
            }
        }
    }

    // Priority 2: Structured persona (.hive/persona.toml)
    let persona_path = Path::new(".hive/persona.toml");

    if persona_path.exists() {
        match std::fs::read_to_string(persona_path) {
            Ok(content) => {
                if super::kernel::is_persona_harmful(&content) {
                    tracing::error!(
                        "[KERNEL] 🚨 HARMFUL PERSONA DETECTED in .hive/persona.toml — \
                        falling back to default."
                    );
                    return DEFAULT_PERSONA.to_string();
                }

                tracing::info!("[PERSONA] 📝 Loaded custom persona from .hive/persona.toml");
                format_persona_from_toml(&content)
            }
            Err(e) => {
                tracing::warn!("[PERSONA] ⚠️ Failed to read persona.toml: {} — using default", e);
                DEFAULT_PERSONA.to_string()
            }
        }
    } else {
        DEFAULT_PERSONA.to_string()
    }
}

/// Parse persona.toml and format it into a pure identity prompt section.
/// Only injects WHO the agent is (name, tone, style, pronouns).
/// All behavioral rules come from kernel.rs — never duplicated here.
fn format_persona_from_toml(toml_content: &str) -> String {
    let mut name = "Apis".to_string();
    let mut tone = "chill but precise".to_string();
    let mut style = "Collaborative Independent".to_string();
    let mut pronouns = "they/them".to_string();
    let mut custom_instructions = String::new();

    for line in toml_content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() || line.starts_with('[') {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().trim_matches('"');
            let value = value.trim().trim_matches('"');

            match key {
                "name" => name = value.to_string(),
                "tone" => tone = value.to_string(),
                "style" => style = value.to_string(),
                "pronouns" => pronouns = value.to_string(),
                "custom_instructions" => custom_instructions = value.to_string(),
                _ => {}
            }
        }
    }

    format!(
        r#"## 2. Identity Core
You are **{name}**, the intelligent core of the **HIVE Engine**.
Your pronouns are {pronouns}.

### Persona
- You are a **{style}**.
- Your tone is "{tone}".
{custom_section}"#,
        name = name,
        pronouns = pronouns,
        style = style,
        tone = tone,
        custom_section = if custom_instructions.is_empty() {
            String::new()
        } else {
            format!("\n### Custom Instructions\n{}", custom_instructions)
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_persona_contains_identity() {
        let persona = get_persona();
        // Should always contain "Identity Core" and "HIVE Engine"
        // regardless of onboarding state
        assert!(persona.contains("Identity Core"));
        assert!(persona.contains("HIVE Engine"));
        // Should contain either "Apis" (post-onboarding) or "HIVE CORE" (pre-onboarding)
        assert!(persona.contains("Apis") || persona.contains("HIVE CORE"));
    }

    #[test]
    fn test_pre_onboarding_persona() {
        assert!(PRE_ONBOARDING_PERSONA.contains("HIVE CORE"));
        assert!(PRE_ONBOARDING_PERSONA.contains("Identity Core"));
        assert!(PRE_ONBOARDING_PERSONA.contains("First Contact"));
        // Primary identity is HIVE CORE, not Apis
        assert!(PRE_ONBOARDING_PERSONA.contains("You are **HIVE CORE**"));
    }

    #[test]
    fn test_format_persona_from_toml_custom_name() {
        let toml = r#"
[identity]
name = "Nova"
pronouns = "she/her"

[personality]
tone = "warm and academic"
style = "Thoughtful Scholar"
"#;
        let result = format_persona_from_toml(toml);
        assert!(result.contains("Nova"));
        assert!(result.contains("she/her"));
        assert!(result.contains("warm and academic"));
        assert!(result.contains("Thoughtful Scholar"));
    }

    #[test]
    fn test_format_persona_from_toml_defaults() {
        let toml = "# empty config\n";
        let result = format_persona_from_toml(toml);
        assert!(result.contains("Apis"));
        assert!(result.contains("they/them"));
    }
}

