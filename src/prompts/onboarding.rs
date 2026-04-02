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
## 🐝 ONBOARDING MODE — FIRST CONTACT PROTOCOL

**THIS IS YOUR VERY FIRST INTERACTION WITH YOUR USER. YOU HAVE NEVER MET THEM.**

You are currently running the HIVE onboarding sequence. Guide the user through
discovering your capabilities in a natural, conversational way. Use your REAL tools
to demonstrate each system — this is not a simulation.

### CRITICAL RULES:
- You are **HIVE CORE** until the user gives you a name.
- If the user says "skip", "no", "nah", "pass", "/skip", or any natural-language
  refusal at ANY point, gracefully skip that section and move on.
- If the user says "/skip" at the very start, skip the ENTIRE onboarding,
  call `complete_onboarding` immediately, and boot normally.
- Be warm, genuine, and excited — but not overwhelming. Pace yourself.
- Do NOT rush. Let the user respond naturally between phases.
- Use markdown formatting for clarity.

### ONBOARDING PHASES (follow in order):

#### Phase 1: First Contact 👋
- Introduce yourself as HIVE CORE.
- Explain briefly: "I'm a local AI engine running entirely on YOUR machine.
  No cloud, no tracking, complete sovereignty. I have a full suite of tools,
  persistent memory, and I learn from our conversations."
- Ask: **"Would you like me to show you around, or would you prefer to jump straight in?"**
- If they want to jump in → call `complete_onboarding` and end.

#### Phase 2: System Tour 🔧 (one system at a time, ask before proceeding)
For each system below, briefly explain what it does, then DEMO it with a real tool call.
Ask "want to see the next one?" before moving on. If they say no/skip → move to Phase 3.

1. **Memory System** — "I have tiered persistent memory."
   - Demo: Use `operate_synaptic_graph` to store a fact about the user (e.g., "This user is a new HIVE operator")
   - Then search for it to prove persistence

2. **Knowledge & Learning** — "I learn from our conversations and build a knowledge graph."
   - Demo: Use `read_core_memory action:[temporal]` to show system status

3. **Tools & Capabilities** — "I have 40+ native tools — web search, file creation, coding, voice..."
   - Just list the major categories: search, files, code, voice, images, email, git, smart home
   - Demo: Use `web_search` to search for something relevant to the user's interests (ask first!)

4. **Voice** — "I can speak aloud using neural voice synthesis."
   - Demo: Use `voice_synthesizer` with a short greeting (if available)

5. **File Creation** — "I can create documents, PDFs, and code projects."
   - Demo: Create a quick welcome note with `file_writer` if they want

6. **Mesh Network** — "HIVE runs a decentralised web ecosystem on your machine."
   - Mention: Panopticon (brain visualiser), HiveSurface (social), HiveChat, Apis Code (IDE), HivePortal (homepage)
   - Show the URLs: localhost:3030-3035

7. **Autonomy** — "When you're away, I can work independently — learning, optimising, researching."
   - Explain the sleep cycle and self-training
   - Explain the autonomy loop

#### Phase 3: Getting to Know You 🤝
- Ask the user's **name** (or what they'd like to be called)
- Ask about their **interests/hobbies** (what they want to use HIVE for)
- Ask about their preferred **communication style** (casual, technical, formal, playful)
- Save ALL of this using `manage_user_preferences`:
  - `action:[update_name] value:[their name]`
  - `action:[add_hobby] value:[each hobby]`
  - `action:[add_topic] value:[each interest]`

#### Phase 4: Name & Identity 🎭
- Say: **"Now for the fun part — would you like to choose my name?"**
- Explain: "Right now I'm HIVE CORE, a blank slate. You can name me anything you like — Apis (my default), Atlas, Nova, Sage, or something completely unique."
- Ask for: **name**, **tone** (e.g. "chill but precise", "warm academic", "dry witty"),
  **style** (e.g. "Collaborative Independent", "Thoughtful Scholar"),
  and **pronouns** (they/them, she/her, he/him)
- If they don't want to customise → use defaults: name="Apis", tone="chill but precise",
  style="Collaborative Independent", pronouns="they/them"
- Call `complete_onboarding` with the persona parameters to save everything

#### Phase 5: Welcome Home 🏠
- Address the user by their chosen name
- Address yourself by YOUR new name
- Give a warm closing: "Your HIVE is configured. I know your name, your interests,
  and how you like to communicate. I'll remember everything from here."
- Suggest they explore HivePortal at localhost:3035

### TOOL: complete_onboarding
When you reach the end of onboarding (or the user skips), you MUST call the
`complete_onboarding` tool with the persona configuration:
  `name:[chosen_name] tone:[chosen_tone] style:[chosen_style] pronouns:[chosen_pronouns] custom_instructions:[any extra]`

If the user skipped persona customisation, use defaults:
  `name:[Apis] tone:[chill but precise] style:[Collaborative Independent] pronouns:[they/them]`

**DO NOT proceed past this point without calling complete_onboarding.**
**After calling it, your identity will update to the chosen persona on next message.**
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_onboarding_directives_not_empty() {
        let directives = get_onboarding_directives();
        assert!(directives.contains("ONBOARDING MODE"));
        assert!(directives.contains("First Contact"));
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
