#![allow(clippy::useless_format, clippy::needless_borrow, clippy::needless_borrows_for_generic_args)]
use std::collections::HashMap;
use std::sync::Arc;
use crate::models::tool::{ToolTemplate, ToolResult, ToolStatus};
use crate::providers::Provider;
use crate::memory::MemoryStore;
use crate::models::scope::Scope;

pub mod planner;
pub mod tool;

pub struct AgentManager {
    registry: HashMap<String, ToolTemplate>,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
}

impl AgentManager {
    pub fn new(provider: Arc<dyn Provider>, memory: Arc<MemoryStore>) -> Self {
        let mut registry = HashMap::new();
        
        // Register default built-in tools
        let researcher = ToolTemplate {
            name: "researcher".into(),
            system_prompt: "You are the Researcher Tool. Your job is to analyze information, find facts, and summarize data objectively. You HAVE LIVE INTERNET ACCESS and will search the web to verify current facts.".into(),
            tools: vec![],
        };

        let channel_reader = ToolTemplate {
            name: "native_channel_reader".into(),
            system_prompt: "You natively pull the recent message history of the current channel based on the task description Target ID. You do not use LLM inference, you return the timeline JSONL block. The planner should provide the Target Entity ID in the description.".into(),
            tools: vec![],
        };

        let codebase_list = ToolTemplate {
            name: "native_codebase_list".into(),
            system_prompt: "You list all files and directories recursively from the project root. You do not use LLM inference, you simply return the directory tree. The planner should output a blank description.".into(),
            tools: vec![],
        };

        let codebase_read = ToolTemplate {
            name: "native_codebase_read".into(),
            system_prompt: "You are the Codebase Reader Tool. You natively read the contents of a specific file in the HIVE codebase. The planner must put EXACTLY the relative file path (e.g. src/engine/mod.rs) in the description.".into(),
            tools: vec![],
        };

        let web_search = ToolTemplate {
            name: "native_web_search".into(),
            system_prompt: "You are the Web Search Tool. You search the LIVE EXTERNAL INTERNET for facts, news, and external documentation via DuckDuckGo. The planner should provide the query in the description.".into(),
            tools: vec![],
        };

        registry.insert(researcher.name.clone(), researcher);
        registry.insert(channel_reader.name.clone(), channel_reader);
        registry.insert(codebase_list.name.clone(), codebase_list);
        registry.insert(codebase_read.name.clone(), codebase_read);
        registry.insert(web_search.name.clone(), web_search);

        Self {
            registry,
            provider,
            memory,
        }
    }

    pub fn register_tool(&mut self, template: ToolTemplate) {
        self.registry.insert(template.name.clone(), template);
    }

    /// Exposes all registered tool names so they can be securely injected into 
    /// the AgentCapabilities matrix at engine boot.
    pub fn get_tool_names(&self) -> Vec<String> {
        self.registry.keys().cloned().collect()
    }

    pub fn get_template(&self, name: &str) -> Option<ToolTemplate> {
        self.registry.get(name).cloned()
    }

    /// Fetches all registered tools formatted as a string for the Planner Planner prompt
    pub fn get_available_tools_text(&self) -> String {
        let mut out = String::new();
        for (name, template) in &self.registry {
            out.push_str(&format!("- TOOL `{}`: {}\n", name, template.system_prompt));
        }
        out
    }

