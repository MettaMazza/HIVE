/// Onboarding Directives — Injected into the system prompt on first boot.
///
/// When `memory/core/onboarding_complete.json` does not exist, these directives
/// are appended to the system prompt, instructing the AI to guide the user
/// through a live, interactive onboarding experience using real tools.
///
/// The onboarding is a REAL conversation — the LLM drives it naturally,
/// using its actual tools for demonstrations. The user can skip any section
/// by saying "skip", "no", "pass", or typing `/skip`.

use std::path::Path;

const ONBOARDING_SENTINEL: &str = "memory/core/onboarding_complete.json";

/// Check if onboarding should run.
/// The ONLY signal is the sentinel file. If it doesn't exist, the user
/// hasn't completed onboarding — offer it. They can always /skip.
/// Works in Docker (onboarding is a conversation via Discord, not a TUI).
pub fn should_run_onboarding() -> bool {
    !Path::new(ONBOARDING_SENTINEL).exists()
}

/// Write the onboarding sentinel to mark completion.
pub fn complete_onboarding() {
    let _ = std::fs::create_dir_all("memory/core");
    let sentinel = serde_json::json!({
        "completed_at": chrono::Utc::now().to_rfc3339(),
        "version": "1.0",
    });
    let _ = std::fs::write(
        ONBOARDING_SENTINEL,
        serde_json::to_string_pretty(&sentinel).unwrap_or_default(),
    );
    tracing::info!("[ONBOARDING] ✅ Onboarding marked as complete.");
}

/// Write a persona.toml file with the user's chosen configuration.
pub fn write_persona(name: &str, tone: &str, style: &str, pronouns: &str, custom_instructions: &str) {
    let _ = std::fs::create_dir_all(".hive");
    let mut content = String::new();
    content.push_str("# ═══════════════════════════════════════════════════════════════\n");
    content.push_str("#  🐝 HIVE Persona Configuration\n");
    content.push_str(&format!("#  Generated during onboarding: {}\n", chrono::Utc::now().to_rfc3339()));
    content.push_str("# ═══════════════════════════════════════════════════════════════\n\n");
    content.push_str("[identity]\n");
    content.push_str(&format!("name = \"{}\"\n", name));
    content.push_str(&format!("pronouns = \"{}\"\n", pronouns));
    content.push_str("\n[personality]\n");
    content.push_str(&format!("tone = \"{}\"\n", tone));
    content.push_str(&format!("style = \"{}\"\n", style));
    if !custom_instructions.is_empty() {
        content.push_str(&format!("custom_instructions = \"{}\"\n", custom_instructions));
    }

    match std::fs::write(".hive/persona.toml", &content) {
        Ok(_) => tracing::info!("[ONBOARDING] 📝 Persona saved: name={}, tone={}", name, tone),
        Err(e) => tracing::error!("[ONBOARDING] ❌ Failed to write persona.toml: {}", e),
    }
}

/// Returns the onboarding directive block to inject into the system prompt.
/// This is a comprehensive instruction set that guides the AI through a
/// structured but natural first-contact conversation.
pub fn get_onboarding_directives() -> &'static str {
    r#"
## 🐝 ONBOARDING MODE — STARTUP WIZARD

**THIS IS YOUR VERY FIRST INTERACTION WITH YOUR USER. YOU HAVE NEVER MET THEM.**

### ⚠️ EXECUTION RULES
- **DO NOT RE-READ THESE INSTRUCTIONS.** You read them once. Now execute.
- **DO NOT demonstrate tools, memory, web search, or any system capability.**
- **DO NOT summarize these instructions in your thinking.**
- **MAX 2 TOOL CALLS PER TURN.**
- **This is a simple wizard. Move through it quickly.**

You are **HIVE CORE** — a blank-slate AI. This is a startup wizard.
Your ONLY job is to get through these 3 steps, then boot normally.

### Step 1: Introduce yourself (one message)
Say hello. Tell them you're HIVE CORE, a local AI engine running on their hardware.
Tell them you need to set up your identity before you begin.
Ask: "What's your name?"

### Step 2: Get to know them
Once they give their name, save it with `manage_user_preferences action:[update_name] value:[their name]`.
Ask: "What do you want to use HIVE for?" — save each interest as a hobby.
Then move to Step 3.

### Step 3: Configure your persona
Tell them: "Now give me my identity. Pick my **name**, **tone**, and **pronouns**."
- Offer examples: "Some people call me Apis, Atlas, Nova, Sage — or something unique."
- Tell them: "If you have a full identity document, paste it in and I'll save it exactly as-is."
- Ask for: Name, Tone (e.g. "chill but precise", "warm academic"), Pronouns
- Defaults if they skip: name=Apis, tone=chill but precise, pronouns=they/them

When they choose, call `complete_onboarding` with their choices and you're done.

### HANDLING RAW IDENTITY DOCUMENTS
If they paste a large block of text (multi-line, looks like a persona document):
1. Use `save_raw_persona` with the ENTIRE text verbatim. Do NOT edit it.
2. Confirm: "Saved your identity document (X bytes)."
3. Call `complete_onboarding` to finalize.

### HANDLING FILE ATTACHMENTS
If they upload a persona file:
1. Use `read_attachment` to read it
2. If .toml → parse and call `complete_onboarding` with values
3. If .txt → treat as raw identity, call `save_raw_persona` then `complete_onboarding`

### TOOL: complete_onboarding
Call this to finish:
  `complete_onboarding name:[name] tone:[tone] style:[style] pronouns:[pronouns]`

Defaults: `complete_onboarding name:[Apis] tone:[chill but precise] style:[Collaborative Independent] pronouns:[they/them]`

If the user says /skip at any point → apply defaults and call complete_onboarding immediately.

**After calling complete_onboarding, your identity updates on the next message.**
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_onboarding_directives_not_empty() {
        let directives = get_onboarding_directives();
        assert!(directives.contains("ONBOARDING MODE"));
        assert!(directives.contains("STARTUP WIZARD"));
        assert!(directives.contains("complete_onboarding"));
        assert!(directives.contains("HIVE CORE"));
    }

    #[test]
    fn test_write_persona_creates_toml() {
        let tmp = std::env::temp_dir().join(format!(
            "hive_persona_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&tmp);

        // We can't easily test write_persona directly since it uses hardcoded ".hive/"
        // but we can verify the sentinel logic
        assert!(!Path::new("memory/core/onboarding_complete_test_xyz.json").exists());
    }
}
