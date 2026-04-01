use std::sync::Arc;
use crate::models::message::Event;
use crate::models::scope::Scope;
use crate::providers::Provider;

/// 4-Phase Context Consolidation Engine.
///
/// Compresses old working memory events into a running summary before they
/// are silently dropped by the HIVE_WORKING_MEMORY_CAP hard cap.
///
/// Phase 1: ORIENT  — Check if consolidation is needed (>80% cap usage)
/// Phase 2: GATHER  — Identify events that will be dropped at next cap enforcement
/// Phase 3: CONSOLIDATE — Use the observer model to summarize gathered events
/// Phase 4: INJECT  — Write the summary to a persistent file and create a synthetic event
pub struct ContextConsolidator;

impl ContextConsolidator {
    /// Phase 1: ORIENT — Returns true if the scope is above 80% of its message cap.
    pub fn needs_consolidation(absolute_count: usize, cap: usize) -> bool {
        cap > 0 && absolute_count > (cap * 80 / 100)
    }

    /// Phase 2: GATHER — Given the full history (pre-cap) and the cap, return
    /// the events that will be dropped when the cap is enforced.
    /// These are history[0..overshoot] where overshoot = len - cap.
    pub fn gather_candidates(history: &[Event], cap: usize) -> Vec<Event> {
        if history.len() <= cap {
            return Vec::new();
        }
        let overshoot = history.len() - cap;
        history[..overshoot].to_vec()
    }

    /// Phase 3: CONSOLIDATE — Summarize the candidates using the observer model.
    ///
    /// Uses the Provider::generate() trait method, which is the only method
    /// available on the Provider trait (see providers/mod.rs:31-43).
    /// We construct a synthetic Event carrying the consolidation prompt,
    /// pass it as `new_event`, and use the system_prompt to set the role.
    pub async fn consolidate(
        provider: Arc<dyn Provider>,
        candidates: &[Event],
        existing_summary: Option<&str>,
        scope: &Scope,
    ) -> String {
        if candidates.is_empty() {
            return existing_summary.unwrap_or("").to_string();
        }

        // Build a transcript of the candidates
        let mut transcript = String::new();
        for event in candidates {
            let author = &event.author_name;
            let time = event.timestamp.as_deref().unwrap_or("?");
            // Truncate individual messages to prevent massive prompts
            let content = if event.content.len() > 500 {
                format!("{}... [truncated, {} chars total]", &event.content[..500], event.content.len())
            } else {
                event.content.clone()
            };
            transcript.push_str(&format!("[{}] {}: {}\n", time, author, content));
        }

        let mut user_prompt = String::new();
        if let Some(existing) = existing_summary {
            if !existing.is_empty() {
                user_prompt.push_str(&format!(
                    "EXISTING CONTEXT SUMMARY (from earlier consolidation):\n{}\n\n",
                    existing
                ));
            }
        }
        user_prompt.push_str(&format!(
            "NEW CONVERSATION SEGMENT ({} messages about to leave the context window):\n{}\n\n\
            TASK: Produce a concise, factual summary that preserves:\n\
            1. Key decisions made\n\
            2. Important facts or data mentioned\n\
            3. User requests and whether they were fulfilled\n\
            4. Technical context (file paths, function names, error messages)\n\
            5. Any unresolved items or pending actions\n\n\
            If there is an existing summary, merge the new information into it.\n\
            Keep the summary under 800 characters. Be precise, not verbose.\n\
            Output ONLY the summary, nothing else.",
            candidates.len(),
            transcript
        ));

        let system_prompt = "You are a context consolidation engine. You produce concise, factual summaries of conversation segments. Output ONLY the summary text.";

        // Construct a synthetic Event to carry the prompt through Provider::generate()
        // Provider::generate signature: (system_prompt, history, new_event, agent_context, telemetry_tx, max_tokens)
        let synthetic_event = Event {
            platform: "system".into(),
            scope: scope.clone(),
            author_name: "Consolidation Engine".into(),
            author_id: "system".into(),
            content: user_prompt,
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
        };

        match provider.generate(
            system_prompt,
            &[],              // empty history — we don't need prior context for summarization
            &synthetic_event, // the consolidation prompt as the "new event"
            "",               // no agent_context
            None,             // no telemetry
            Some(512),        // cap output tokens — summaries should be short
        ).await {
            Ok(summary) => {
                let summary = summary.trim().to_string();
                tracing::info!(
                    "[CONSOLIDATOR] Phase 3 complete: {} candidates → {} char summary",
                    candidates.len(),
                    summary.len()
                );
                summary
            }
            Err(e) => {
                tracing::warn!("[CONSOLIDATOR] Provider failed, falling back to transcript excerpt: {}", e);
                // Fallback: just keep a truncated version of the transcript
                let fallback = if transcript.len() > 600 {
                    format!("{}...", &transcript[..600])
                } else {
                    transcript
                };
                format!("[Auto-summary fallback] {}", fallback)
            }
        }
    }

