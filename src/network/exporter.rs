/// MeshExporter — The ONLY data bridge between HIVE memory and the mesh.
///
/// This trait is the architectural isolation boundary. The `src/network/` module
/// receives data ONLY through implementations of this trait, which MUST:
/// 1. Return only global-scope data (never user-scoped)
/// 2. Strip all user-identifying information (Discord IDs, usernames, emails)
/// 3. Anonymize golden examples (remove user prompts, keep only response quality)
///
/// The network module has NO imports to `memory::working`, `memory::timeline`,
/// `memory::scratch`, or any scoped system. This is enforced by Rust's module
/// visibility — not by policy.
use crate::memory::lessons::Lesson;
use crate::memory::synaptic::{SynapticNode, SynapticEdge};
use crate::teacher::generation::GoldenExample;

/// Sanitizer: strips PII patterns from text before it crosses the mesh boundary.
pub fn sanitize_for_mesh(text: &str) -> String {
    use regex::Regex;

    let mut clean = text.to_string();

    // Email addresses (MUST run before Discord ID regex to avoid partial mangling)
    let email = Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap();
    clean = email.replace_all(&clean, "[REDACTED_EMAIL]").to_string();

    // Discord user IDs (17-19 digit numbers)
    let discord_id = Regex::new(r"\b\d{17,19}\b").unwrap();
    clean = discord_id.replace_all(&clean, "[REDACTED_ID]").to_string();

    // @mentions
    let mention = Regex::new(r"@[\w.-]+").unwrap();
    clean = mention.replace_all(&clean, "@[REDACTED]").to_string();

    // Phone numbers (various formats)
    let phone = Regex::new(r"\b(?:\+?\d{1,3}[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b").unwrap();
    clean = phone.replace_all(&clean, "[REDACTED_PHONE]").to_string();

    clean
}

/// Check if text contains any PII patterns. Returns true if PII is detected.
pub fn contains_pii(text: &str) -> bool {
    use regex::Regex;

    let patterns = [
        r"\b\d{17,19}\b",                                           // Discord IDs
        r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}",       // Emails
        r"\b(?:\+?\d{1,3}[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b", // Phones
    ];

    for pat in &patterns {
        if let Ok(re) = Regex::new(pat) {
            if re.is_match(text) {
                return true;
            }
        }
    }
    false
}

/// The sole interface between HIVE memory and the NeuroLease mesh.
/// Implementations MUST strip all user-identifying information.
pub trait MeshExporter: Send + Sync {
    /// Returns global-scope lessons only. NEVER scoped lessons.
    /// All lesson text is sanitized through `sanitize_for_mesh()`.
    fn export_lessons(&self) -> Vec<Lesson>;

    /// Returns synaptic nodes with user-referencing data entries stripped.
    /// Nodes whose concept name matches a known username are excluded.
    fn export_synaptic(&self) -> (Vec<SynapticNode>, Vec<SynapticEdge>);

    /// Returns golden examples with user prompts anonymized.
    /// Only the response quality matters for distributed training.
    fn export_golden(&self) -> Vec<GoldenExample>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_discord_ids() {
        let input = "User 1299810741984956449 said hello";
        let cleaned = sanitize_for_mesh(input);
        assert!(!cleaned.contains("1299810741984956449"));
        assert!(cleaned.contains("[REDACTED_ID]"));
    }

    #[test]
    fn test_sanitize_emails() {
        let input = "Contact me at user@example.com for details";
        let cleaned = sanitize_for_mesh(input);
        assert!(!cleaned.contains("user@example.com"));
        assert!(cleaned.contains("[REDACTED_EMAIL]"));
    }

    #[test]
    fn test_sanitize_mentions() {
        let input = "Hey @metta_mazza check this out";
        let cleaned = sanitize_for_mesh(input);
        assert!(!cleaned.contains("@metta_mazza"));
    }

    #[test]
    fn test_contains_pii_detection() {
        assert!(contains_pii("User 1299810741984956449"));
        assert!(contains_pii("email: test@example.com"));
        assert!(!contains_pii("The sky is blue"));
        assert!(!contains_pii("Rust is a systems language"));
    }

    #[test]
    fn test_sanitize_clean_text_unchanged() {
        let input = "Rust memory model uses ownership and borrowing";
        let cleaned = sanitize_for_mesh(input);
        assert_eq!(input, cleaned);
    }
}
