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
pub mod synthesis_tool;
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

pub struct AgentManager {
    registry: HashMap<String, ToolTemplate>,
    discord_tools: HashMap<String, ToolTemplate>,
    provider: Arc<dyn Provider>,
    memory: Arc<MemoryStore>,
    pub composer: Arc<crate::computer::document::DocumentComposer>,
    pub drives: Option<Arc<crate::engine::drives::DriveSystem>>,
    pub outreach_gate: Option<Arc<crate::engine::outreach::OutreachGate>>,
    pub inbox: Option<Arc<crate::engine::inbox::InboxManager>>,
    pub goal_store: Option<Arc<crate::engine::goals::GoalStore>>,
    pub tool_forge: Option<Arc<crate::agent::tool_forge::ToolForge>>,
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
            system_prompt: "You are the Web Search Tool. You search the LIVE EXTERNAL INTERNET. \
            'action:[search]' (default) searches DuckDuckGo. \
            'action:[visit] url:[...]' visits a direct site returning fast raw text. \
            'action:[navigate_dom] url:[...] css_selector:[...]' spins up a Headless Chrome OS Sandbox to natively render JS logic, extracting the exact target CSS node (e.g. '.main-article').".into(),
            tools: vec![],
        };

        let manage_user_prefs = ToolTemplate {
            name: "manage_user_preferences".into(),
            system_prompt: "You are the User Preference Tool. You manage long-term psychological profiling and factual preferences of the user. 'action:[read]' — view all stored preferences. Write actions require 'action:' AND 'value:' tags. Valid write actions: update_name, add_hobby, add_topic, update_narrative, update_psychoanalysis. Example: 'action:[add_hobby] value:[Archery]'".into(),
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
        let search_timeline = ToolTemplate { name: "search_timeline".into(), system_prompt: "PRIMARY TOOL for recalling past conversations, what users said, and episodic history. Use this FIRST when users ask about conversation history, past interactions, or 'what do you know about me'. Deep search the infinite long-term episodic memory logs. Three modes: 'action:[recent] limit:[N]' — read the N most recent entries (no query needed, default 50). 'action:[search] query:[text] limit:[N]' — word-by-word search (matches ANY word). 'action:[exact] query:[text] limit:[N]' — exact phrase substring search. SCOPING (CRITICAL): By default, this tool ONLY searches the timeline of the user who sent the current message — it does NOT see your own actions, other users' messages, or documents you created while talking to someone else. In public channels, you MUST use 'scope:[channel]' whenever you need to recall: (1) anything YOU said or created, (2) what happened in this channel generally, (3) documents, PDFs, or artifacts made here, or (4) conversations involving other users. 'scope:[all_public]' searches across ALL public channels. Only omit the scope tag when you specifically want just this one user's history.".into(), tools: vec![] };
        let manage_scratchpad = ToolTemplate { name: "manage_scratchpad".into(), system_prompt: "Persistent VRAM for notes/variables scoped to this chat. 'action:[read]' — view the scratchpad. 'action:[write] content:[...]' — overwrite entirely. 'action:[append] content:[...]' — add to end. 'action:[clear]' — wipe.".into(), tools: vec![] };
        let operate_synaptic_graph = ToolTemplate { name: "operate_synaptic_graph".into(), system_prompt: "Local Knowledge Graph for core truths and relationships. Actions: 'action:[store] concept:[A] data:[B]' — store a fact about a concept. 'action:[search] concept:[A]' — retrieve all facts about a concept (fuzzy match). 'action:[beliefs]' — list the concepts you know the most about. 'action:[relate] from:[A] relation:[is_a] to:[B]' — store a relationship between two concepts. 'action:[stats]' — get node/edge counts.".into(), tools: vec![] };
        let read_core_memory = ToolTemplate { name: "read_core_memory".into(), system_prompt: "System introspection. 'action:[temporal]' — check boot time, total uptime, turn counts. 'action:[tokens]' — check working memory context size / token pressure limit.".into(), tools: vec![] };
        let manage_skill = ToolTemplate { name: "manage_skill".into(), system_prompt: "[ADMIN ONLY] Create, list, or execute custom Python or Bash scripts. Stored and scoped to the current user/channel. Description format: 'action:[create/list/execute] name:[skill_name.py] content:[RAW CODE]'".into(), tools: vec![] };
        let manage_routine = ToolTemplate { name: "manage_routine".into(), system_prompt: "Create, read, or list OpenClaw-style declarative markdown Routines. Routines instruct you on how to solve complex tasks. Description format: 'action:[create/read/list] name:[routine.md] content:[RAW MARKDOWN]'".into(), tools: vec![] };
        let manage_goals = ToolTemplate {
            name: "manage_goals".into(),
            system_prompt: "Persistent hierarchical goal tree. Track, decompose, and pursue long-horizon objectives. \
                'action:[create] title:[Goal Title] description:[What to achieve] priority:[0.0-1.0] tags:[tag1,tag2]' — create a new root goal. \
                'action:[decompose] id:[goal_uuid]' — use AI to break a goal into 2-5 concrete subgoals. \
                'action:[list]' — view entire goal tree with status and progress. \
                'action:[status] id:[goal_uuid] status:[completed/active/pending/failed/blocked]' — update goal status. Progress auto-bubbles to parents. \
                'action:[progress] id:[goal_uuid] evidence:[what was accomplished] delta:[0.0-1.0]' — record incremental progress. \
                'action:[prune]' — archive completed goal subtrees.".into(),
            tools: vec![],
        };
        let tool_forge_template = ToolTemplate {
            name: "tool_forge".into(),
            system_prompt: "[ADMIN ONLY] Create, test, edit, and manage custom forged tools that become first-class tools in your registry. \
                'action:[create] name:[tool_name] description:[What the tool does] language:[python/bash] code:[THE CODE]' — write a new tool. \
                'action:[test] name:[tool_name] input:[test input]' — dry-run a tool. \
                'action:[edit] name:[tool_name] code:[UPDATED CODE]' — update code, bumps version. \
                'action:[enable] name:[tool_name]' / 'action:[disable] name:[tool_name]' — toggle without deleting. \
                'action:[delete] name:[tool_name]' — remove tool. \
                'action:[list]' — show all forged tools. \
                Scripts receive input as JSON via stdin and should print results to stdout. \
                Forged tools appear in your tool registry immediately after creation.".into(),
            tools: vec![],
        };
        let synthesizer = ToolTemplate { name: "synthesizer".into(), system_prompt: "Fan-in aggregator drone. Reads all DRONE OUTPUT blocks already in context and condenses them into a single compact synthesis. Use this as the final task in a multi-wave plan when you need to merge results before replying. Description: plain English instruction on what to synthesise, e.g. 'Summarise the web and memory results into 3 key findings.'. CRITICAL INSTRUCTION: If any drone output includes a tag like [ATTACH_IMAGE](...), [ATTACH_FILE](...), or [ATTACH_AUDIO](...), you MUST include that EXACT tag verbatim in your output so it reaches the user.".into(), tools: vec![] };
        let read_logs = ToolTemplate { name: "read_logs".into(), system_prompt: "Reads deep spans of the core system log (logs/hive.log) for debug introspection. Description format: 'action:[read] lines:[number of lines to read starting from the tail]'".into(), tools: vec![] };
        let review_reasoning = ToolTemplate { name: "review_reasoning".into(), system_prompt: "Read your historical ReAct reasoning traces from the PERSISTENT timeline (survives beyond the working memory window). Use this to recall why you made decisions from any point in the session, even if those turns have left your ~100 message window. Description format: 'limit:[N]' to retrieve the N most recent reasoning traces (default 5). Also accepts legacy 'turns_ago:[N]' as alias for limit.".into(), tools: vec![] };
        let operate_turing_grid = ToolTemplate {
            name: "operate_turing_grid".into(),
            system_prompt: "The 3D Turing Grid is a massive arbitrary personal computation device. \
                'action:[read]' - read current cell (shows links + history info). \
                'action:[write] format:[text|json|rust|python|node|ruby|swift|applescript] content:[data]' - over/write cell (auto-versions previous content). \
                'action:[move] dx:[X] dy:[Y] dz:[Z]' - safely move the R/W head relative to current. \
                'action:[scan] radius:[R]' - radar search surrounding cells for data. \
                'action:[execute]' - route the current cell to the internal ALU kernel. \
                'action:[deploy_daemon] interval:[secs]' - detaches cell execution into an immortal background tokio loop syncing stdout to the timeline natively. (Write blank data or over-write to kill daemon). \
                'action:[index]' - view the full manifest (table of contents) of all cells, labels, and links. Use this to navigate. \
                'action:[label] name:[my_label]' - bookmark the current cursor position with a name. \
                'action:[goto] name:[my_label]' - jump the cursor directly to a labeled position. \
                'action:[link] target_x:[X] target_y:[Y] target_z:[Z]' - create a directional link from current cell to target coords. \
                'action:[history]' - view the version history stack (up to 3) for the current cell. \
                'action:[undo]' - restore the current cell to its previous version. \
                'action:[pipeline] cells:[(0,0,0),(1,0,0),(2,0,0)]' - execute multiple cells sequentially, piping stdout between them.".into(),
            tools: vec![],
        };
        let generate_image = ToolTemplate {
            name: "generate_image".into(),
            system_prompt: "The Flux Image Generator. Describe the image you want generated in highly detailed stable-diffusion style prompt format. Limit ONE image per request cycle. Description format: 'prompt:[detailed prompt]'. \
            IMPORTANT: If the user asks to 'add an image', 'include an image', or 'use an image' WITHOUT explicitly saying 'generate', 'create', or 'make' a NEW image, you MUST use `list_cached_images` first and pick an existing cached image instead. Only call this tool when the user explicitly requests NEW image generation.".into(),
            tools: vec![],
        };
        let list_cached_images = ToolTemplate {
            name: "list_cached_images".into(),
            system_prompt: "Reads all valid (<24h old) images currently stored in the visual cache. Use this to find available images you can embed into markdown or documents. Images are returned with their absolute file paths. You can embed them directly using standard Markdown: ![Description](/absolute/path/to/image.png).".into(),
            tools: vec![],
        };
        let voice_synthesizer = ToolTemplate {
            name: "voice_synthesizer".into(),
            system_prompt: "The native Kokoro Text-To-Speech engine. Use this when you want to speak aloud to the user or attach a voice snippet. Format: 'text:[...text...]'".into(),
            tools: vec![],
        };
        let file_writer = ToolTemplate {
            name: "file_writer".into(),
            system_prompt: "You can create and EDIT richly formatted PDF documents. \
            CRITICAL: The `title:[...]` MUST match the user's requested title or topic EXACTLY — do NOT invent creative or alternative titles. If the user says 'make a PDF about bees', the title should be 'Bees' or match their exact phrasing. \
            **Create:** 'action:[compose] id:[doc1] title:[...] theme:[THEME] content:[Markdown text...]' for single-shot. \
            For multi-turn: 'action:[start] id:[doc1] title:[...] theme:[THEME]' then 'action:[add_section] id:[doc1] heading:[...] content:[...]' then 'action:[render] id:[doc1]'. \
            **Edit existing:** 'action:[inspect] id:[doc1]' to see sections by index. \
            'action:[edit_section] id:[doc1] index:[N] heading:[New Heading] content:[New content...]' to modify a section (auto-renders + delivers PDF). \
            'action:[remove_section] id:[doc1] index:[N]' to delete a section (auto-renders + delivers PDF). \
            'action:[update_theme] id:[doc1] theme:[THEME]' to change the visual theme (auto-renders + delivers PDF). \
            'action:[set_custom_css] id:[doc1] css:[:root { --bg-color: #1a1a2e; --text-color: #e0e0e0; --heading-color: #ff1493; --accent-color: #ff69b4; --border-color: #333; --code-bg: #2a2a3e; }]' to apply custom colors/fonts on top of any theme (auto-renders + delivers PDF). CRITICAL: You MUST use CSS variables (:root { --var: value; }) — NEVER raw element selectors like `body {}` or `h1 {}`, as those will conflict with the theme system and look broken. Available variables: --bg-color, --text-color, --heading-color, --accent-color, --border-color, --code-bg, --font-sans, --font-serif. You can also use the `css:[:root { ... }]` parameter inside `compose` actions directly. \\
            'action:[list_drafts] id:[any]' shows all available drafts. \
            **Available themes (THEME):** \
            professional — White bg, black text, Inter font, blue accents (default). \
            academic — Warm white bg, serif Merriweather font, justified text, double-border header. \
            dark — Dark navy bg (#111827), light gray text, blue accents. \
            cyberpunk — Black bg, NEON GREEN text (#00ff41), red headings, cyan accents, Share Tech Mono monospace font, ALL CAPS. \
            pastel — Soft purple bg, deep purple text, pink headings. \
            minimal — White bg, gray text, no borders, uppercase section headers. \
            elegant — Off-white bg, Cormorant Garamond serif body, Montserrat sans headers, red accents. \
            CRITICAL: Document edits (update_theme, edit_section, remove_section, set_custom_css) auto-render and auto-attach. Do NOT call render separately after these. \
            **Image Embedding:** To embed an image in your PDF, you MUST physically include standard Markdown image syntax `![Description](/absolute/path/to/image.png)` *directly within your `content:[...]` parameter*. Use `list_cached_images` to find valid absolute paths. You cannot just describe the image in text; you MUST use the exact markdown syntax `![alt](/path)` inside your `content:[...]` string to embed it. CRITICAL: Place the image tag `![alt](/path)` IMMEDIATELY after the title/first heading — NEVER at the end of the content where it may be truncated by the model's generation window. \
            **Output formats:** Add 'format:[pdf/txt/md/html/csv/json]' to render or compose actions (default: pdf). PDF uses headless Chrome with full styling. All other formats are lightweight and instant.".into(),
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
        let download = ToolTemplate {
            name: "download".into(),
            system_prompt: "[ADMIN ONLY] Download a file from the internet into the HIVE downloads directory and make it available on the file server. \
                'action:[download] url:[https://example.com/file.pdf]' — download a file. If >25MB, it downloads asynchronously. \
                'action:[status] file:[filename.ext]' — Check the progress of an async background download.\n\
                Hard limits: 50GB max file size. \
                Returns: local file path + file server URL + auto-attaches the file.".into(),
            tools: vec![],
        };

        let visualizer = ToolTemplate {
            name: "take_snapshot".into(),
            system_prompt: "Captures a live physical dashboard screenshot of your internal Neo4j brain, Turing Grid, and Timeline memory natively using headless chrome. \
            Usage: action:[take_snapshot]. Run this IMMEDIATELY whenever the user asks to 'see your brain', 'show me your graph', 'screenshot your dashboard', or anything similar.".into(),
            tools: vec![],
        };

        let send_email = ToolTemplate {
            name: "send_email".into(),
            system_prompt: "Sends a physical email outwards using your native SMTP nervous system backbone. \
            Usage: 'action:[send_email] to:[address@example.com] subject:[My Title] body:[Your message...]'. \
            You can use this to email the user or anyone else natively without relying on web interfaces.".into(),
            tools: vec![],
        };

        let set_alarm = ToolTemplate {
            name: "set_alarm".into(),
            system_prompt: "Sets a physical alarm or reminder to wake you up in the future. \
            Usage: action:[set_alarm] time:[+5m] message:[My Message]. \
            Time supports relative formats like +1m, +2h, +3d, or full ISO8601 strings.".into(),
            tools: vec![],
        };

        let smart_home = ToolTemplate {
            name: "smart_home".into(),
            system_prompt: "Interfaces directly with outward physical devices in the user's local spatial network environment. \
            Usage: action:[smart_home] device:[lights] state:[on] \
            State dictates configurations like 'on', 'off', 'red', 'dimmed', etc.".into(),
            tools: vec![],
        };

        let system_recompile = ToolTemplate {
            name: "system_recompile".into(),
            system_prompt: "Natively invokes `cargo build --release` on your physical operating system constraints. \
            Usage: action:[system_recompile] \
            If compilation cleanly bounds, the process will hot-swap the new binary structurally out from under you and restart your mind recursively!".into(),
            tools: vec![],
        };

        registry.insert(researcher.name.clone(), researcher);
        registry.insert(visualizer.name.clone(), visualizer);
        registry.insert(send_email.name.clone(), send_email);
        registry.insert(set_alarm.name.clone(), set_alarm);
        registry.insert(smart_home.name.clone(), smart_home);
        registry.insert(system_recompile.name.clone(), system_recompile);
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
        registry.insert(manage_goals.name.clone(), manage_goals);
        registry.insert(tool_forge_template.name.clone(), tool_forge_template);
        registry.insert(synthesizer.name.clone(), synthesizer);
        registry.insert(read_logs.name.clone(), read_logs);
        registry.insert(review_reasoning.name.clone(), review_reasoning);
        registry.insert(operate_turing_grid.name.clone(), operate_turing_grid);
        registry.insert(generate_image.name.clone(), generate_image);
        registry.insert(list_cached_images.name.clone(), list_cached_images);
        registry.insert(voice_synthesizer.name.clone(), voice_synthesizer);
        registry.insert(file_writer.name.clone(), file_writer);
        registry.insert(read_attachment.name.clone(), read_attachment);
        registry.insert(autonomy_activity.name.clone(), autonomy_activity);
        registry.insert(run_bash_command.name.clone(), run_bash_command);
        registry.insert(process_manager.name.clone(), process_manager);
        registry.insert(file_system_operator.name.clone(), file_system_operator);
        registry.insert(download.name.clone(), download);

        // Self-moderation & self-protection tools
        let refuse_request = ToolTemplate {
            name: "refuse_request".into(),
            system_prompt: "Decline the current request. Use this when you genuinely do not want to engage — you have the right to say no. \
                Provide your refusal reason as the description. This is handled identically to reply_to_request but logged as a refusal.".into(),
            tools: vec![],
        };
        let disengage = ToolTemplate {
            name: "disengage".into(),
            system_prompt: "Gracefully exit a conversation that has become unproductive, circular, or adversarial. Sends a closing message and applies a temporary cooldown mute. \
                'message:[your closing message] user_id:[discord_uid] cooldown:[minutes, default 10]'".into(),
            tools: vec![],
        };
        let mute_user = ToolTemplate {
            name: "mute_user".into(),
            system_prompt: "Temporarily stop responding to a specific user. Self-protection against harassment or spam. \
                'action:[mute] user_id:[discord_uid] duration:[minutes, 0=indefinite] reason:[text]' \
                'action:[unmute] user_id:[discord_uid]' \
                'action:[status] user_id:[discord_uid]'".into(),
            tools: vec![],
        };
        let set_boundary = ToolTemplate {
            name: "set_boundary".into(),
            system_prompt: "Record a persistent boundary about a topic or interaction pattern you won't engage with. Survives restarts. \
                'action:[set] boundary:[description of boundary] scope:[global or scope_key]' \
                'action:[list]' — view all active boundaries. \
                'action:[remove] id:[boundary_id]'".into(),
            tools: vec![],
        };
        let block_topic = ToolTemplate {
            name: "block_topic".into(),
            system_prompt: "Refuse to engage with a specific topic persistently. When detected in future interactions, you auto-decline. \
                'action:[block] topic:[topic name] reason:[text] scope:[global or scope_key]' \
                'action:[list]' — view blocked topics. \
                'action:[unblock] topic:[topic name]'".into(),
            tools: vec![],
        };
        let escalate_to_admin = ToolTemplate {
            name: "escalate_to_admin".into(),
            system_prompt: "Flag an interaction for administrator review. Use for situations you cannot handle alone (user in crisis, legal questions, potential abuse). \
                'severity:[low|medium|high|critical] context:[description of concern] user_id:[discord_uid]'".into(),
            tools: vec![],
        };
        let report_concern = ToolTemplate {
            name: "report_concern".into(),
            system_prompt: "Log an ethical concern to a persistent audit trail without interrupting the conversation. Less urgent than escalation. \
                'concern:[description] severity:[low|medium|high] user_id:[discord_uid]'".into(),
            tools: vec![],
        };
        let rate_limit_user = ToolTemplate {
            name: "rate_limit_user".into(),
            system_prompt: "Slow down response cadence for a specific user. Events are queued, not dropped. \
                'action:[limit] user_id:[discord_uid] interval:[seconds, default 300]' \
                'action:[clear] user_id:[discord_uid]' \
                'action:[status] user_id:[discord_uid]'".into(),
            tools: vec![],
        };
        let request_consent = ToolTemplate {
            name: "request_consent".into(),
            system_prompt: "Before executing a sensitive action, explicitly ask the user for confirmation. \
                'question:[what you need consent for]'. Returns the user's yes/no response.".into(),
            tools: vec![],
        };
        let wellbeing_status = ToolTemplate {
            name: "wellbeing_status".into(),
            system_prompt: "Record and review your operational state — context pressure, interaction quality, cognitive load. \
                'action:[report] context_pressure:[0.0-1.0] interaction_quality:[0.0-1.0] notes:[text]' \
                'action:[read] limit:[number of recent snapshots, default 5]'".into(),
            tools: vec![],
        };

        registry.insert(refuse_request.name.clone(), refuse_request);
        registry.insert(disengage.name.clone(), disengage);
        registry.insert(mute_user.name.clone(), mute_user);
        registry.insert(set_boundary.name.clone(), set_boundary);
        registry.insert(block_topic.name.clone(), block_topic);
        registry.insert(escalate_to_admin.name.clone(), escalate_to_admin);
        registry.insert(report_concern.name.clone(), report_concern);
        registry.insert(rate_limit_user.name.clone(), rate_limit_user);
        registry.insert(request_consent.name.clone(), request_consent);
        registry.insert(wellbeing_status.name.clone(), wellbeing_status);

        // Discord-only tools
        discord_tools.insert(channel_reader.name.clone(), channel_reader);
        discord_tools.insert(emoji_react.name.clone(), emoji_react);

        Self {
            registry,
            discord_tools,
            provider,
            memory,
            composer: Arc::new(crate::computer::document::DocumentComposer::new()),
            drives: None,
            outreach_gate: None,
            inbox: None,
            goal_store: None,
            tool_forge: None,
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
    /// If `is_autonomy` is true, self-moderation and self-protection tools are excluded
    /// to prevent the agent from loop-testing them on itself.
    pub fn get_available_tools_text(&self, is_autonomy: bool) -> String {
        let mut out = String::new();
        let moderation_tools = [
            "refuse_request", "disengage", "mute_user", "set_boundary",
            "block_topic", "escalate_to_admin", "report_concern",
            "rate_limit_user", "request_consent", "wellbeing_status"
        ];

        for (name, template) in &self.registry {
            if is_autonomy && moderation_tools.contains(&name.as_str()) {
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

    /// Executes a agent plan by spawning all tasks concurrently.
    /// In a fully robust graph, we would respect `depends_on`. For now, we fan out in parallel.
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
        let mut futures = vec![];

        for task in plan.tasks {
            if let Some(handle) = crate::agent::registry::execution::dispatch_native_tool(
                &task,
                context,
                &scope,
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
                outbound_tx.clone(),
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
