use super::*;
use crate::providers::MockProvider;
use crate::models::tool::ToolStatus;

#[tokio::test]
async fn test_agent_manager_registration() {
    let provider = Arc::new(MockProvider::new());
    let memory = Arc::new(MemoryStore::default());
    let mut agent = AgentManager::new(provider, memory);
    
    let template = ToolTemplate {
        name: "test_tool".into(),
        system_prompt: "sys".into(),
        tools: vec![],
    };
    
    agent.register_tool(template.clone());
    assert!(agent.get_template("test_tool").is_some());
}

#[tokio::test]
async fn test_agent_execute_plan_success() {
    let mut mock_provider = MockProvider::new();
    mock_provider
        .expect_generate()
        .returning(|_, _, _, _, _| Ok("Tool output".to_string()));

    let memory = Arc::new(MemoryStore::default());
    let agent = AgentManager::new(Arc::new(mock_provider), memory);
    
    let plan = crate::agent::planner::AgentPlan {
        thought: Some("I should do research".to_string()),
        tasks: vec![
            crate::agent::planner::AgentTask {
                task_id: "1".into(),
                tool_type: "researcher".into(),
                description: "do research".into(),
                depends_on: vec![],
                source: None,
            }
        ],
    };

    let results = agent.execute_plan(plan, "User said hello", crate::models::scope::Scope::Private { user_id: "test".into() }, None).await;
    
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].task_id, "1");
    assert!(
        results[0].output.contains("SEARCH RESULTS for") || 
        results[0].output.contains("GOOGLE NEWS RSS") ||
        results[0].output.contains("Tool output")
    );
    assert_eq!(results[0].status, ToolStatus::Success);
}

#[tokio::test]
async fn test_agent_execute_plan_tool_not_found() {
    let mock_provider = MockProvider::new();
    let memory = Arc::new(MemoryStore::default());
    let agent = AgentManager::new(Arc::new(mock_provider), memory);
    
    let plan = crate::agent::planner::AgentPlan {
        thought: None,
        tasks: vec![
            crate::agent::planner::AgentTask {
                task_id: "2".into(),
                tool_type: "missing_tool".into(),
                description: "fail".into(),
                depends_on: vec![],
                source: None,
            }
        ],
    };

    let results = agent.execute_plan(plan, "Context", crate::models::scope::Scope::Private { user_id: "test".into() }, None).await;
    
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].task_id, "2");
    assert!(matches!(results[0].status, ToolStatus::Failed(_)));
}

#[tokio::test]
async fn test_agent_channel_reader() {
    let mock_provider = MockProvider::new();
    let memory = Arc::new(MemoryStore::default());
    let test_evt = crate::models::message::Event {
        platform: "test".into(),
        scope: crate::models::scope::Scope::Public { channel_id: "test_chan".into(), user_id: "system".into() },
        author_name: "test".into(),
        author_id: "test".into(),
        content: "test timeline string payload".into(),
    };
    let _ = memory.timeline.append_event(&test_evt).await;

    let agent = AgentManager::new(Arc::new(mock_provider), memory);
    
    let plan = crate::agent::planner::AgentPlan {
        thought: None,
        tasks: vec![
            crate::agent::planner::AgentTask {
                task_id: "1".into(),
                tool_type: "channel_reader".into(),
                description: "read test_chan".into(),
                depends_on: vec![],
                source: None,
            }
        ],
    };

    let results = agent.execute_plan(plan, "Context", crate::models::scope::Scope::Private { user_id: "test".into() }, None).await;
    assert_eq!(results.len(), 1);
    assert!(results[0].output.contains("test timeline"));
}

#[tokio::test]
async fn test_agent_codebase_list() {
    let mock_provider = MockProvider::new();
    let memory = Arc::new(MemoryStore::default());
    let agent = AgentManager::new(Arc::new(mock_provider), memory);
    
    let plan = crate::agent::planner::AgentPlan {
        thought: None,
        tasks: vec![
            crate::agent::planner::AgentTask {
                task_id: "1".into(),
                tool_type: "codebase_list".into(),
                description: "".into(),
                depends_on: vec![],
                source: None,
            }
        ],
    };

    let results = agent.execute_plan(plan, "Context", crate::models::scope::Scope::Private { user_id: "test".into() }, None).await;
    assert_eq!(results.len(), 1);
    assert!(results[0].output.contains("src/agent/mod.rs"));
}

#[tokio::test]
async fn test_agent_codebase_read() {
    let mock_provider = MockProvider::new();
    let memory = Arc::new(MemoryStore::default());
    let agent = AgentManager::new(Arc::new(mock_provider), memory);
    
    let plan = crate::agent::planner::AgentPlan {
        thought: None,
        tasks: vec![
            crate::agent::planner::AgentTask {
                task_id: "1".into(),
                tool_type: "codebase_read".into(),
                description: "Cargo.toml".into(),
                depends_on: vec![],
                source: None,
            }
        ],
    };

    let results = agent.execute_plan(plan, "Context", crate::models::scope::Scope::Private { user_id: "test".into() }, None).await;
    assert_eq!(results.len(), 1);
    assert!(results[0].output.contains("File: Cargo.toml"));
}

#[tokio::test]
async fn test_agent_codebase_read_security() {
    let mock_provider = MockProvider::new();
    let memory = Arc::new(MemoryStore::default());
    let agent = AgentManager::new(Arc::new(mock_provider), memory);
    
    let plan = crate::agent::planner::AgentPlan {
        thought: None,
        tasks: vec![
            crate::agent::planner::AgentTask {
                task_id: "1".into(),
                tool_type: "codebase_read".into(),
                description: "../Cargo.toml".into(),
                depends_on: vec![],
                source: None,
            }
        ],
    };

    let results = agent.execute_plan(plan, "Context", crate::models::scope::Scope::Private { user_id: "test".into() }, None).await;
    assert_eq!(results.len(), 1);
    assert!(results[0].output.contains("Access Denied"));
}

#[tokio::test]
async fn test_agent_web_search() {
    let mock_provider = MockProvider::new();
    let memory = Arc::new(MemoryStore::default());
    let agent = AgentManager::new(Arc::new(mock_provider), memory);
    
    let plan = crate::agent::planner::AgentPlan {
        thought: None,
        tasks: vec![
            crate::agent::planner::AgentTask {
                task_id: "1".into(),
                tool_type: "web_search".into(),
                description: "Rust programming language".into(),
                depends_on: vec![],
                source: None,
            }
        ],
    };

    let results = agent.execute_plan(plan, "Context", crate::models::scope::Scope::Private { user_id: "test".into() }, None).await;
    assert_eq!(results.len(), 1);
    assert!(
        results[0].output.contains("SEARCH RESULTS for") || 
        results[0].output.contains("GOOGLE NEWS RSS")
    );
}
