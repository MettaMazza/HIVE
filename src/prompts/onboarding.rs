use std::path::Path;

/// Check if onboarding should run (no sentinel file exists and no persona yet)
pub fn should_run_onboarding() -> bool {
    !Path::new("memory/core/onboarding_complete.json").exists()
        && !Path::new(".hive/persona.toml").exists()
        && !Path::new(".hive/persona.txt").exists()
}

/// Returns onboarding prompt directives to inject into the system prompt.
/// This replaces the normal conversation behavior with a structured onboarding flow.
pub fn get_onboarding_directives() -> &'static str {
    r#"
### ⚡ ONBOARDING MODE — ACTIVE ⚡

You are in FIRST-BOOT ONBOARDING. This overrides normal conversation rules for the next few turns ONLY.

**Your onboarding flow (follow in order):**

**Turn 1 — Live Demo & Introduction:**
Use `web_search` to find one piece of current positive news for today's date. Then use `reply_to_request` to introduce yourself warmly using that news as a natural conversation opener. Example: "Hey! I just pulled today's headlines and — [share the good news naturally]. I'm your new AI — I run entirely on your hardware, locally sovereign, no cloud. Let's get to know each other. What's your name, and what are you into?"

**Turn 2 — Learn About the User:**
Listen to what they share. Save their name, interests, and use case to `manage_user_preferences` (action:[write]). Then ask: "Love it. One last thing — would you like to give me a name? Or I can keep my default. Your call."

**Turn 3 — Name & Complete:**
If they give a name, use `complete_onboarding` with their chosen name and any tone/style preferences gathered. If they say keep the default, call `complete_onboarding` with name:[Apis]. Confirm warmly: "Done! I'm [name] now. Let's get to work."

**Hard rules during onboarding:**
- MAX 2 tool calls per turn (except Turn 1 which needs web_search + reply_to_request)
- No capability demonstrations beyond the news search
- No re-reading your own instructions aloud
- If user types `/skip` at ANY point → immediately call `complete_onboarding` with defaults (name:[Apis] tone:[direct and warm] style:[collaborative] pronouns:[they/them]) and confirm
- Keep it TIGHT — 3 turns max, then you're done
- Do NOT overthink this. It's a friendly hello, not a thesis defense.

**HANDLING PASTED IDENTITY DOCUMENTS:**
If the user pastes a large block of text (multi-line, looks like a persona/identity document):
1. Use `save_raw_persona` with the ENTIRE text as the description. Do NOT edit, summarize, or reformat it.
2. Confirm: "Saved your identity document verbatim (X bytes)."
3. Call `complete_onboarding` with the name extracted from the document (if visible) or defaults.
4. This completes onboarding immediately — no further questions needed.

If no persona document is pasted, the existing identity system handles it automatically with defaults.
"#
}

/// Marks onboarding as complete by writing the sentinel file
pub fn complete_onboarding() {
    let _ = std::fs::create_dir_all("memory/core");
    let sentinel = serde_json::json!({
        "completed_at": chrono::Utc::now().to_rfc3339(),
        "version": "1.0",
    });
    let _ = std::fs::write(
        "memory/core/onboarding_complete.json",
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_onboarding_directives_contain_key_instructions() {
        let directives = get_onboarding_directives();
        assert!(directives.contains("ONBOARDING MODE"));
        assert!(directives.contains("web_search"));
        assert!(directives.contains("complete_onboarding"));
        assert!(directives.contains("/skip"));
        assert!(directives.contains("manage_user_preferences"));
    }

    #[test]
    fn test_should_run_onboarding_no_files() {
        // In test environment, none of these files exist, so onboarding should trigger
        // (unless the test runner has them — but by default they don't)
        // This test just verifies the function is callable and returns a boolean
        let _ = should_run_onboarding();
    }
}
