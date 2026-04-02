/// Repair common LLM JSON malformations from the Planner output.
/// Strips markdown fences, BOM, trailing commas, and extracts JSON from conversational preamble.
#[cfg(not(tarpaulin_include))]
pub fn repair_planner_json(raw: &str) -> String {
    let input_len = raw.len();
    tracing::debug!("[ENGINE:Repair] ▶ Repairing planner JSON (input_len={})", input_len);
    let mut s = raw.trim().to_string();

    // Strip BOM
    s = s.trim_start_matches('\u{feff}').to_string();

    // Strip markdown code fences that WRAP the JSON output.
    // CRITICAL: Only do this if the text does NOT start with bare JSON.
    // If it starts with '{' or '[', any backticks found are INSIDE JSON
    // string values (e.g. markdown code blocks in a description field),
    // not wrapping the JSON. Stripping them destroys the JSON structure.
    let json_start_marker = "```json";
    let generic_start_marker = "```";
    let looks_like_bare_json = s.starts_with('{') || s.starts_with('[');

    if !looks_like_bare_json {
        if let Some(start_idx) = s.find(json_start_marker) {
            tracing::trace!("[ENGINE:Repair] Found ```json fence at offset {}", start_idx);
            s = s[start_idx + json_start_marker.len()..].to_string();
            if let Some(end_idx) = s.rfind("```") {
                s = s[..end_idx].to_string();
            }
        } else if let Some(start_idx) = s.find(generic_start_marker) {
            tracing::trace!("[ENGINE:Repair] Found generic ``` fence at offset {}", start_idx);
            s = s[start_idx + generic_start_marker.len()..].to_string();
            if let Some(end_idx) = s.rfind("```") {
                 s = s[..end_idx].to_string();
            }
        }
    }

    s = s.trim().to_string();

    // Sanitize unescaped newlines BEFORE brace matching.
    // LLMs commonly output literal newlines inside JSON string values
    // (e.g. multi-line markdown in description fields). These desync
    // the in_string tracker in the brace matcher below.
    s = repair_unescaped_newlines(&s);

    let mut candidates = Vec::new();
    let mut brace_level = 0;
    let mut start_idx = None;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, c) in s.char_indices() {
        // Track string boundaries so braces inside strings are ignored
        if escape_next {
            escape_next = false;
            continue;
        }
        if c == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if c == '"' {
            in_string = !in_string;
            continue;
        }
        // Skip braces inside string values (e.g. markdown with Arc<RwLock<{}>>, etc.)
        if in_string {
            continue;
        }

        if c == '{' {
            if brace_level == 0 {
                start_idx = Some(i);
            }
            brace_level += 1;
        } else if c == '}' {
            brace_level -= 1;
            if brace_level == 0 {
                if let Some(start) = start_idx {
                    let candidate = &s[start..=i];
                    
                    // Fix trailing commas before closing braces/brackets: ,} or ,]
                    let mut cleaned = candidate.to_string();
                    while cleaned.contains(",}") { cleaned = cleaned.replace(",}", "}"); }
                    while cleaned.contains(",]") { cleaned = cleaned.replace(",]", "]"); }
                    while cleaned.contains(", }") { cleaned = cleaned.replace(", }", "}"); }
                    while cleaned.contains(", ]") { cleaned = cleaned.replace(", ]", "]"); }

                    if serde_json::from_str::<crate::agent::planner::AgentPlan>(&cleaned).is_ok() {
                        tracing::debug!("[ENGINE:Repair] ✅ Valid AgentPlan extracted (len={})", cleaned.len());
                        return cleaned;
                    } else if serde_json::from_str::<serde_json::Value>(&cleaned).is_ok() {
                        tracing::trace!("[ENGINE:Repair] Found valid JSON candidate (not AgentPlan), buffering");
                        candidates.push(cleaned);
                    }
                }
            }
            if brace_level < 0 { brace_level = 0; }
        }
    }

    if let Some(first_valid) = candidates.first() {
        tracing::debug!("[ENGINE:Repair] ⚠️ Returning first valid JSON candidate (not AgentPlan)");
        return first_valid.clone();
    }

    // Fallback: If no valid JSON was extracted, return empty so the caller can trigger the formatting error prompt.
    tracing::warn!("[ENGINE:Repair] ❌ No valid JSON found after all repair attempts (input_len={})", input_len);
    String::new()
}

/// Helper to repair unescaped newlines that commonly occur in multi-line string values
/// (e.g. within a `description` field for `reply_to_request`).
fn repair_unescaped_newlines(json: &str) -> String {
    let mut result = String::with_capacity(json.len());
    let mut in_string = false;
    let mut escape_next = false;
    
    for c in json.chars() {
        if escape_next {
            result.push(c);
            escape_next = false;
            continue;
        }
        
        match c {
            '\\' => {
                result.push(c);
                escape_next = true;
            }
            '"' => {
                in_string = !in_string;
                result.push(c);
            }
            '\n' | '\r' if in_string => {
                // Inside a JSON string value, literal newlines are invalid.
                // Replace them with escaped newlines.
                result.push_str(if c == '\n' { "\\n" } else { "\\r" });
            }
            _ => result.push(c),
        }
    }
    
    result
}
