use crate::models::tool::{ToolResult, ToolStatus};
use crate::memory::MemoryStore;
use crate::agent::preferences::extract_tag;
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn execute_operate_synaptic_graph(
    task_id: String,
    description: String,
    memory: Arc<MemoryStore>,
    telemetry_tx: Option<mpsc::Sender<String>>,
) -> ToolResult {
    if let Some(ref tx) = telemetry_tx {
        let _ = tx.send("🕸️ Synaptic Drone executing...\n".to_string()).await;
    }
    tracing::debug!("[AGENT:synaptic] ▶ task_id={}", task_id);

    let action = extract_tag(&description, "action:").unwrap_or_default().to_lowercase();
    let concept = extract_tag(&description, "concept:").unwrap_or_default();
    let data = extract_tag(&description, "data:").unwrap_or_default();

    if action.is_empty() {
        return ToolResult { 
            task_id, 
            output: "Error: Missing 'action:' field.".to_string(), 
            tokens_used: 0, 
            status: ToolStatus::Failed("Missing action field".into()) 
        };
    }

    match action.as_str() {
        "store" => {
            if concept.is_empty() {
                return ToolResult { task_id, output: "Error: Missing 'concept:' field for store action.".to_string(), tokens_used: 0, status: ToolStatus::Failed("Missing field".into()) };
            }
            if data.is_empty() {
                return ToolResult { task_id, output: "Error: Missing 'data:' field for store action.".to_string(), tokens_used: 0, status: ToolStatus::Failed("Missing field".into()) };
            }
            memory.synaptic.store(&concept, &data).await;
            ToolResult { task_id, output: format!("Concept '{}' stored in the synaptic graph.", concept), tokens_used: 0, status: ToolStatus::Success }
        }
        "search" => {
            if concept.is_empty() {
                return ToolResult { task_id, output: "Error: Missing 'concept:' field for search action.".to_string(), tokens_used: 0, status: ToolStatus::Failed("Missing field".into()) };
            }
            let results = memory.synaptic.search(&concept).await;
            if results.is_empty() {
                ToolResult { task_id, output: format!("No nodes found for concept '{}'.", concept), tokens_used: 0, status: ToolStatus::Success }
            } else {
                ToolResult { task_id, output: format!("Synaptic Results for '{}':\n{}", concept, results.join("\n")), tokens_used: 0, status: ToolStatus::Success }
            }
        }
        "beliefs" => {
            let limit_str = extract_tag(&description, "limit:").unwrap_or("10".to_string());
            let limit: usize = limit_str.parse().unwrap_or(10);
            
            let beliefs = memory.synaptic.get_beliefs(limit).await;
            if beliefs.is_empty() {
                ToolResult { task_id, output: "No core beliefs firmly established in the graph yet.".to_string(), tokens_used: 0, status: ToolStatus::Success }
            } else {
                ToolResult { task_id, output: format!("Core System Beliefs:\n- {}", beliefs.join("\n- ")), tokens_used: 0, status: ToolStatus::Success }
            }
        }
        "relate" => {
            let from = extract_tag(&description, "from:").unwrap_or_default();
            let to = extract_tag(&description, "to:").unwrap_or_default();
            let relation = extract_tag(&description, "relation:").unwrap_or_default();
            if from.is_empty() || to.is_empty() || relation.is_empty() {
                return ToolResult { task_id, output: "Error: 'relate' requires 'from:', 'to:', and 'relation:' fields. Example: action:[relate] from:[Apple] relation:[is_a] to:[Fruit]".to_string(), tokens_used: 0, status: ToolStatus::Failed("Missing fields".into()) };
            }
            memory.synaptic.store_relationship(&from, &relation, &to).await;
            ToolResult { task_id, output: format!("Relationship stored: '{}' --[{}]--> '{}'", from, relation, to), tokens_used: 0, status: ToolStatus::Success }
        }
        "stats" => {
            let (nodes, edges) = memory.synaptic.stats().await;
            ToolResult { task_id, output: format!("Synaptic Graph Stats:\n- Nodes (concepts): {}\n- Edges (relationships): {}", nodes, edges), tokens_used: 0, status: ToolStatus::Success }
        }
        _ => ToolResult {
            task_id,
            output: format!("Unknown action '{}'. Valid actions: store, search, beliefs, relate, stats.", action),
            tokens_used: 0,
            status: ToolStatus::Failed("Unknown action".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_operate_synaptic_graph() {
        let mem = Arc::new(MemoryStore::default());

        // Test missing action
        let res = execute_operate_synaptic_graph("1".into(), "".into(), mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Failed("Missing action field".into()));

        // Test store missing fields
        let res = execute_operate_synaptic_graph("2".into(), "action:[store] concept:[A]".into(), mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Failed("Missing field".into()));
        
        let res = execute_operate_synaptic_graph("3".into(), "action:[store] data:[B]".into(), mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Failed("Missing field".into()));

        // Test successful store
        let res = execute_operate_synaptic_graph("4".into(), "action:[store] concept:[Apple] data:[is red]".into(), mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Success);
        assert!(res.output.contains("stored"));

        // Test search missing fields
        let res = execute_operate_synaptic_graph("5".into(), "action:[search]".into(), mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Failed("Missing field".into()));

        // Test successful search — should now return real data
        let res = execute_operate_synaptic_graph("6".into(), "action:[search] concept:[Apple]".into(), mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Success);
        assert!(res.output.contains("is red"), "Search should return stored data, got: {}", res.output);

        // Test beliefs — should show Apple as the top concept
        let res = execute_operate_synaptic_graph("7".into(), "action:[beliefs]".into(), mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Success);
        assert!(res.output.contains("Apple"), "Beliefs should include Apple, got: {}", res.output);

        // Test relate
        let res = execute_operate_synaptic_graph("8".into(), "action:[relate] from:[Apple] relation:[is_a] to:[Fruit]".into(), mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Success);
        assert!(res.output.contains("Relationship stored"));

        // Test relate missing fields
        let res = execute_operate_synaptic_graph("9".into(), "action:[relate] from:[Apple]".into(), mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Failed("Missing fields".into()));

        // Test stats
        let res = execute_operate_synaptic_graph("10".into(), "action:[stats]".into(), mem.clone(), None).await;
        assert_eq!(res.status, ToolStatus::Success);
        assert!(res.output.contains("Nodes (concepts): 1"), "Expected 1 node, got: {}", res.output);
        assert!(res.output.contains("Edges (relationships): 1"), "Expected 1 edge, got: {}", res.output);
    }
}
