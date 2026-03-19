//! Result Aggregator — LLM-driven synthesis of multiple sub-agent outputs.
//!
//! Ported from Ernos 3.0's `ResultAggregator.synthesize()`.
//! Takes N agent outputs and produces one coherent merged response.

use std::sync::Arc;
use tokio::sync::mpsc;
use crate::providers::Provider;
use crate::models::message::Event;
use crate::models::scope::Scope;

/// Synthesize multiple sub-agent outputs into a single coherent response
/// using an LLM merge call.
#[cfg(not(tarpaulin_include))]
pub async fn synthesize(
    outputs: Vec<String>,
    provider: Arc<dyn Provider>,
    telemetry_tx: &mpsc::Sender<String>,
) -> String {
    if outputs.is_empty() {
        return "No agent outputs to synthesize.".into();
    }
    if outputs.len() == 1 {
        return outputs.into_iter().next().unwrap();
    }

    let _ = telemetry_tx.send(format!(
        "🧬 **Synthesizing** {} agent outputs into unified response...",
        outputs.len()
    )).await;

    let mut combined = String::new();
    for (i, output) in outputs.iter().enumerate() {
        combined.push_str(&format!(
            "--- Agent {} Output ---\n{}\n\n",
            i + 1, output
        ));
    }

    let system_prompt = format!(
        "You are a Result Synthesizer within the HIVE swarm.\n\
         You have received outputs from {} independent sub-agents that worked on related tasks.\n\n\
         YOUR JOB:\n\
         1. Read ALL agent outputs carefully.\n\
         2. Merge them into ONE coherent, well-structured response.\n\
         3. Remove redundancies — don't repeat the same information.\n\
         4. Preserve ALL unique findings from each agent.\n\
         5. If agents contradict each other, note the disagreement.\n\
         6. Write the final synthesis as if YOU did all the research.\n\
         7. Do NOT mention \"Agent 1 said...\" or \"Agent 2 found...\" — just present the unified findings.\n\n\
         Output ONLY the synthesized response. No preamble, no meta-commentary.",
        outputs.len()
    );

    let dummy_event = Event {
        platform: "agent".into(),
        scope: Scope::Private { user_id: "synthesizer".into() },
        author_name: "Aggregator".into(),
        author_id: "internal".into(),
        content: "Synthesize agent outputs.".into(),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
            message_index: None,
    };

    match provider.generate(&system_prompt, &[], &dummy_event, &combined, None, None).await {
        Ok(synthesis) => {
            let _ = telemetry_tx.send("🧬 **Synthesis complete**".into()).await;
            synthesis
        }
        Err(e) => {
            tracing::error!("[AGGREGATOR] Synthesis LLM call failed: {:?}", e);
            // Fallback: just concatenate outputs with headers
            let mut fallback = String::from("## Combined Agent Results\n\n");
            for (i, output) in outputs.iter().enumerate() {
                fallback.push_str(&format!("### Agent {}\n{}\n\n", i + 1, output));
            }
            fallback
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::MockProvider;

    #[tokio::test]
    async fn test_synthesize_single_output() {
        let (tx, _rx) = mpsc::channel(10);
        let mock = MockProvider::new();
        let provider: Arc<dyn Provider> = Arc::new(mock);
        
        let result = synthesize(vec!["Only one output".into()], provider, &tx).await;
        assert_eq!(result, "Only one output");
    }

    #[tokio::test]
    async fn test_synthesize_empty() {
        let (tx, _rx) = mpsc::channel(10);
        let mock = MockProvider::new();
        let provider: Arc<dyn Provider> = Arc::new(mock);
        
        let result = synthesize(vec![], provider, &tx).await;
        assert_eq!(result, "No agent outputs to synthesize.");
    }

    #[tokio::test]
    async fn test_synthesize_multiple_calls_provider() {
        let mut mock = MockProvider::new();
        mock.expect_generate()
            .returning(|sys, _, _, ctx, _, _| {
                assert!(sys.contains("Result Synthesizer"));
                assert!(ctx.contains("Agent 1 Output"));
                assert!(ctx.contains("Agent 2 Output"));
                Ok("Unified synthesis of all findings.".into())
            });

        let provider: Arc<dyn Provider> = Arc::new(mock);
        let (tx, _rx) = mpsc::channel(10);

        let result = synthesize(
            vec!["Finding A".into(), "Finding B".into()],
            provider,
            &tx,
        ).await;

        assert_eq!(result, "Unified synthesis of all findings.");
    }
}
