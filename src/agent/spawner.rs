//! Sub-Agent Spawner — Spawns and manages multiple sub-agents with 4 strategies.
//!
//! Ported from Ernos 3.0's `AgentSpawner.spawn_many()` to Rust/tokio.
//! Strategies: Parallel, Pipeline, Competitive, FanOutFanIn.

use std::sync::Arc;
use tokio::sync::mpsc;
use crate::agent::sub_agent::{SubAgentSpec, SubAgentResult, SubAgentStatus, SpawnStrategy, SpawnResult};
use crate::providers::Provider;
use crate::memory::MemoryStore;
use crate::models::capabilities::AgentCapabilities;

/// Spawn multiple sub-agents with the given strategy.
#[cfg(not(tarpaulin_include))]
pub async fn spawn_agents(
    specs: Vec<SubAgentSpec>,
    strategy: SpawnStrategy,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
    telemetry_tx: mpsc::Sender<String>,
    agent_manager: Arc<crate::agent::AgentManager>,
    capabilities: Arc<AgentCapabilities>,
) -> SpawnResult {
    let start = std::time::Instant::now();
    let total = specs.len();

    let _ = telemetry_tx.send(format!(
        "🐝 **Swarm Spawning** — {} agents, strategy: {:?}",
        total, strategy
    )).await;

    // Record spawn in lifecycle metrics
    let lifecycle = crate::agent::lifecycle::AgentLifecycle::get();
    lifecycle.record_spawn_batch(total as u64);

    let results = match strategy {
        SpawnStrategy::Parallel => {
            spawn_parallel(specs, provider, memory, telemetry_tx.clone(), agent_manager, capabilities).await
        }
        SpawnStrategy::Pipeline => {
            spawn_pipeline(specs, provider, memory, telemetry_tx.clone(), agent_manager, capabilities).await
        }
        SpawnStrategy::Competitive => {
            spawn_competitive(specs, provider, memory, telemetry_tx.clone(), agent_manager, capabilities).await
        }
        SpawnStrategy::FanOutFanIn => {
            let results = spawn_parallel(specs, provider.clone(), memory.clone(), telemetry_tx.clone(), agent_manager, capabilities).await;
            
            // Synthesize outputs
            let successful_outputs: Vec<String> = results.iter()
                .filter(|r| r.status == SubAgentStatus::Completed)
                .map(|r| r.output.clone())
                .collect();
            
            let synthesis = if successful_outputs.len() > 1 {
                Some(crate::agent::aggregator::synthesize(
                    successful_outputs, provider, &telemetry_tx
                ).await)
            } else {
                successful_outputs.first().cloned()
            };

            let successful = results.iter().filter(|r| r.status == SubAgentStatus::Completed).count();
            return SpawnResult {
                synthesis,
                successful,
                total_agents: total,
                total_duration_ms: start.elapsed().as_millis() as u64,
                results,
            };
        }
    };

    let successful = results.iter().filter(|r| r.status == SubAgentStatus::Completed).count();

    // Record individual outcomes
    for r in &results {
        match &r.status {
            SubAgentStatus::Completed => lifecycle.record_completion(r.duration_ms, r.tools_called.len()),
            SubAgentStatus::Failed(_) => lifecycle.record_failure(),
            SubAgentStatus::TimedOut => lifecycle.record_timeout(),
            SubAgentStatus::Cancelled => {},
        }
    }

    let _ = telemetry_tx.send(format!(
        "🐝 **Swarm Complete** — {}/{} succeeded in {:.1}s",
        successful, total, start.elapsed().as_secs_f64()
    )).await;

    SpawnResult {
        results,
        synthesis: None,
        total_duration_ms: start.elapsed().as_millis() as u64,
        successful,
        total_agents: total,
    }
}

/// All agents fire concurrently, collect all results.
async fn spawn_parallel(
    specs: Vec<SubAgentSpec>,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
    telemetry_tx: mpsc::Sender<String>,
    agent_manager: Arc<crate::agent::AgentManager>,
    capabilities: Arc<AgentCapabilities>,
) -> Vec<SubAgentResult> {
    let mut handles = vec![];

    for (i, spec) in specs.into_iter().enumerate() {
        let agent_id = format!("swarm-{}", i + 1);
        let p = provider.clone();
        let m = memory.clone();
        let tx = telemetry_tx.clone();
        let am = agent_manager.clone();
        let caps = capabilities.clone();

        handles.push(tokio::spawn(async move {
            crate::agent::sub_agent::execute_sub_agent(
                agent_id, spec, p, m, tx, am, caps, None,
            ).await
        }));
    }

    let mut results = vec![];
    for h in handles {
        match h.await {
            Ok(r) => results.push(r),
            Err(e) => {
                tracing::error!("[SPAWNER] Agent task panicked: {:?}", e);
                results.push(SubAgentResult {
                    agent_id: "panicked".into(),
                    output: format!("Agent panicked: {:?}", e),
                    status: SubAgentStatus::Failed("Panic".into()),
                    tools_called: vec![],
                    duration_ms: 0,
                    turns_used: 0,
                });
            }
        }
    }
    results
}

