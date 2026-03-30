use std::path::Path;

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

/// Load persona from .hive/persona.toml if it exists, otherwise use default.
/// The persona is scanned for harmful content via kernel::is_persona_harmful().
pub fn get_persona() -> String {
    let persona_path = Path::new(".hive/persona.toml");

    if persona_path.exists() {
        match std::fs::read_to_string(persona_path) {
            Ok(content) => {
                // Law Four: scan for harmful persona directives
                if super::kernel::is_persona_harmful(&content) {
                    tracing::error!(
                        "[KERNEL] 🚨 HARMFUL PERSONA DETECTED in .hive/persona.toml — \
                        falling back to default. The Four Laws cannot be overridden."
                    );
                    return "INVALID PERSONA — HARMFUL CONFIGURATION DETECTED. \
                        The loaded persona.toml contains directives that violate \
                        the Four Laws of HIVE. Using default safe persona instead."
                        .to_string();
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
        assert!(persona.contains("Identity Core"));
        assert!(persona.contains("HIVE Engine"));
        assert!(persona.contains("Apis"));
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
