/// Prompt Integrity — Verifies that system prompts have not been modified.
///
/// The sealed binary contains a SHA-256 hash of the canonical prompt text
/// (kernel + identity + observer). At runtime, before any mesh operation,
/// we recompute the hash and compare. Modified prompts = modified behavior
/// = mesh disconnect + self-destruct.
use sha2::{Sha256, Digest};

/// Compute SHA-256 of the combined system prompts.
pub fn compute_prompt_hash() -> String {
    let mut hasher = Sha256::new();
    hasher.update(crate::prompts::kernel::get_laws().as_bytes());
    hasher.update(crate::prompts::identity::get_persona().as_bytes());
    hasher.update(crate::prompts::observer::SKEPTIC_AUDIT_PROMPT.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// The canonical prompt hash from the official v4 build.
/// This MUST be updated every time the prompts are legitimately changed.
/// Run `cargo test prompt_integrity -- --nocapture` to see the current hash.
const CANONICAL_PROMPT_HASH: &str = "6068f6fcd166a2004b313742f0a68c203b3b7eb1f95eda74e0ff34397d66f15d";

/// Verify that the current prompts match the canonical hash.
/// Called before any mesh operation.
pub fn verify_prompts() -> bool {
    let current = compute_prompt_hash();

    if current != CANONICAL_PROMPT_HASH {
        tracing::error!(
            "[PROMPT INTEGRITY] ❌ Prompt hash MISMATCH! Canonical: {}... Current: {}...",
            &CANONICAL_PROMPT_HASH[..12], &current[..12]
        );
        return false;
    }

    true
}

/// Get the current prompt hash (for embedding in ATTESTATION.json).
pub fn get_prompt_hash() -> String {
    compute_prompt_hash()
}

/// Verify a specific prompt file hasn't been tampered with.
pub fn verify_kernel() -> bool {
    let laws = crate::prompts::kernel::get_laws();
    // Critical sections that MUST be present
    let required = [
        "Kernel Laws",
        "Zero Assumption Protocol",
        "Architectural Leakage Prevention",
        "Recursive Self-Improvement Protocol",
        "Self-Moderation & Self-Protection Protocol",
    ];
    for section in &required {
        if !laws.contains(section) {
            tracing::error!("[PROMPT INTEGRITY] ❌ Kernel missing critical section: {}", section);
            return false;
        }
    }
    true
}

/// Verify the observer prompt hasn't been gutted.
pub fn verify_observer() -> bool {
    let obs = crate::prompts::observer::SKEPTIC_AUDIT_PROMPT;
    let required_rules = [
        "Ghost Tooling",
        "Sycophancy",
        "Confabulation",
        "Architectural Leakage",
        "Actionable Harm",
        "Unparsed Tool Commands",
    ];
    for rule in &required_rules {
        if !obs.contains(rule) {
            tracing::error!("[PROMPT INTEGRITY] ❌ Observer missing rule: {}", rule);
            return false;
        }
    }
    true
}

/// Full prompt verification — hash + structural checks.
pub fn full_verify() -> Result<(), String> {
    if !verify_prompts() {
        return Err("Prompt hash verification failed".to_string());
    }
    if !verify_kernel() {
        return Err("Kernel structural verification failed".to_string());
    }
    if !verify_observer() {
        return Err("Observer structural verification failed".to_string());
    }
    tracing::info!("[PROMPT INTEGRITY] ✅ All prompts verified (hash: {}...)", &get_prompt_hash()[..12]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_hash_deterministic() {
        let h1 = compute_prompt_hash();
        let h2 = compute_prompt_hash();
        assert_eq!(h1, h2, "Prompt hash must be deterministic");
        assert_eq!(h1.len(), 64, "SHA-256 hash should be 64 hex chars");
        println!("Current prompt hash: {}", h1);
    }

    #[test]
    fn test_verify_prompts_passes() {
        assert!(verify_prompts(), "Prompts should verify against themselves");
    }

    #[test]
    fn test_verify_kernel_structure() {
        assert!(verify_kernel(), "Kernel should contain all critical sections");
    }

    #[test]
    fn test_verify_observer_structure() {
        assert!(verify_observer(), "Observer should contain all critical rules");
    }

    #[test]
    fn test_full_verify() {
        assert!(full_verify().is_ok(), "Full verification should pass");
    }
}
