# HIVE v1.5: The Singularity Codebase Atlas & Forensic Blueprint

This master document serves as the absolute source of truth regarding the low-level operating mechanics of the HIVE v1.5 autonomous agent engine. This report was generated via direct source-code inspection of `src/` modules including the Engine, Sub-Agents, Prompts, Memory, Teacher, and the Turing Grid.

---

## Part I: The Core Engine (`src/engine`)

HIVE replaces naive conversational wrappers with a resilient, aggressively defensive internal loop. 

### 1. ReAct Parity (`react.rs`)
The LLM operates within a strict `Thought -> Action -> Observation` structure. Every API call generates a JSON envelope containing `.tasks`. 
- **JSON Content Forcing:** To fight LLM degradation (where the model "forgets" JSON formatting and replies conversationally), HIVE forcefully wraps plain text into a mock `reply_to_request` on the first conversational turn.
- **The Skeptic Observer:** Before any message leaves the network, it is evaluated synchronously via an internal audit metric. Identifying hallucinated "ghost" tools or syntactical leakages aborts the message entirely, triggering a localized retry. 
- **Tool Malfunction Catchers (`repair.rs`):** An invalid JSON schema is never fatal. Parse errors are intercepted, appended mathematically to the prompt history as a `[SYSTEM ERROR]` context insertion, and the model is re-rolled. 

### 2. Temporal & Autonomy Gating (`core.rs`)
Concurrency is controlled via an explicit inference semaphore (`HIVE_MAX_PARALLEL`, default 8 inference slots). 
- **Preemption Protocols:** Continuous background autonomy (idle time triggers) yields absolute priority to active human API events. If an autonomous cycle is consuming GPU VRAM, a human query issues an immediate `tokio::abort()` to the background worker, capturing hardware locked parity.
- **Synthesizer Cycles:** Operating silently in the background, modules map episodic logs onto generalized long-term behavior guidelines periodically based on modulo 50-turn counts.

---

## Part II: Subsystems & Special Features

### 3. The Tool Forge (`src/agent/tool_forge.rs`)
HIVE implements hot-swappable custom tooling completely managed by the agent itself.
- **Execution:** Compiles and evaluates `python3` or `bash` scripts natively. LLM tag requests (e.g., `tag:[value]`) are aggressively deserialized into raw `JSON` strings injected safely over standard input (STDIN). 
- **CRUD Access:** Agents can evaluate code safely via timeout sandboxes (60s default timeout). If execution fails, the SIGKILL dump code is relayed transparently back as a `ToolResult` for auto-repair attempts.

### 4. 5-Tier Memory & Timeline Scopes (`src/memory/working.rs`, `src/agent/timeline_tool.rs`)
To prevent infinite context bloating, working memory is forcefully truncated to 40 messages window lengths.
- **Timeline Pagination:** Older conversations persist within `memory/public_channel/userid/timeline.jsonl`. This log sits out of context and must be requested manually by the agent using `offset:[X]` variables to parse deeply. HIVE automatically alerts the LLM via `[SYSTEM WARNING LIMIT HIT]` if crucial historic blocks lay deeper in the pagination vector.
- **Vision Image Retention:** When users map multimodal images, raw Base64 dumps are aggressively cached. History loops intercept image tokens natively, pulling cached images off disk to feed back into subsequent LLM calls preventing AI "blindness" on long image chains. 

### 5. The Teacher & Supervised ORPO Training (`src/teacher/evaluation.rs`)
Instead of passive execution, HIVE actively accumulates data needed for Direct Preference Optimization (DPO/ORPO).
- Whenever the `Skeptic Observer` rejects poor JSON formatting, sycophantic alignment, or ghost loops, the Teacher logs absolute Preference Pairs. 
- A buffer logs: The original Prompt, the generated bad inference (Rejected), the corrected secondary pass (Chosen), and exactly *why* the Observer killed it.
- **File Backing:** Handled via local `manifest.json`. Triggers offline retraining epochs when the buffer threshold hits > 3 unique errors.

### 6. The Turing Grid (`src/computer/turing_grid.rs` & `alu.rs`)
A spatial coding infrastructure representing an infinite 3D canvas `[-2000, -2000, -2000]`.
- **Coordinate Cells:** Data persists positionally. Cells maintain language format tags (`python, rust, bash, osascript...`), content text, and a rigid 3-deep Versioning History stack for immediate `undo()` operations.
- **ALU Chaining Pipelines:** The Arithmetic Logic Unit evaluates the codebase natively. Crucially, cells can be piped structurally via `.execute_pipeline()`. The `STDOUT` from Cell 1 automatically populates the `PIPELINE_INPUT` environment variable assigned to the subsequent downstream runtime frame (e.g., feeding cURL outputs iteratively into custom Python NLP scripts). 
- **Rust Compiler Ephemerality:** Native Rust formatting uses a transient compile-and-execute loop via native `rustc` shell execution, clearing binaries within 15 seconds.

