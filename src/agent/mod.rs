#![allow(clippy::useless_format, clippy::needless_borrow, clippy::needless_borrows_for_generic_args)]
use std::collections::HashMap;
use std::sync::Arc;
use crate::models::tool::{ToolTemplate, ToolResult, ToolStatus};
use crate::providers::Provider;
use crate::memory::MemoryStore;


pub mod planner;
pub mod tool;
pub mod preferences;
pub mod synthesis;
pub mod containment;

pub mod outreach;
pub mod skills;
pub mod routines;
pub mod process_manager;
pub mod lessons_tool;
pub mod turing_tool;
pub mod web_tool;
pub mod image_tool;
pub mod tts_tool;
pub mod file_reader;
pub mod file_writer;
pub mod registry;
pub mod timeline_tool;
pub mod scratchpad_tool;
pub mod synaptic_tool;
pub mod core_memory_tool;
pub mod file_system_tool;
pub mod autonomy_tool;
pub mod reasoning_tool;
pub mod attachment_tool;
pub mod log_tool;
pub mod download_tool;
pub mod moderation_tool;
pub mod sub_agent;
pub mod spawner;
pub mod aggregator;
pub mod lifecycle;
pub mod goal_planner;
pub mod goal_tool;
pub mod tool_forge;
pub mod visualizer_tool;
pub mod email_tool;
pub mod calendar_tool;
pub mod smarthome_tool;
pub mod compiler_tool;
pub mod contributors_tool;
pub mod distillation_tool;
pub mod contacts_tool;
pub mod opencode;
pub mod wallet_tool;
pub mod credits_tool;
pub mod nft_tool;
pub mod deep_think_tool;
pub mod model_tool;
pub mod git_tool;
pub mod lsp_client;
pub mod lsp_tool;
pub mod ratchet_tool;

pub mod tool_registry;

pub struct AgentManager {
    registry: HashMap<String, ToolTemplate>,
    discord_tools: HashMap<String, ToolTemplate>,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
    pub config: Arc<crate::config::AppConfig>,
    pub composer: Arc<crate::computer::document::DocumentComposer>,
    pub drives: Option<Arc<crate::engine::drives::DriveSystem>>,
    pub outreach_gate: Option<Arc<crate::engine::outreach::OutreachGate>>,
    pub inbox: Option<Arc<crate::engine::inbox::InboxManager>>,
    pub goal_store: Option<Arc<crate::engine::goals::GoalStore>>,
    pub tool_forge: Option<Arc<crate::agent::tool_forge::ToolForge>>,
    pub opencode_bridge: Option<Arc<crate::agent::opencode::OpenCodeBridge>>,
}

impl AgentManager {
    pub fn new(provider: Arc<dyn Provider>, memory: Arc<MemoryStore>, config: Arc<crate::config::AppConfig>) -> Self {
        let (registry, discord_tools) = tool_registry::build_default_registries();

        Self {
            registry,
            discord_tools,
            provider,
            memory,
            config,
            composer: Arc::new(crate::computer::document::DocumentComposer::new()),
            drives: None,
            outreach_gate: None,
            inbox: None,
            goal_store: None,
            tool_forge: None,
            opencode_bridge: None,
        }
    }

    /// Inject the outreach subsystem after construction.
    pub fn with_outreach(
        mut self,
        drives: Arc<crate::engine::drives::DriveSystem>,
        outreach_gate: Arc<crate::engine::outreach::OutreachGate>,
        inbox: Arc<crate::engine::inbox::InboxManager>,
    ) -> Self {
        self.drives = Some(drives);
        self.outreach_gate = Some(outreach_gate);
        self.inbox = Some(inbox);
        self
    }

    pub fn with_goals(mut self, goal_store: Arc<crate::engine::goals::GoalStore>) -> Self {
        self.goal_store = Some(goal_store);
        self
    }

    pub fn with_forge(mut self, forge: Arc<crate::agent::tool_forge::ToolForge>) -> Self {
        self.tool_forge = Some(forge);
        self
    }

    pub fn with_opencode(mut self, bridge: Arc<crate::agent::opencode::OpenCodeBridge>) -> Self {
        self.opencode_bridge = Some(bridge);
        self
    }