/// Sequential chain — each agent's output is injected into the next agent's context.
async fn spawn_pipeline(
    specs: Vec<SubAgentSpec>,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
    telemetry_tx: mpsc::Sender<String>,
    agent_manager: Arc<crate::agent::AgentManager>,
    capabilities: Arc<AgentCapabilities>,
) -> Vec<SubAgentResult> {
    let mut results = vec![];
    let mut pipeline_context: Option<String> = None;

    for (i, spec) in specs.into_iter().enumerate() {
        let agent_id = format!("pipe-{}", i + 1);

        let result = crate::agent::sub_agent::execute_sub_agent(
            agent_id,
            spec,
            provider.clone(),
            memory.clone(),
            telemetry_tx.clone(),
            agent_manager.clone(),
            capabilities.clone(),
            pipeline_context.clone(),
        ).await;

        // Feed this agent's output into the next agent
        if result.status == SubAgentStatus::Completed {
            pipeline_context = Some(result.output.clone());
        }

        results.push(result);
    }
    results
}

/// Race — first successful result wins, cancel the rest.
async fn spawn_competitive(
    specs: Vec<SubAgentSpec>,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
    telemetry_tx: mpsc::Sender<String>,
    agent_manager: Arc<crate::agent::AgentManager>,
    capabilities: Arc<AgentCapabilities>,
) -> Vec<SubAgentResult> {
    use tokio::sync::mpsc as tokio_mpsc;

    let count = specs.len();
    let (result_tx, mut result_rx) = tokio_mpsc::channel::<SubAgentResult>(count);

    let mut handles = vec![];
    for (i, spec) in specs.into_iter().enumerate() {
        let agent_id = format!("racer-{}", i + 1);
        let p = provider.clone();
        let m = memory.clone();
        let tx = telemetry_tx.clone();
        let am = agent_manager.clone();
        let caps = capabilities.clone();
        let rtx = result_tx.clone();

        handles.push(tokio::spawn(async move {
            let result = crate::agent::sub_agent::execute_sub_agent(
                agent_id, spec, p, m, tx, am, caps, None,
            ).await;
            let _ = rtx.send(result).await;
        }));
    }
    drop(result_tx); // Close the sender so rx terminates when all done

    let mut results = vec![];
    let mut winner_found = false;

    while let Some(result) = result_rx.recv().await {
        if result.status == SubAgentStatus::Completed && !winner_found {
            winner_found = true;
            let _ = telemetry_tx.send(format!(
                "🏆 **[{}]** Won the race! Cancelling others...",
                result.agent_id
            )).await;
            results.push(result);
            // Abort remaining tasks
            for h in &handles {
                h.abort();
            }
            break;
        } else {
            results.push(result);
        }
    }

    if !winner_found && results.is_empty() {
        results.push(SubAgentResult {
            agent_id: "race-failed".into(),
            output: "No agent completed successfully in the race.".into(),
            status: SubAgentStatus::Failed("All racers failed".into()),
            tools_called: vec![],
            duration_ms: 0,
            turns_used: 0,
        });
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::MockProvider;
    use crate::models::scope::Scope;

    fn make_test_spec(task: &str) -> SubAgentSpec {
        SubAgentSpec {
            task: task.into(),
            max_turns: 3,
            timeout_secs: 10,
            scope: Scope::Private { user_id: "test".into() },
            user_id: "test".into(),
        }
    }

    #[tokio::test]
    async fn test_parallel_spawn_multiple() {
        let mut mock = MockProvider::new();
        mock.expect_generate()
            .returning(|_, _, _, _, _, _| {
                Ok(r#"{"thought":"Done","tasks":[{"task_id":"r","tool_type":"reply_to_request","description":"Agent output","depends_on":[]}]}"#.to_string())
            });

        let provider: Arc<dyn Provider> = Arc::new(mock);
        let memory = Arc::new(MemoryStore::default());
        let (tx, mut _rx) = mpsc::channel(100);
        let agent_mgr = Arc::new(crate::agent::AgentManager::new(provider.clone(), memory.clone()));
        let capabilities = Arc::new(AgentCapabilities::default());

        let specs = vec![
            make_test_spec("Task A"),
            make_test_spec("Task B"),
            make_test_spec("Task C"),
        ];

        let result = spawn_agents(
            specs, SpawnStrategy::Parallel, provider, memory,
            tx, agent_mgr, capabilities,
        ).await;

        assert_eq!(result.total_agents, 3);
        assert_eq!(result.successful, 3);
        assert!(result.synthesis.is_none());
    }

    #[tokio::test]
    async fn test_strategy_from_str() {
        assert_eq!(SpawnStrategy::from_str("pipeline"), SpawnStrategy::Pipeline);
        assert_eq!(SpawnStrategy::from_str("competitive"), SpawnStrategy::Competitive);
    }
}