    /// Executes a agent plan by spawning all tasks concurrently.
    /// In a fully robust graph, we would respect `depends_on`. For now, we fan out in parallel.
    #[cfg(not(tarpaulin_include))]
    pub async fn execute_plan(&self, plan: crate::agent::planner::AgentPlan, context: &str, telemetry_tx: Option<tokio::sync::mpsc::Sender<String>>) -> Vec<ToolResult> {
        let mut futures = vec![];

        for task in plan.tasks {
            // Intercept Native Tools
            if task.tool_type == "native_channel_reader" {
                let mem_clone = self.memory.clone();
                let task_id = task.task_id.clone();
                let desc = task.description.clone(); // E.g., tells which Scope or channel ID
                
                // For now, we assume the planner task description contains the target scope string
                // But generally the planner executes on the current Context Event anyway.
                // We'll parse the description or just default read the timeline logic.
                
                let tx_clone = telemetry_tx.clone();
                let handle = tokio::spawn(async move {
                    if let Some(ref tx) = tx_clone {
                        let _ = tx.send(format!("🧠 Native Channel Reader Tool executing...\n")).await;
                    }
                    // Extract channel_id if possible, or we could just pass `Event` down the AgentManager tree.
                    // To keep it simple, we'll try to extract target from desc 
                    let target_id = desc.split_whitespace().last().unwrap_or(&"").to_string();
                    let pub_scope = Scope::Public { channel_id: target_id.clone(), user_id: "system".into() };

                    let output = if let Ok(timeline_data) = mem_clone.timeline.read_timeline(&pub_scope).await {
                        String::from_utf8_lossy(&timeline_data).to_string()
                    } else {
                        "Failed to read timeline for channel.".to_string()
                    };
                    
                    ToolResult {
                        task_id,
                        output,
                        tokens_used: 0,
                        status: ToolStatus::Success,
                    }
                });
                futures.push(handle);
                continue;
            } else if task.tool_type == "native_codebase_list" {
                let task_id = task.task_id.clone();
                let tx_clone = telemetry_tx.clone();
                let handle = tokio::spawn(async move {
                    if let Some(ref tx) = tx_clone {
                        let _ = tx.send(format!("🧠 Native Codebase List Tool executing...\n")).await;
                    }
                    // Quick recursive list, we'll shell out to `find` for simplicity or use standard local traversal.
                    // Returning a hardcoded string or running a quick command is easiest since we know linux/mac.
                    // For pure rust, we'll try std::process::Command
                    let output = match std::process::Command::new("find").arg("src").arg("-type").arg("f").output() {
                        Ok(res) => String::from_utf8_lossy(&res.stdout).to_string(),
                        Err(e) => format!("Failed to list codebase: {}", e),
                    };
                    ToolResult {
                        task_id,
                        output,
                        tokens_used: 0,
                        status: ToolStatus::Success,
                    }
                });
                futures.push(handle);
                continue;
            } else if task.tool_type == "native_codebase_read" {
                let task_id = task.task_id.clone();
                let desc = task.description.clone();
                let tx_clone = telemetry_tx.clone();
                let handle = tokio::spawn(async move {
                    if let Some(ref tx) = tx_clone {
                        let _ = tx.send(format!("🧠 Native Codebase Reader Tool reading: {}\n", desc)).await;
                    }
                    // Extract the path by looking for something that looks like a file path
                    // Apis often writes: "Read the main engine module file (src/engine/mod.rs) to verify..."
                    let target_path = desc
                        .split_whitespace()
                        .find(|s| s.contains("src/") || s.contains('/') || s.ends_with(".rs") || s.ends_with(".py") || s.ends_with(".toml"))
                        .map(|s| s.trim_matches(|c| c == '(' || c == ')' || c == '\'' || c == '"' || c == '`'))
                        .unwrap_or_else(|| desc.split_whitespace().last().unwrap_or(""))
                        .to_string();
                    
                    // Basic sanity check to prevent arbitrary file reading outside cwd
                    let output = if target_path.contains("..") || target_path.starts_with('/') {
                        "Access Denied: Path traverses outside isolated project root.".to_string()
                    } else if let Ok(content) = tokio::fs::read_to_string(&target_path).await {
                        format!("--- FILE: {} ---\n{}", target_path, content)
                    } else {
                        // Fuzzy fallback: extract filename and search src/ for it
                        let filename = std::path::Path::new(&target_path)
                            .file_name()
                            .and_then(|f| f.to_str())
                            .unwrap_or(&target_path);
                        
                        let find_result = std::process::Command::new("find")
                            .args(&["src", "-name", filename, "-type", "f"])
                            .output();
                        
                        match find_result {
                            Ok(res) => {
                                let found = String::from_utf8_lossy(&res.stdout);
                                let found_path = found.trim().lines().next().unwrap_or("");
                                if !found_path.is_empty() {
                                    if let Ok(content) = tokio::fs::read_to_string(found_path).await {
                                        format!("--- FILE: {} (resolved from '{}') ---\n{}", found_path, target_path, content)
                                    } else {
                                        format!("Failed to read file: {} (found at {} but read failed)", target_path, found_path)
                                    }
                                } else {
                                    format!("Failed to read file: {} (not found, also searched src/ for '{}')", target_path, filename)
                                }
                            }
                            Err(_) => format!("Failed to read file: {}", target_path),
                        }
                    };

                    ToolResult {
                        task_id,
                        output,
                        tokens_used: 0,
                        status: ToolStatus::Success,
                    }
                });
                futures.push(handle);
                continue;
            } else if task.tool_type == "native_web_search" || task.tool_type == "researcher" {
                let task_id = task.task_id.clone();
                let desc = task.description.clone();
                let tx_clone = telemetry_tx.clone();

                let handle = tokio::spawn(async move {
                    if let Some(ref tx) = tx_clone {
                        let _ = tx.send(format!("🌐 Native Web Search Tool searching for: {}\n", desc)).await;
                    }
                    
                    // Simple internet fetch using curl to DuckDuckGo HTML Lite
                    let query = desc.replace(" ", "+");
                    let output = match std::process::Command::new("curl")
                        .args(&["-s", "-A", "Mozilla/5.0", &format!("https://html.duckduckgo.com/html/?q={}", query)])
                        .output() 
                    {
                        Ok(res) => {
                            let html = String::from_utf8_lossy(&res.stdout);
                            if html.is_empty() {
                                "Failed to retrieve search results (empty response).".to_string()
                            } else {
                                // Extremely naive HTML stripping to extract snippets
                                let mut text = String::new();
                                let mut in_tag = false;
                                for c in html.chars() {
                                    if c == '<' { in_tag = true; }
                                    else if c == '>' { in_tag = false; text.push(' '); }
                                    else if !in_tag { text.push(c); }
                                }
                                
                                // Clean up excessive whitespace
                                let cleaned: Vec<&str> = text.split_whitespace().collect();
                                let final_text = cleaned.join(" ");
                                
                                format!("--- SEARCH RESULTS for '{}' ---\n{}", desc, final_text)
                            }
                        }
                        Err(e) => format!("Failed to execute search: {}", e),
                    };

                    ToolResult {
                        task_id,
                        output,
                        tokens_used: 0,
                        status: ToolStatus::Success,
                    }
                });
                futures.push(handle);
                continue;
            }

            if let Some(template) = self.get_template(&task.tool_type) {
                let context_clone = context.to_string();
                let provider_clone = self.provider.clone();
                let task_id = task.task_id.clone();
                let desc = task.description.clone();

                let tx_clone = telemetry_tx.clone();
                let template_name = template.name.clone();

                let handle = tokio::spawn(async move {
                    if let Some(ref tx) = tx_clone {
                        let _ = tx.send(format!("🚀 Spawning Tool `{}` for Task: {}\n", template_name, task_id)).await;
                    }
                    let executor = tool::ToolExecutor::new(provider_clone, template);
                    executor.execute(&task_id, &desc, &context_clone, tx_clone).await
                });

                futures.push(handle);
            } else {
                // Return immediate failure if tool doesn't exist
                futures.push(tokio::spawn(async move {
                    ToolResult {
                        task_id: task.task_id.clone(),
                        output: String::new(),
                        tokens_used: 0,
                        status: ToolStatus::Failed(format!("Tool type '{}' not found", task.tool_type)),
                    }
                }));
            }
        }

        let mut results = vec![];
        for f in futures {
            if let Ok(res) = f.await {
                results.push(res);
            }
        }
        results
    }
}

