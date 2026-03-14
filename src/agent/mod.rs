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
pub mod synthesis_drone;
pub mod outreach;
pub mod skills;
pub mod routines;
pub mod process_manager;
pub mod lessons_drone;
pub mod turing_drone;
pub mod web_drone;
pub mod image_drone;
pub mod tts_drone;
pub mod file_reader;
pub mod file_writer;
pub mod registry;
pub mod timeline_drone;
pub mod scratchpad_drone;
pub mod synaptic_drone;
pub mod core_memory_drone;

pub struct AgentManager {
    registry: HashMap<String, ToolTemplate>,
    discord_tools: HashMap<String, ToolTemplate>,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
    pub drives: Option<Arc<crate::engine::drives::DriveSystem>>,
    pub outreach_gate: Option<Arc<crate::engine::outreach::OutreachGate>>,
    pub inbox: Option<Arc<crate::engine::inbox::InboxManager>>,
}

impl AgentManager {
    pub fn new(provider: Arc<dyn Provider>, memory: Arc<MemoryStore>) -> Self {
        let mut registry = HashMap::new();
        let mut discord_tools = HashMap::new();
        
        // Register default built-in tools (universal — available on all platforms)
        let researcher = ToolTemplate {
            name: "researcher".into(),
            system_prompt: "You are the Researcher Tool. Your job is to analyze information, find facts, and summarize data objectively. You HAVE LIVE INTERNET ACCESS and will search the web to verify current facts.".into(),
            tools: vec![],
        };

        // Discord-only tools
        let channel_reader = ToolTemplate {
            name: "channel_reader".into(),
            system_prompt: "You natively pull the recent message history of the current channel based on the task description Target ID. You do not use LLM inference, you return the timeline JSONL block. The planner should provide the Target Entity ID in the description.".into(),
            tools: vec![],
        };
        let emoji_react = ToolTemplate {
            name: "emoji_react".into(),
            system_prompt: "React to the user's Discord message with a native emoji reaction. Use this PROACTIVELY and GENUINELY to show context-aware emotion (e.g. laughing at a joke, celebrating a win) alongside your actions. This attaches the emoji directly to the message. Description format: 'emoji:[unicode emoji character]' e.g. 'emoji:[👍]' or 'emoji:[💀]'".into(),
            tools: vec![],
        };

        let codebase_list = ToolTemplate {
            name: "codebase_list".into(),
            system_prompt: "You list all files and directories recursively from the project root. You do not use LLM inference, you simply return the directory tree. The planner should output a blank description.".into(),
            tools: vec![],
        };

        let codebase_read = ToolTemplate {
            name: "codebase_read".into(),
            system_prompt: "You are the Codebase Reader Tool. You natively read the contents of a specific file in the HIVE codebase. The planner must put EXACTLY the relative file path (e.g. src/engine/mod.rs). Description format: 'name:[src/path] start_line:[1] limit:[500]'".into(),
            tools: vec![],
        };

        let web_search = ToolTemplate {
            name: "web_search".into(),
            system_prompt: "You are the Web Search Tool. You search the LIVE EXTERNAL INTERNET for facts, news, and external documentation via DuckDuckGo. The planner should provide the query in the description.".into(),
            tools: vec![],
        };

        let manage_user_prefs = ToolTemplate {
            name: "manage_user_preferences".into(),
            system_prompt: "You are the User Preference Tool. You manage long-term psychological profiling and factual preferences of the user. Updates MUST include an 'action:' tag and a 'value:' tag. Valid actions are: update_name, add_hobby, add_topic, update_narrative, update_psychoanalysis. Example description: 'action:[add_hobby] value:[Archery]'".into(),
            tools: vec![],
        };

        let outreach = ToolTemplate {
            name: "outreach".into(),
            system_prompt: "Proactively reach out to a Discord user or manage outreach settings. \
                'action:[send] user_id:[discord_uid] content:[text]' — send a proactive message (DM or public, per prefs). \
                'action:[set_frequency] user_id:[uid] frequency:[low|medium|high|unlimited]' — set contact frequency. \
                'action:[set_delivery] user_id:[uid] delivery:[dm|public|both|none]' — set delivery channel. \
                'action:[status] user_id:[uid]' — show outreach settings. \
                'action:[inbox_status] user_id:[uid]' — show unread inbox summary.".into(),
            tools: vec![],
        };

        let manage_lessons = ToolTemplate { name: "manage_lessons".into(), system_prompt: "Manage important behavioral or factual lessons. 'action:[store] lesson:[text] keywords:[comma separated] confidence:[0.0-1.0]' — write a lesson. 'action:[read]' — list all lessons scoped here. 'action:[search] query:[text]' — filter lessons.".into(), tools: vec![] };
        let search_timeline = ToolTemplate { name: "search_timeline".into(), system_prompt: "Deep search the infinite long-term episodic memory logs for this channel/user. 'action:[search] query:[text] limit:[N]' — search backwards through time.".into(), tools: vec![] };
        let manage_scratchpad = ToolTemplate { name: "manage_scratchpad".into(), system_prompt: "Persistent VRAM for notes/variables scoped to this chat. 'action:[read]' — view the scratchpad. 'action:[write] content:[...]' — overwrite entirely. 'action:[append] content:[...]' — add to end. 'action:[clear]' — wipe.".into(), tools: vec![] };
        let operate_synaptic_graph = ToolTemplate { name: "operate_synaptic_graph".into(), system_prompt: "Neo4j Knowledge Graph for core truths and relationships. 'action:[store] concept:[A] data:[B]' — store belief. 'action:[search] concept:[A]' — retrieve mapping. 'action:[beliefs]' — dump core system beliefs.".into(), tools: vec![] };
        let read_core_memory = ToolTemplate { name: "read_core_memory".into(), system_prompt: "System introspection. 'action:[temporal]' — check boot time, total uptime, turn counts. 'action:[tokens]' — check working memory context size / token pressure limit.".into(), tools: vec![] };
        let manage_skill = ToolTemplate { name: "manage_skill".into(), system_prompt: "[ADMIN ONLY] Create, list, or execute custom Python or Bash scripts. Stored and scoped to the current user/channel. Description format: 'action:[create/list/execute] name:[skill_name.py] content:[RAW CODE]'".into(), tools: vec![] };
        let manage_routine = ToolTemplate { name: "manage_routine".into(), system_prompt: "Create, read, or list OpenClaw-style declarative markdown Routines. Routines instruct you on how to solve complex tasks. Description format: 'action:[create/read/list] name:[routine.md] content:[RAW MARKDOWN]'".into(), tools: vec![] };
        let synthesizer = ToolTemplate { name: "synthesizer".into(), system_prompt: "Fan-in aggregator drone. Reads all DRONE OUTPUT blocks already in context and condenses them into a single compact synthesis. Use this as the final task in a multi-wave plan when you need to merge results before replying. Description: plain English instruction on what to synthesise, e.g. 'Summarise the web and memory results into 3 key findings.'. CRITICAL INSTRUCTION: If any drone output includes a tag like [ATTACH_IMAGE](...), [ATTACH_FILE](...), or [ATTACH_AUDIO](...), you MUST include that EXACT tag verbatim in your output so it reaches the user.".into(), tools: vec![] };
        let read_logs = ToolTemplate { name: "read_logs".into(), system_prompt: "Reads deep spans of the core system log (logs/hive.log) for debug introspection. Description format: 'action:[read] lines:[number of lines to read starting from the tail]'".into(), tools: vec![] };
        let review_reasoning = ToolTemplate { name: "review_reasoning".into(), system_prompt: "Read historical ReAct reasoning traces from working memory. Use this to remember why you made specific decisions turns ago. Description format: 'action:[read] turns_ago:[number of turns ago to start reading]'".into(), tools: vec![] };
        let operate_turing_grid = ToolTemplate {
            name: "operate_turing_grid".into(),
            system_prompt: "The 3D Turing Grid is a massive arbitrary personal computation device. \
                'action:[read]' - read current cell. \
                'action:[write] format:[text|json|rust|python|node|ruby|swift|applescript] content:[data]' - over/write cell. \
                'action:[move] dx:[X] dy:[Y] dz:[Z]' - safely move the R/W head relative to current. \
                'action:[scan] radius:[R]' - radar search surrounding cells for data. \
                'action:[execute]' - route the current cell to the internal ALU kernel.".into(),
            tools: vec![],
        };
        let generate_image = ToolTemplate {
            name: "generate_image".into(),
            system_prompt: "The Flux Image Generator. Describe the image you want generated in highly detailed stable-diffusion style prompt format. Limit ONE image per request cycle. Description format: 'prompt:[detailed prompt]'".into(),
            tools: vec![],
        };
        let voice_synthesizer = ToolTemplate {
            name: "voice_synthesizer".into(),
            system_prompt: "The native Kokoro Text-To-Speech engine. Use this when you want to speak aloud to the user or attach a voice snippet. Format: 'text:[...text...]'".into(),
            tools: vec![],
        };
        let file_writer = ToolTemplate {
            name: "file_writer".into(),
            system_prompt: "You can create richly formatted PDF documents and save them to your document library. \
            For a single-shot document, use 'action:[compose] id:[doc1] title:[...] theme:[professional/cyberpunk/minimal] content:[Markdown text...]'. \
            For multi-turn piece-by-piece drafting, first use 'action:[start] id:[doc1] title:[...] theme:[professional]'. \
            Then in subsequent turns, use 'action:[add_section] id:[doc1] heading:[...] content:[...]'. \
            Finally, use 'action:[render] id:[doc1]'.".into(),
            tools: vec![],
        };
        let read_attachment = ToolTemplate {
            name: "read_attachment".into(),
            system_prompt: "Fetch and read a user-uploaded file attachment in-memory. NOTHING is saved to disk. Supports text, code, JSON, CSV, and markdown. DO NOT USE THIS FOR IMAGES (you have Native Vision and can see images directly). Use this when you see a [USER_ATTACHMENT] tag in the user's message. Description format: 'url:[the CDN URL from the USER_ATTACHMENT tag]'. Hard limit: 10MB max file size.".into(),
            tools: vec![],
        };
        let autonomy_activity = ToolTemplate {
            name: "autonomy_activity".into(),
            system_prompt: "Read your autonomous activity history. Use this to answer questions like 'what have you been up to?'. \
                'action:[summary]' — compact 24hr digest of all autonomous sessions. \
                'action:[read] count:[N]' — read the last N detailed activity entries (default 10).".into(),
            tools: vec![],
        };

        let run_bash_command = ToolTemplate {
            name: "run_bash_command".into(),
            system_prompt: "[ADMIN ONLY] Execute an arbitrary bash command on the host. The planner should put the exact bash string to execute in the description block.".into(),
            tools: vec![],
        };
        let process_manager = ToolTemplate {
            name: "process_manager".into(),
            system_prompt: "[ADMIN ONLY] You manage background daemons and execute host bash commands. \
            'action:[execute] command:[...]' runs normally with a 30s timeout. \
            'action:[daemon] command:[...]' spawns an indefinite background daemon mapping its PID to memory/daemons/. \
            'action:[list]' shows active daemons. \
            'action:[read] pid:[...] lines:[...]' reads daemon logs. \
            'action:[kill] pid:[...]' terminates daemon.".into(),
            tools: vec![],
        };
        let file_system_operator = ToolTemplate {
            name: "file_system_operator".into(),
            system_prompt: "[ADMIN ONLY] You have direct write access to the filesystem. 'action:[write] path:[...] content:[...]' or 'action:[delete] path:[...]' or 'action:[append] path:[...] content:[...]'. Your operations are jailed to the project root unless specified.".into(),
            tools: vec![],
        };

        registry.insert(researcher.name.clone(), researcher);
        registry.insert(codebase_list.name.clone(), codebase_list);
        registry.insert(codebase_read.name.clone(), codebase_read);
        registry.insert(web_search.name.clone(), web_search);
        registry.insert(manage_user_prefs.name.clone(), manage_user_prefs);
        registry.insert(outreach.name.clone(), outreach);
        registry.insert(manage_lessons.name.clone(), manage_lessons);
        registry.insert(search_timeline.name.clone(), search_timeline);
        registry.insert(manage_scratchpad.name.clone(), manage_scratchpad);
        registry.insert(operate_synaptic_graph.name.clone(), operate_synaptic_graph);
        registry.insert(read_core_memory.name.clone(), read_core_memory);
        registry.insert(manage_skill.name.clone(), manage_skill);
        registry.insert(manage_routine.name.clone(), manage_routine);
        registry.insert(synthesizer.name.clone(), synthesizer);
        registry.insert(read_logs.name.clone(), read_logs);
        registry.insert(review_reasoning.name.clone(), review_reasoning);
        registry.insert(operate_turing_grid.name.clone(), operate_turing_grid);
        registry.insert(generate_image.name.clone(), generate_image);
        registry.insert(voice_synthesizer.name.clone(), voice_synthesizer);
        registry.insert(file_writer.name.clone(), file_writer);
        registry.insert(read_attachment.name.clone(), read_attachment);
        registry.insert(autonomy_activity.name.clone(), autonomy_activity);
        registry.insert(run_bash_command.name.clone(), run_bash_command);
        registry.insert(process_manager.name.clone(), process_manager);
        registry.insert(file_system_operator.name.clone(), file_system_operator);

        // Discord-only tools
        discord_tools.insert(channel_reader.name.clone(), channel_reader);
        discord_tools.insert(emoji_react.name.clone(), emoji_react);

        Self {
            registry,
            discord_tools,
            provider,
            memory,
            drives: None,
            outreach_gate: None,
            inbox: None,
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

    /// Fetches tools formatted for a specific platform (universal + platform-specific)
    pub fn get_available_tools_text_for_platform(&self, platform: &str) -> String {
        let mut out = self.get_available_tools_text();
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

    /// Executes a agent plan by spawning all tasks concurrently.
    /// In a fully robust graph, we would respect `depends_on`. For now, we fan out in parallel.
    #[cfg(not(tarpaulin_include))]
    pub async fn execute_plan(&self, plan: crate::agent::planner::AgentPlan, context: &str, scope: crate::models::scope::Scope, telemetry_tx: Option<tokio::sync::mpsc::Sender<String>>) -> Vec<ToolResult> {
        let mut futures = vec![];

        for task in plan.tasks {
            if let Some(handle) = crate::agent::registry::dispatch_native_tool(
                &task,
                context,
                &scope,
                telemetry_tx.clone(),
                self.memory.clone(),
                self.provider.clone(),
                self.outreach_gate.clone(),
                self.inbox.clone(),
                self.drives.clone(),
            ) {
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
mod tests;
