use std::collections::HashMap;
use std::sync::Arc;
use crate::models::drone::{DroneTemplate, DroneResult, DroneStatus};
use crate::providers::Provider;
use crate::memory::MemoryStore;
use crate::models::scope::Scope;

pub mod planner;
pub mod drone;

pub struct SwarmManager {
    registry: HashMap<String, DroneTemplate>,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
}

impl SwarmManager {
    pub fn new(provider: Arc<dyn Provider>, memory: Arc<MemoryStore>) -> Self {
        let mut registry = HashMap::new();
        
        // Register default built-in drones
        let researcher = DroneTemplate {
            name: "researcher".into(),
            system_prompt: "You are the Researcher Drone. Your job is to analyze information, find facts, and summarize data objectively. You HAVE LIVE INTERNET ACCESS and will search the web to verify current facts.".into(),
            tools: vec![],
        };

        let channel_reader = DroneTemplate {
            name: "native_channel_reader".into(),
            system_prompt: "You natively pull the recent message history of the current channel based on the task description Target ID. You do not use LLM inference, you return the timeline JSONL block. The planner should provide the Target Entity ID in the description.".into(),
            tools: vec![],
        };

        let codebase_list = DroneTemplate {
            name: "native_codebase_list".into(),
            system_prompt: "You list all files and directories recursively from the project root. You do not use LLM inference, you simply return the directory tree. The planner should output a blank description.".into(),
            tools: vec![],
        };

        let codebase_read = DroneTemplate {
            name: "native_codebase_read".into(),
            system_prompt: "You are the Codebase Reader Drone. You natively read the contents of a specific file in the HIVE codebase. The planner must put EXACTLY the relative file path (e.g. src/engine/mod.rs) in the description.".into(),
            tools: vec![],
        };

        let web_search = DroneTemplate {
            name: "native_web_search".into(),
            system_prompt: "You are the Web Search Drone. You search the LIVE EXTERNAL INTERNET for facts, news, and external documentation via DuckDuckGo. The planner should provide the query in the description.".into(),
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

    pub fn register_drone(&mut self, template: DroneTemplate) {
        self.registry.insert(template.name.clone(), template);
    }

    /// Exposes all registered drone names so they can be securely injected into 
    /// the AgentCapabilities matrix at engine boot.
    pub fn get_drone_names(&self) -> Vec<String> {
        self.registry.keys().cloned().collect()
    }

    pub fn get_template(&self, name: &str) -> Option<DroneTemplate> {
        self.registry.get(name).cloned()
    }

    /// Fetches all registered drones formatted as a string for the Queen Planner prompt
    pub fn get_available_drones_text(&self) -> String {
        let mut out = String::new();
        for (name, template) in &self.registry {
            out.push_str(&format!("- DRONE `{}`: {}\n", name, template.system_prompt));
        }
        out
    }

    /// Executes a swarm plan by spawning all tasks concurrently.
    /// In a fully robust graph, we would respect `depends_on`. For now, we fan out in parallel.
    #[cfg(not(tarpaulin_include))]
    pub async fn execute_plan(&self, plan: crate::swarm::planner::SwarmPlan, context: &str, telemetry_tx: Option<tokio::sync::mpsc::Sender<String>>) -> Vec<DroneResult> {
        let mut futures = vec![];

        for task in plan.tasks {
            // Intercept Native Drones
            if task.drone_type == "native_channel_reader" {
                let mem_clone = self.memory.clone();
                let task_id = task.task_id.clone();
                let desc = task.description.clone(); // E.g., tells which Scope or channel ID
                
                // For now, we assume the planner task description contains the target scope string
                // But generally the planner executes on the current Context Event anyway.
                // We'll parse the description or just default read the timeline logic.
                
                let tx_clone = telemetry_tx.clone();
                let handle = tokio::spawn(async move {
                    if let Some(ref tx) = tx_clone {
                        let _ = tx.send(format!("🧠 Native Channel Reader Drone executing...\n")).await;
                    }
                    // Extract channel_id if possible, or we could just pass `Event` down the SwarmManager tree.
                    // To keep it simple, we'll try to extract target from desc 
                    let target_id = desc.split_whitespace().last().unwrap_or(&"").to_string();
                    let pub_scope = Scope::Public { channel_id: target_id.clone(), user_id: "system".into() };

                    let output = if let Ok(timeline_data) = mem_clone.timeline.read_timeline(&pub_scope).await {
                        String::from_utf8_lossy(&timeline_data).to_string()
                    } else {
                        "Failed to read timeline for channel.".to_string()
                    };
                    
                    DroneResult {
                        task_id,
                        output,
                        tokens_used: 0,
                        status: DroneStatus::Success,
                    }
                });
                futures.push(handle);
                continue;
            } else if task.drone_type == "native_codebase_list" {
                let task_id = task.task_id.clone();
                let tx_clone = telemetry_tx.clone();
                let handle = tokio::spawn(async move {
                    if let Some(ref tx) = tx_clone {
                        let _ = tx.send(format!("🧠 Native Codebase List Drone executing...\n")).await;
                    }
                    // Quick recursive list, we'll shell out to `find` for simplicity or use standard local traversal.
                    // Returning a hardcoded string or running a quick command is easiest since we know linux/mac.
                    // For pure rust, we'll try std::process::Command
                    let output = match std::process::Command::new("find").arg("src").arg("-type").arg("f").output() {
                        Ok(res) => String::from_utf8_lossy(&res.stdout).to_string(),
                        Err(e) => format!("Failed to list codebase: {}", e),
                    };
                    DroneResult {
                        task_id,
                        output,
                        tokens_used: 0,
                        status: DroneStatus::Success,
                    }
                });
                futures.push(handle);
                continue;
            } else if task.drone_type == "native_codebase_read" {
                let task_id = task.task_id.clone();
                let desc = task.description.clone();
                let tx_clone = telemetry_tx.clone();
                let handle = tokio::spawn(async move {
                    if let Some(ref tx) = tx_clone {
                        let _ = tx.send(format!("🧠 Native Codebase Reader Drone reading: {}\n", desc)).await;
                    }
                    // Extract the path from the end of the description
                    let parts: Vec<&str> = desc.split_whitespace().collect();
                    let target_path = parts.last().unwrap_or(&"").to_string();
                    
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

                    DroneResult {
                        task_id,
                        output,
                        tokens_used: 0,
                        status: DroneStatus::Success,
                    }
                });
                futures.push(handle);
                continue;
            } else if task.drone_type == "native_web_search" || task.drone_type == "researcher" {
                let task_id = task.task_id.clone();
                let desc = task.description.clone();
                let tx_clone = telemetry_tx.clone();

                let handle = tokio::spawn(async move {
                    if let Some(ref tx) = tx_clone {
                        let _ = tx.send(format!("🌐 Native Web Search Drone searching for: {}\n", desc)).await;
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

                    DroneResult {
                        task_id,
                        output,
                        tokens_used: 0,
                        status: DroneStatus::Success,
                    }
                });
                futures.push(handle);
                continue;
            }

            if let Some(template) = self.get_template(&task.drone_type) {
                let context_clone = context.to_string();
                let provider_clone = self.provider.clone();
                let task_id = task.task_id.clone();
                let desc = task.description.clone();

                let tx_clone = telemetry_tx.clone();
                let template_name = template.name.clone();

                let handle = tokio::spawn(async move {
                    if let Some(ref tx) = tx_clone {
                        let _ = tx.send(format!("🚀 Spawning Drone `{}` for Task: {}\n", template_name, task_id)).await;
                    }
                    let executor = drone::DroneExecutor::new(provider_clone, template);
                    executor.execute(&task_id, &desc, &context_clone, tx_clone).await
                });

                futures.push(handle);
            } else {
                // Return immediate failure if drone doesn't exist
                futures.push(tokio::spawn(async move {
                    DroneResult {
                        task_id: task.task_id.clone(),
                        output: String::new(),
                        tokens_used: 0,
                        status: DroneStatus::Failed(format!("Drone type '{}' not found", task.drone_type)),
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
    use crate::models::drone::DroneStatus;

    #[tokio::test]
    async fn test_swarm_manager_registration() {
        let provider = Arc::new(MockProvider::new());
        let memory = Arc::new(MemoryStore::default());
        let mut swarm = SwarmManager::new(provider, memory);
        
        let template = DroneTemplate {
            name: "test_drone".into(),
            system_prompt: "sys".into(),
            tools: vec![],
        };
        
        swarm.register_drone(template.clone());
        assert!(swarm.get_template("test_drone").is_some());
    }

    #[tokio::test]
    async fn test_swarm_execute_plan_success() {
        let mut mock_provider = MockProvider::new();
        mock_provider
            .expect_generate()
            .returning(|_, _, _, _| Ok("Drone output".to_string()));

        let memory = Arc::new(MemoryStore::default());
        let swarm = SwarmManager::new(Arc::new(mock_provider), memory);
        
        let plan = crate::swarm::planner::SwarmPlan {
            tasks: vec![
                crate::swarm::planner::SwarmTask {
                    task_id: "1".into(),
                    drone_type: "researcher".into(),
                    description: "do research".into(),
                    depends_on: vec![],
                }
            ],
        };

        let results = swarm.execute_plan(plan, "User said hello", None).await;
        
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, "1");
        assert_eq!(results[0].output, "Drone output");
        assert_eq!(results[0].status, DroneStatus::Success);
    }

    #[tokio::test]
    async fn test_swarm_execute_plan_drone_not_found() {
        let mock_provider = MockProvider::new();
        let memory = Arc::new(MemoryStore::default());
        let swarm = SwarmManager::new(Arc::new(mock_provider), memory);
        
        let plan = crate::swarm::planner::SwarmPlan {
            tasks: vec![
                crate::swarm::planner::SwarmTask {
                    task_id: "2".into(),
                    drone_type: "missing_drone".into(),
                    description: "fail".into(),
                    depends_on: vec![],
                }
            ],
        };

        let results = swarm.execute_plan(plan, "Context", None).await;
        
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, "2");
        assert!(matches!(results[0].status, DroneStatus::Failed(_)));
    }
}