    /// Hot-load forged tools into the registry so the planner sees them.
    pub fn load_forged_tools(&mut self, forge: &crate::agent::tool_forge::ToolForge) {
        // Synchronous load from the forge's data (already loaded from disk)
        let tools_dir = &forge.tools_dir;
        let registry_path = tools_dir.join("registry.json");
        if registry_path.exists()
            && let Ok(raw) = std::fs::read_to_string(&registry_path)
                && let Ok(data) = serde_json::from_str::<serde_json::Value>(&raw)
                    && let Some(tools) = data.get("tools").and_then(|t| t.as_array()) {
                        for tool_val in tools {
                            let enabled = tool_val.get("enabled").and_then(|e| e.as_bool()).unwrap_or(false);
                            if !enabled { continue; }
                            let name = tool_val.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let desc = tool_val.get("description").and_then(|d| d.as_str()).unwrap_or("");
                            if name.is_empty() { continue; }
                            let template = ToolTemplate {
                                name: name.to_string(),
                                system_prompt: format!("[FORGED TOOL] {}", desc),
                                tools: vec![],
                            };
                            self.registry.insert(name.to_string(), template);
                            tracing::info!("[FORGE] Hot-loaded forged tool: {}", name);
                        }
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

    /// Fetches all registered tools formatted as a string for the Planner prompt.
    /// If `is_autonomy` is true, dangerous and self-moderation tools are excluded
    /// to prevent unsupervised execution of system-altering operations.
    pub fn get_available_tools_text(&self, is_autonomy: bool) -> String {
        let mut out = String::new();

        // Tools excluded during autonomy:
        // 1. Moderation tools — prevent loop-testing on itself
        // 2. System modification tools — require human supervision
        // 3. External tooling — too compute-intensive or dangerous unsupervised
        let autonomy_excluded = [
            // Moderation
            "refuse_request", "disengage", "mute_user", "set_boundary",
            "block_topic", "escalate_to_admin", "report_concern",
            "rate_limit_user", "request_consent", "wellbeing_status",
            // System modification (require human supervision)
            "opencode", "system_recompile",
            // Dangerous unsupervised
            "run_bash_command",
        ];

        for (name, template) in &self.registry {
            if is_autonomy && autonomy_excluded.contains(&name.as_str()) {
                continue;
            }
            out.push_str(&format!("- TOOL `{}`: {}\n", name, template.system_prompt));
        }
        out
    }

    /// Fetches tools formatted for a specific platform (universal + platform-specific)
    pub fn get_available_tools_text_for_platform(&self, platform: &str, is_autonomy: bool) -> String {
        let mut out = self.get_available_tools_text(is_autonomy);
        let platform_prefix = platform.split(':').next().unwrap_or("");
        let platform_tools = match platform_prefix {
            "discord" => Some(&self.discord_tools),
            _ => None,
        };
        if let Some(tools) = platform_tools {
            out.push_str("\n## Platform-Specific Tools (Discord)\n");
            for (name, template) in tools {
                out.push_str(&format!("- TOOL `{}`: {}\n", name, template.system_prompt));
            }
        }
        out
    }

    /// Executes a plan respecting `depends_on` via wave-based scheduling.
    /// Wave 0: tasks with no dependencies (fan out in parallel).
    /// Wave N: tasks whose deps are all in the completed set (fan out in parallel).
    /// Prevents races where dependent tasks (goal decompose, forge test) run before
    /// the task that creates the entity they reference.
    #[cfg(not(tarpaulin_include))]
    pub async fn execute_plan(
        &self,
        plan: crate::agent::planner::AgentPlan,
        context: &str,
        scope: crate::models::scope::Scope,
        telemetry_tx: Option<tokio::sync::mpsc::Sender<String>>,
        swarm_agent: Option<Arc<AgentManager>>,
        swarm_caps: Option<Arc<crate::models::capabilities::AgentCapabilities>>,
        outbound_tx: Option<tokio::sync::mpsc::Sender<crate::models::message::Response>>,
    ) -> Vec<ToolResult> {
        use std::collections::HashSet;

        let mut all_results: Vec<ToolResult> = vec![];
        let mut completed_ids: HashSet<String> = HashSet::new();
        let mut remaining_tasks = plan.tasks;

        // Safety: cap at 20 waves to prevent infinite loops from circular deps
        for wave in 0..20 {
            if remaining_tasks.is_empty() {
                break;
            }

            // Partition: ready tasks (all deps satisfied) vs blocked tasks
            let (ready, blocked): (Vec<_>, Vec<_>) = remaining_tasks.into_iter().partition(|t| {
                t.depends_on.is_empty() || t.depends_on.iter().all(|dep| completed_ids.contains(dep))
            });

            if ready.is_empty() {
                // All remaining tasks have unsatisfiable deps — force-run them to avoid deadlock
                tracing::warn!(
                    "[AGENT:execute_plan] Wave {} deadlock: {} tasks have unsatisfiable depends_on, force-dispatching",
                    wave, blocked.len()
                );
                remaining_tasks = blocked;
                let forced: Vec<_> = remaining_tasks.drain(..).collect();
                let mut futures = vec![];
                for task in forced {
                    futures.push(self.dispatch_single_task(
                        task, context, &scope, telemetry_tx.clone(),
                        swarm_agent.clone(), swarm_caps.clone(), outbound_tx.clone(),
                    ));
                }
                for f in futures {
                    if let Ok(res) = f.await {
                        completed_ids.insert(res.task_id.clone());
                        all_results.push(res);
                    }
                }
                break;
            }

            tracing::debug!(
                "[AGENT:execute_plan] Wave {}: dispatching {} ready tasks, {} blocked",
                wave, ready.len(), blocked.len()
            );

            // Dispatch all ready tasks in parallel
            let mut futures = vec![];
            for task in ready {
                futures.push(self.dispatch_single_task(
                    task, context, &scope, telemetry_tx.clone(),
                    swarm_agent.clone(), swarm_caps.clone(), outbound_tx.clone(),
                ));
            }

            // Collect results
            for f in futures {
                if let Ok(res) = f.await {
                    completed_ids.insert(res.task_id.clone());
                    all_results.push(res);
                }
            }

            remaining_tasks = blocked;
        }

        all_results
    }

    /// Dispatch a single task to either native handler or LLM-backed template.
    #[cfg(not(tarpaulin_include))]
    fn dispatch_single_task(
        &self,
        task: crate::agent::planner::AgentTask,
        context: &str,
        scope: &crate::models::scope::Scope,
        telemetry_tx: Option<tokio::sync::mpsc::Sender<String>>,
        swarm_agent: Option<Arc<AgentManager>>,
        swarm_caps: Option<Arc<crate::models::capabilities::AgentCapabilities>>,
        outbound_tx: Option<tokio::sync::mpsc::Sender<crate::models::message::Response>>,
    ) -> tokio::task::JoinHandle<ToolResult> {
        if let Some(handle) = crate::agent::registry::execution::dispatch_native_tool(
            &task,
            context,
            scope,
            telemetry_tx.clone(),
            self.memory.clone(),
            self.provider.clone(),
            self.outreach_gate.clone(),
            self.inbox.clone(),
            self.drives.clone(),
            Some(self.composer.clone()),
            swarm_agent.clone(),
            swarm_caps.clone(),
            self.goal_store.clone(),
            self.tool_forge.clone(),
            self.opencode_bridge.clone(),
            outbound_tx.clone(),
        ) {
            return handle;
        }

        if let Some(template) = self.get_template(&task.tool_type) {
            let context_clone = context.to_string();
            let provider_clone = self.provider.clone();
            let task_id = task.task_id.clone();
            let desc = task.description.clone();
            let tx_clone = telemetry_tx.clone();
            let template_name = template.name.clone();

            return tokio::spawn(async move {
                if let Some(ref tx) = tx_clone {
                    let _ = tx.send(format!("🚀 Spawning Tool `{}` for Task: {}\n", template_name, task_id)).await;
                }
                let executor = tool::ToolExecutor::new(provider_clone, template);
                executor.execute(&task_id, &desc, &context_clone, tx_clone).await
            });
        }

        // Return immediate failure if tool doesn't exist
        let task_id = task.task_id.clone();
        let tool_type = task.tool_type.clone();
        tokio::spawn(async move {
            ToolResult {
                task_id,
                output: String::new(),
                tokens_used: 0,
                status: ToolStatus::Failed(format!("Tool type '{}' not found", tool_type)),
            }
        })
    }
}

#[cfg(test)]
mod tests;