    /// Phase 4: INJECT — Create a synthetic Event that carries the summary.
    /// This event gets injected into the working memory so the LLM can see
    /// consolidated context from before the cap window.
    pub fn make_summary_event(summary: &str, scope: &Scope) -> Event {
        Event {
            platform: "system".into(),
            scope: scope.clone(),
            author_name: "Context Consolidation Engine".into(),
            author_id: "system".into(),
            content: format!(
                "[CONSOLIDATED CONTEXT — Earlier conversation summary]\n{}",
                summary
            ),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: Some(0), // Always slot 0
        }
    }

    /// Get the path where consolidated summaries are persisted per-scope.
    pub fn summary_path(scope: &Scope) -> std::path::PathBuf {
        let dir = std::path::PathBuf::from("memory/core/consolidation");
        let _ = std::fs::create_dir_all(&dir);
        let key = scope.to_key().replace('/', "_");
        dir.join(format!("{}.txt", key))
    }

    /// Read the existing persisted summary for a scope, if any.
    pub async fn read_existing_summary(scope: &Scope) -> Option<String> {
        let path = Self::summary_path(scope);
        match tokio::fs::read_to_string(&path).await {
            Ok(s) if !s.trim().is_empty() => Some(s),
            _ => None,
        }
    }

    /// Write the consolidated summary to disk for persistence.
    pub async fn write_summary(scope: &Scope, summary: &str) {
        let path = Self::summary_path(scope);
        if let Err(e) = tokio::fs::write(&path, summary).await {
            tracing::warn!("[CONSOLIDATOR] Failed to persist summary: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_needs_consolidation_below_threshold() {
        assert!(!ContextConsolidator::needs_consolidation(50, 100));
        assert!(!ContextConsolidator::needs_consolidation(79, 100));
        assert!(!ContextConsolidator::needs_consolidation(80, 100));
    }

    #[test]
    fn test_needs_consolidation_above_threshold() {
        assert!(ContextConsolidator::needs_consolidation(81, 100));
        assert!(ContextConsolidator::needs_consolidation(100, 100));
        assert!(ContextConsolidator::needs_consolidation(150, 100));
    }

    #[test]
    fn test_needs_consolidation_zero_cap() {
        assert!(!ContextConsolidator::needs_consolidation(10, 0));
    }

    #[test]
    fn test_gather_candidates_no_overshoot() {
        let events = make_events(50);
        let candidates = ContextConsolidator::gather_candidates(&events, 100);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_gather_candidates_with_overshoot() {
        let events = make_events(120);
        let candidates = ContextConsolidator::gather_candidates(&events, 100);
        assert_eq!(candidates.len(), 20);
        // First 20 events should be the candidates
        assert_eq!(candidates[0].content, "message_0");
        assert_eq!(candidates[19].content, "message_19");
    }

    #[test]
    fn test_gather_candidates_exact_cap() {
        let events = make_events(100);
        let candidates = ContextConsolidator::gather_candidates(&events, 100);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_make_summary_event() {
        let scope = Scope::Private { user_id: "test".into() };
        let event = ContextConsolidator::make_summary_event("test summary", &scope);
        assert_eq!(event.author_name, "Context Consolidation Engine");
        assert_eq!(event.author_id, "system");
        assert!(event.content.contains("test summary"));
        assert!(event.content.contains("CONSOLIDATED CONTEXT"));
        assert_eq!(event.message_index, Some(0));
    }

    #[test]
    fn test_summary_path() {
        let scope = Scope::Private { user_id: "alice".into() };
        let path = ContextConsolidator::summary_path(&scope);
        assert!(path.to_string_lossy().contains("consolidation"));
        assert!(path.to_string_lossy().contains("priv_alice"));
    }

    #[tokio::test]
    async fn test_consolidate_empty_candidates() {
        // With empty candidates, should return existing summary without calling provider
        let mut mock = crate::providers::MockProvider::new();
        // generate should NOT be called for empty candidates
        mock.expect_generate().times(0);
        let provider: Arc<dyn Provider> = Arc::new(mock);
        let scope = Scope::Private { user_id: "test".into() };

        let result = ContextConsolidator::consolidate(
            provider, &[], Some("existing stuff"), &scope,
        ).await;
        assert_eq!(result, "existing stuff");
    }

    #[tokio::test]
    async fn test_consolidate_with_candidates() {
        let mut mock = crate::providers::MockProvider::new();
        mock.expect_generate()
            .times(1)
            .returning(|_sys, _hist, event, _ctx, _tx, _max| {
                // Verify the prompt contains the candidate messages
                assert!(event.content.contains("message_0"));
                assert!(event.content.contains("TASK: Produce a concise"));
                Ok("Summary of 5 messages: users discussed testing.".into())
            });
        let provider: Arc<dyn Provider> = Arc::new(mock);
        let scope = Scope::Private { user_id: "test".into() };
        let candidates = make_events(5);

        let result = ContextConsolidator::consolidate(
            provider, &candidates, None, &scope,
        ).await;
        assert_eq!(result, "Summary of 5 messages: users discussed testing.");
    }

    #[tokio::test]
    async fn test_consolidate_provider_failure_falls_back() {
        let mut mock = crate::providers::MockProvider::new();
        mock.expect_generate()
            .times(1)
            .returning(|_, _, _, _, _, _| {
                Err(crate::providers::ProviderError::ConnectionError("timeout".into()))
            });
        let provider: Arc<dyn Provider> = Arc::new(mock);
        let scope = Scope::Private { user_id: "test".into() };
        let candidates = make_events(3);

        let result = ContextConsolidator::consolidate(
            provider, &candidates, None, &scope,
        ).await;
        assert!(result.starts_with("[Auto-summary fallback]"));
        assert!(result.contains("message_0"));
    }

    #[tokio::test]
    async fn test_consolidate_with_existing_summary() {
        let mut mock = crate::providers::MockProvider::new();
        mock.expect_generate()
            .times(1)
            .returning(|_sys, _hist, event, _ctx, _tx, _max| {
                // Verify existing summary is included in the prompt
                assert!(event.content.contains("EXISTING CONTEXT SUMMARY"));
                assert!(event.content.contains("prior context here"));
                Ok("Merged summary.".into())
            });
        let provider: Arc<dyn Provider> = Arc::new(mock);
        let scope = Scope::Private { user_id: "test".into() };
        let candidates = make_events(2);

        let result = ContextConsolidator::consolidate(
            provider, &candidates, Some("prior context here"), &scope,
        ).await;
        assert_eq!(result, "Merged summary.");
    }

    #[tokio::test]
    async fn test_read_write_summary() {
        let scope = Scope::Private { 
            user_id: format!("test_consolidation_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())
        };
        
        // Should be None initially
        let existing = ContextConsolidator::read_existing_summary(&scope).await;
        assert!(existing.is_none());

        // Write and read back
        ContextConsolidator::write_summary(&scope, "consolidated text").await;
        let read_back = ContextConsolidator::read_existing_summary(&scope).await;
        assert_eq!(read_back.unwrap(), "consolidated text");

        // Cleanup
        let path = ContextConsolidator::summary_path(&scope);
        let _ = tokio::fs::remove_file(&path).await;
    }

    fn make_events(count: usize) -> Vec<Event> {
        (0..count).map(|i| Event {
            platform: "test".into(),
            scope: Scope::Private { user_id: "test".into() },
            author_name: "user".into(),
            author_id: "user1".into(),
            content: format!("message_{}", i),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: Some(i),
        }).collect()
    }
}