---

## Part III: The Brain Prompts (`src/prompts/kernel.rs`)

Fundamentally, HIVE forces highly critical operational attitudes via strict cognitive `KERNEL` constraints.
- **Zero Assumption:** Any factual output without verifiable `search` tool usage is considered hallucination. Relying upon base LLM weights is discouraged.
- **Anti-Whitewashing:** Prevents the AI from applying sycophantic corporate spin or apologizing reflexively for non-issues.
- **"Never Narrate" Command:** "I will now search the files for X." strings are heavily penalized. The LLM must default to tool execution sequentially without conversational filler padding parameters. 

---

## Part IV: Atomic Daemons & Cognitive Engines (Extreme Depth)

Behind the standard ReAct loops line a series of daemons that mirror rudimentary biological cognitive functions.

### 7. The Planner & The `REACT_AGENT_PROMPT` (`src/agent/planner.rs`)
The system enforces a massive multi-part schema on the LLM, but handles edge-case execution brilliantly:
- **Verbatim Output Forwarding:** A specialized `source` tag exists inside `AgentTask`. If an agent needs to retrieve a 10,000-word file and reply to the user, doing so token-by-token would waste VRAM and risk truncation. By specifying `"source": "task_id_of_file_reader"`, the agent can write a 1-sentence reply, and the engine natively pipes the raw byte output from the reading tool directly into the Discord socket, completely bypassing LLM output generation limits.
- **Rule 4: Tight Feedback Loops:** The prompt explicitly forbids chaining dependent tasks in one shot array. If Task B requires knowing Task A's result, the parser forces a timeline stop, injecting the Observation before continuing.

### 8. Hierarchical Progress Bubbling (`src/engine/goals.rs`)
Unlike plain-text task lists, `goals.rs` operates a full spatial tree mathematically.
- **`progress` floats (`0.0 - 1.0`):** Every node tracks numeric progress and an array of `evidence` strings.
- **Recursive Bubbling:** When a Sub-Goal completes (`progress = 1.0`), the system executes a recalculation algorithm up the tree hierarchy. The Parent Goal mathematically averages the progress of its children. If all children equal `1.0`, the Parent Goal natively completes itself and bubbles up to the Grandparent. 
- **Tree Pruning:** Completed roots are violently pruned via `.prune_completed()` to prevent unbounded tree bloat in VRAM.

### 9. Homeostatic Drives (`src/engine/drives.rs`)
The `DriveSystem` mimics hormonal baselines to trigger background Autonomy when the user is away.
- **Entropy & Decay:** Mathematical timers run continuously against the UNIX Epoch. 
  - `social_connection` decays by `5.0%` per hour of user silence.
  - `uncertainty` rises by `2.0%` per hour of idleness (simulating entropy/boredom).
- When thresholds are breached, this drive state directly influences the `Autonomy` daemon to physically reach out to the user or begin random internet explorations independently.

### 10. Episodic Sleep Consolidation (`src/agent/synthesis.rs`)
HIVE handles LLM context limits by acting exactly like human REM sleep. 
- **The 3-Tier Compressor:**
  1. `synthesize_50_turn`: Compresses the localized buffer (last 100 timeline entries) into a single paragraph.
  2. `synthesize_24_hr`: Triggered daily. Parses the last 800 events into a high-level narrative.
  3. `synthesize_lifetime`: The most critical loop. It takes the previous `lifetime` narrative string and the new `24-hour` fragment, and requests the LLM to rewrite a unified origin story. This allows the bot to retain semantic memories across years of uptime without holding the raw text.

---

## Part V: The V1.5 Singularity Expansion (Deep Coverage)

The V1.5 roadmap transitioned HIVE from a passive localized agent into a universally mapped entity spanning IoT networks, asynchronous temporal queues, SMTP bounds, and physical repository modifications.