#[cfg(test)]
mod tests {
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
                }
            ],
        };

        let results = agent.execute_plan(plan, "User said hello", None).await;
        
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, "1");
        assert_eq!(results[0].output, "Tool output");
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
                }
            ],
        };

        let results = agent.execute_plan(plan, "Context", None).await;
        
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, "2");
        assert!(matches!(results[0].status, ToolStatus::Failed(_)));
    }

    #[tokio::test]
    async fn test_agent_native_channel_reader() {
        let mock_provider = MockProvider::new();
        let memory = Arc::new(MemoryStore::default());
        // Populate timeline so read has something
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
                    tool_type: "native_channel_reader".into(),
                    description: "read test_chan".into(),
                    depends_on: vec![],
                }
            ],
        };

        let results = agent.execute_plan(plan, "Context", None).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].output.contains("test timeline"));
    }

    #[tokio::test]
    async fn test_agent_native_codebase_list() {
        let mock_provider = MockProvider::new();
        let memory = Arc::new(MemoryStore::default());
        let agent = AgentManager::new(Arc::new(mock_provider), memory);
        
        let plan = crate::agent::planner::AgentPlan {
            thought: None,
            tasks: vec![
                crate::agent::planner::AgentTask {
                    task_id: "1".into(),
                    tool_type: "native_codebase_list".into(),
                    description: "".into(),
                    depends_on: vec![],
                }
            ],
        };

        let results = agent.execute_plan(plan, "Context", None).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].output.contains("src/agent/mod.rs"));
    }

    #[tokio::test]
    async fn test_agent_native_codebase_read() {
        let mock_provider = MockProvider::new();
        let memory = Arc::new(MemoryStore::default());
        let agent = AgentManager::new(Arc::new(mock_provider), memory);
        
        let plan = crate::agent::planner::AgentPlan {
            thought: None,
            tasks: vec![
                crate::agent::planner::AgentTask {
                    task_id: "1".into(),
                    tool_type: "native_codebase_read".into(),
                    description: "Cargo.toml".into(), // guaranteed to exist
                    depends_on: vec![],
                }
            ],
        };

        let results = agent.execute_plan(plan, "Context", None).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].output.contains("--- FILE: Cargo.toml"));
    }

    #[tokio::test]
    async fn test_agent_native_codebase_read_security() {
        let mock_provider = MockProvider::new();
        let memory = Arc::new(MemoryStore::default());
        let agent = AgentManager::new(Arc::new(mock_provider), memory);
        
        let plan = crate::agent::planner::AgentPlan {
            thought: None,
            tasks: vec![
                crate::agent::planner::AgentTask {
                    task_id: "1".into(),
                    tool_type: "native_codebase_read".into(),
                    description: "../Cargo.toml".into(), // traverse attempts
                    depends_on: vec![],
                }
            ],
        };

        let results = agent.execute_plan(plan, "Context", None).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].output.contains("Access Denied"));
    }

    #[tokio::test]
    async fn test_agent_native_web_search() {
        let mock_provider = MockProvider::new();
        let memory = Arc::new(MemoryStore::default());
        let agent = AgentManager::new(Arc::new(mock_provider), memory);
        
        let plan = crate::agent::planner::AgentPlan {
            thought: None,
            tasks: vec![
                crate::agent::planner::AgentTask {
                    task_id: "1".into(),
                    tool_type: "native_web_search".into(),
                    description: "Rust programming language".into(),
                    depends_on: vec![],
                }
            ],
        };

        let results = agent.execute_plan(plan, "Context", None).await;
        assert_eq!(results.len(), 1);
        assert!(results[0].output.contains("SEARCH RESULTS for"));
    }
}