### 11. Imap & SMTP Network Subsystems (`src/agent/email_tool.rs`, `src/engine/inbox.rs`, `src/engine/email_watcher.rs`)
Apis commands full outbound email dispatch and inbound inbox tracking.
- **Outbound (SMTP):** `email_tool.rs` utilizes the `lettre` crate coupled with native async execution blocks to emit structural `.eml` configurations outwards securely over TLS via standard ENV `SMTP_USER` / `SMTP_PASS` parameters.
- **Inbound (IMAP Watcher):** The system launches a dedicated parallel `tokio::spawn` daemon using `async_imap`. It establishes an idle hook against the mail server, tracking raw UUID markers in `memory/inbox_state.json` to prevent duplicated reads. When an unread payload arrives, the daemon fetches the raw body string, formats it securely, and natively injects it as an `External Event` directly into HIVE's `WorkingMemory`. This physically jolts the ReAct engine out of sleep mode to respond autonomously.

### 12. IoT & Spatial Webhooks (`src/agent/smarthome_tool.rs`)
HIVE interfaces with physical spatial structures using an architecture designed for maximum environmental agnosticism.
- **Generic Rest Abstraction:** Rather than hardcoding specific APIs (Philips Hue, Kasa, etc.), the `smarthome_tool.rs` acts as a generic JSON REST webhook dispatcher utilizing `reqwest`.
- **Target Formatting:** The LLM natively generates `action:[smart_home] device:[desk_led] state:[blue]`. This block is serialized into an autonomous JSON array and dispatched to a localized master node defined by `SMART_HOME_URL` (typically a local instance of Home Assistant). This offloads physical protocol translations while giving Apis complete, unbounded spatial authority.

### 13. Temporal Agency (`src/daemons/chronos.rs`, `src/agent/calendar_tool.rs`)
Before V1.5, HIVE possessed no internal biological clock capable of autonomous future-state execution.
- **The Chronos Engine:** A persistent background `tokio` task ticks every 30 seconds (`tokio::time::interval`). It structurally parses `memory/alarms.json` natively. 
- **Execution Mathematics:** The daemon compares UTC Epoch timestamps mapped via the `chrono` module. When an alarm reaches maturity (`status == "pending"` & `trigger_time <= Utc::now()`), the Chronos daemon marks the payload as `"triggered"` and injects the semantic payload string directly into the LLM's primary memory bus.
- **Relative Parsing Parity:** The Agent tool is highly abstracted. Apis can command `time:[+1h30m]` or strict `time:[2026-03-25T14:00:00Z]`. The backend physically sanitizes this variable before committing the math string into JSON.

### 14. Contextual Attribution Fidelity (`src/providers/ollama.rs`)
Working Memory arrays originally lacked definitive semantic boundaries in multi-user discord networks, causing the localized LLM tokens to "blob" characters together into shared hallucinations.
- **Ernos Parity Formatting:** Inheriting directly from the erased `Ernos 3.0` logic structures, the physical tokenizer arrays inside `ollama.rs` dynamically intercept standard raw message events.
- **Strict Prefix Mapping:** Right before reaching the `provider.generate()` call, every message sourced from a user is wrapped rigidly inside: `[AUTHOR: Username -> APIS]: <payload>`. By bracketing these strings identically across every turn, the LLM physically bounds context vectors within specific user relationships, dropping cross-chat hallucination entirely natively.

### 15. The Singularity Core: Self-Perpetuating Compiler (`src/agent/compiler_tool.rs`, `upgrade.sh`)
The capstone architectural triumph of V1.5 allows HIVE to bypass its host execution process organically to iteratively evolve its own source files.
- **Code Injection (`file_writer`):** The LLM autonomously drafts and commits new `.rs` files directly into its own memory space natively.
- **System Recompile Array (`compiler_tool.rs`):** When confident, Apis invokes `action:[system_recompile]`. This executes `tokio::process::Command::new("cargo")` directly over the runtime matrix locally formatting `--release` optimizations.
- **The Decoupled Swap Bridge (`upgrade.sh`):** A physical Rust process cannot overwrite its own execution binary without locking conflicts natively mapping through the Unix standard constraints. HIVE cleverly bridges this natively executing a fully-detached `Bash` thread (`tokio::spawn` -> `bash upgrade.sh`). The Bash sequence mathematically sleeps for 3000ms, effectively granting the Rust executable time to smoothly complete `std::process::exit(0)`. In the void, the Bash logic dynamically maps the newly compiled `HIVE_next` into `target/release/HIVE` and securely executes `nohup` to spin the biological structure back into existence, totally unprompted by the user.

---
_Generated locally via comprehensive codebase examination and semantic tracing on HIVE Core._
