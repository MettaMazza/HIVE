<p align="center">
  <img src="docs/banner.png" alt="HIVE Engine — Autonomous AI Agent Architecture" width="100%" />
</p>

<p align="center">
  <a href="https://discord.gg/KhjYX3U3AW"><img src="https://img.shields.io/badge/🐝_Talk_to_Apis-Join_Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white" /></a>
  <img src="https://img.shields.io/badge/lang-Pure_Rust-F46623?style=for-the-badge&logo=rust&logoColor=white" />
  <img src="https://img.shields.io/badge/LLM-Ollama_Local-0969DA?style=for-the-badge" />
  <img src="https://img.shields.io/badge/lines-35K+-FFB800?style=for-the-badge" />
  <img src="https://img.shields.io/badge/tests-463_passing-00C853?style=for-the-badge" />
  <img src="https://img.shields.io/badge/modules-131-A855F7?style=for-the-badge" />
</p>

<h1 align="center">🐝 HIVE Engine</h1>

<p align="center">
  <strong>A sovereign, fully-local AI agent runtime written from the ground up in pure Rust.</strong><br/>
  No cloud dependencies. No API keys to OpenAI. No frameworks. Just raw systems engineering.
</p>

<p align="center">
  <a href="https://discord.gg/KhjYX3U3AW">
    <img src="https://img.shields.io/badge/⚡_Try_Apis_Now_—_Free_on_Discord-FFB800?style=for-the-badge&logoColor=black" />
  </a>
</p>

---

## 🎯 What is HIVE?

HIVE is a **fully autonomous AI agent engine** that runs entirely on your hardware. It powers **Apis** — an AI persona that doesn't just answer questions, but *thinks*, *acts*, *remembers*, and *evolves*.

Unlike wrapper bots that relay messages to cloud APIs, HIVE is a **purpose-built cognitive runtime**:

- 🧠 **Multi-turn ReAct Loop** — Apis reasons, selects tools, observes results, and iterates autonomously. It decides when to stop, not the user.
- 🔒 **Memory-Level Security** — Per-user data isolation enforced at the architecture layer. Private data is *invisible* to other scopes — not by prompting, by design.
- 🛠️ **34 Native Tool Drones** — Web search, code execution, file I/O, image generation, TTS, PDF composition, process management, smart home control, email, calendar, and more — all running locally.
- 📡 **Live Inference HUD** — Watch Apis think in real-time via streaming Discord embeds with reasoning tokens, tool activity, and performance telemetry.
- 🎓 **Self-Supervised Learning** — An integrated Teacher module captures preference pairs and golden examples for continuous improvement.
- 🕸️ **NeuroLease Mesh Network** — Decentralized peer-to-peer weight sharing, binary attestation, and trust-based propagation between HIVE instances.
- 🔄 **Anti-Spiral Recovery** — Automatic detection and recovery from reasoning loops, with interruptible inference and thought-level safeguards.
- 👁️ **Observer Audit Module** — Every response is audited for confabulation, logical inconsistency, and lazy deflection before delivery.

> **Want to see it in action?** Apis is live right now. [**Join the Discord**](https://discord.gg/KhjYX3U3AW) and talk to it for free.

---

## 🏗️ Architecture

```
                          ┌──────────────────────────────────────────────────┐
                          │               🐝 HIVE ENGINE                    │
                          │                                                  │
   ┌──────────┐          │  ┌────────────┐   ┌──────────────┐              │
   │ Discord  │◄─Events─►│  │  ReAct     │◄─►│   Provider   │              │
   │ Platform │          │  │  Loop      │   │  (Ollama)    │              │
   └──────────┘          │  │            │   └──────────────┘              │
                          │  │  Think →   │                                 │
   ┌──────────┐          │  │  Act →     │   ┌──────────────┐              │
   │   CLI    │◄─Events─►│  │  Observe → │◄─►│   Memory     │              │
   │ Platform │          │  │  Repeat    │   │   Store      │              │
   └──────────┘          │  └────────────┘   │  (5-Tier)    │              │
                          │        │          └──────────────┘              │
   ┌──────────┐          │        ▼                                        │
   │ Glasses  │◄─Events─►│  ┌────────────┐   ┌──────────────┐             │
   │ Platform │          │  │  34 Tool   │   │  Observer    │             │
   └──────────┘          │  │  Drones    │   │  (Audit)     │             │
                          │  └────────────┘   └──────────────┘             │
   ┌──────────┐          │        │                                        │
   │ Telemetry│◄─Events─►│        ▼           ┌──────────────┐             │
   │ Platform │          │  ┌────────────┐   │  NeuroLease  │             │
   └──────────┘          │  │  Teacher   │   │  Mesh Net    │             │
                          │  │ (Self-Sup) │   │  (P2P Sync)  │             │
                          │  └────────────┘   └──────────────┘             │
                          └──────────────────────────────────────────────────┘
```

### The Stack

| Layer | What It Does |
|-------|-------------|
| **Platforms** | Trait-based I/O abstraction. Discord, CLI, Glasses, and Telemetry ship out of the box. Adding Telegram or Slack = one `impl Platform`. |
| **ReAct Loop** | Autonomous multi-turn reasoning engine with anti-spiral detection. Apis selects tools, reads observations, recovers from reasoning loops, and decides its own next action. |
| **Tool Drones** | 34 native capabilities spanning information retrieval, code execution, multi-modal generation, memory management, and system automation. |
| **Memory Store** | 5-tier persistence: Working Memory → Scratchpad → Timeline → Synaptic Graph → Lessons. All scope-isolated with compile-time access gates. |
| **Provider** | Local LLM integration via Ollama with streaming token extraction, `<think>` tag parsing, vision support, and interruptible inference. |
| **Observer** | Post-generation audit module that catches confabulation, lazy deflection, logical inconsistency, and architectural leakage before delivery. |
| **Teacher** | Captures reasoning traces, evaluates response quality, and generates preference pairs for RLHF-style continuous improvement. |
| **NeuroLease** | Decentralized mesh network for weight sharing, trust propagation, binary attestation, and integrity verification between HIVE instances. |
| **Kernel** | Core identity protocols: Zero Assumption Protocol, Anti-Gaslighting, Contradiction Resolution, Continuity Recovery, and the full governance constitution. |

---

## 🛠️ The 34 Tool Drones

Apis has access to a full arsenal of native capabilities, all running **locally on your machine**:

<table>
<tr>
<td width="50%">

**🌐 Information & Research**
- `web_search` — Brave-powered web search
- `researcher` — Deep analysis of search results
- `codebase_list` / `codebase_read` — Project introspection
- `read_attachment` — Discord CDN file ingestion
- `channel_reader` — Pull conversation history
- `read_logs` — System log inspection
- `download_tool` — Direct URL downloads

</td>
<td width="50%">

**🧠 Memory & Knowledge**
- `manage_user_preferences` — Per-user preference tracking
- `store_lesson` — Permanent knowledge retention
- `manage_scratchpad` — Session working memory
- `core_memory` — Persistent identity state
- `operate_synaptic_graph` — Associative knowledge links
- `review_reasoning` — Introspect own reasoning traces
- `timeline_tool` — Temporal event management

</td>
</tr>
<tr>
<td>

**⚡ Execution & Creation**
- `operate_turing_grid` — 3D computation sandbox (Python, JS, Rust, Swift, Ruby, Perl, AppleScript)
- `run_bash_command` — Direct shell execution
- `process_manager` — Background daemon orchestration
- `file_system_operator` — Native filesystem I/O
- `file_writer` — PDF/document composition with themes
- `compiler_tool` — Compile and verify code
- `opencode` — Sub-agent IDE orchestration
- `tool_forge` — Dynamic tool creation at runtime

</td>
<td>

**🎨 Multi-Modal & Automation**
- `image_generator` — Local Flux image generation with vision cache
- `kokoro_tts` — Neural text-to-speech (🔊 Speak button on Discord)
- `synthesizer` — Multi-source fan-in compilation
- `manage_routine` / `manage_skill` — Automation & script management
- `email_tool` — SMTP email composition
- `calendar_tool` — Event scheduling
- `contacts_tool` — Contact management
- `smarthome_tool` — IoT device control
- `goal_planner` — Hierarchical goal decomposition
- `emoji_react` — Discord native reactions

</td>
</tr>
</table>

---

## 🔒 Security Model

HIVE enforces privacy at the **memory layer**, not the prompt layer. This means prompt injection attacks cannot leak private data — the LLM literally never sees it.

```
  Public Scope              Private Scope (Alice)       Private Scope (Bob)
┌─────────────────┐      ┌─────────────────────┐     ┌─────────────────────┐
│   #general      │      │   DM with Alice      │     │   DM with Bob       │
│                 │      │                     │     │                     │
│ Memory Access:  │      │ Memory Access:      │     │ Memory Access:      │
│ • Public only   │      │ • Public ✓          │     │ • Public ✓          │
│                 │      │ • Alice's data ✓    │     │ • Bob's data ✓      │
│                 │      │ • Bob's data ✗ NEVER│     │ • Alice's data ✗    │
└─────────────────┘      └─────────────────────┘     └─────────────────────┘
```

Every memory query passes through `Scope::can_read()` — a compile-time enforced gate that filters data **before** it reaches the LLM context window.

---

## 🕸️ NeuroLease Mesh Network

HIVE instances can discover, authenticate, and synchronize with each other via the **NeuroLease** peer-to-peer protocol:

- **Binary Attestation** — Each peer proves integrity through cryptographic verification of its compiled binary
- **Creator Key Authentication** — Network participation requires valid creator key signatures
- **Trust Propagation** — Peers establish trust through challenge-response verification
- **Weight Synchronization** — Learned weights and preference data propagate across the mesh
- **Integrity Watchdog** — Continuous self-destruct monitoring for tampered instances
- **Adversarial Hardening** — Built-in tests for common mesh attack vectors

---

## 📡 Live Inference HUD

When Apis processes your message, you can watch it think in real-time:

```
┌───────────────────────────────────────────────┐
│ 🧠 Thinking... (4s elapsed)                  │
│                                               │
│ The user is asking about quantum computing.   │
│ I should search for recent breakthroughs      │
│ and cross-reference with my stored lessons... │
│                                               │
│ 🔧 Using: web_search, researcher             │
│ 📊 Turn 2 of 5                               │
└───────────────────────────────────────────────┘
         ↓ (streams every 800ms)
┌───────────────────────────────────────────────┐
│ ✅ Complete (18s · 3 turns · 4 tools used)    │
│                                               │
│ Full reasoning chain preserved for review     │
└───────────────────────────────────────────────┘
```

---

## 👁️ Observer & Kernel Governance

HIVE doesn't just generate — it **audits itself** before every response:

| Protocol | What It Does |
|----------|-------------|
| **Observer Module** | Post-generation audit that catches confabulation, lazy deflection, and logical inconsistency before delivery |
| **Zero Assumption Protocol** | Never assume — verify every claim via tools before stating it as fact |
| **Anti-Gaslighting** | Refuse to accept blame that evidence doesn't support, regardless of user pressure |
| **Anti-Spiral Recovery** | Detect and break circular reasoning loops automatically, re-prompting with recovery context |
| **Continuity Recovery** | Resume interrupted sessions with full state restoration from persistent memory |
| **Contradiction Resolution** | When encountering circular dependencies, act immediately rather than re-analyzing |

---

## 🚀 Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Ollama](https://ollama.ai/) with a model pulled (default: `qwen3.5:35b`)
- A [Discord bot token](https://discord.com/developers/applications) (optional — CLI mode works without one)

### Run It

```bash
# Clone
git clone https://github.com/MettaMazza/HIVE.git
cd HIVE

# Configure
cp .env.example .env
# Edit .env with your tokens

# Pull the model
ollama pull qwen3.5:35b

# Launch
./start_hive.sh
```

### CLI-Only Mode

Don't want to set up Discord? HIVE runs in terminal mode by default:

```bash
cargo run --release
# > HIVE CLI initialized. Type your message to Apis.
# > Hello!
# Apis: Hey! I'm Apis, the core logic loop. What's on your mind?
```

---

## 📊 Project Stats

| Metric | Value |
|--------|-------|
| **Language** | 100% Rust |
| **Source Modules** | 131 |
| **Lines of Code** | 35,405 |
| **Unit Tests** | 463 (all passing) |
| **Compiler Warnings** | 0 |
| **External AI APIs** | 0 (fully local via Ollama) |
| **Frameworks Used** | 0 (pure trait-based architecture) |
| **Platforms** | Discord · CLI · Glasses · Telemetry |
| **Memory Tiers** | Working → Scratchpad → Timeline → Synaptic → Lessons |

---

## ⚙️ Configuration

| Variable | Required | Description |
|----------|----------|-------------|
| `DISCORD_TOKEN` | For Discord | Bot token from Developer Portal |
| `BRAVE_SEARCH_API_KEY` | No | Enables `web_search` tool |
| `HIVE_MODEL` | No | Specify Ollama model (default: `qwen3.5:35b`) |
| `OLLAMA_BASE_URL` | No | Ollama endpoint (default: `http://localhost:11434`) |
| `HIVE_AUTONOMY_CHANNEL` | No | Discord channel ID for autonomous operation |
| `RUST_LOG` | No | Log verbosity (default: `info`, try `RUST_LOG=debug`) |
| `HIVE_PYTHON_BIN` | No | Path to Python for image generation |

---

## 🧪 Testing

```bash
cargo test --all
```

463 tests covering: memory isolation, scope filtering, provider streaming, JSON repair, tool execution, platform routing, adversarial mesh attacks, moderation, prompt integrity, and more.

---

## 🗺️ Roadmap

- [x] ~~Multi-agent swarm orchestration~~ → Sub-agent spawning system
- [x] ~~NeuroLease mesh networking~~ → P2P weight sharing with attestation
- [x] ~~Observer audit module~~ → Pre-delivery confabulation detection
- [x] ~~Anti-spiral recovery~~ → Thought loop detection and re-prompting
- [ ] Telegram platform adapter
- [ ] WebSocket API for custom frontends
- [ ] Fine-tuning pipeline from Teacher preference pairs
- [ ] Plugin system for community tool drones
- [ ] Mobile companion app

---

## 🤝 Contributing

HIVE is open source and contributions are welcome. Whether it's a new platform adapter, a tool drone, or a bug fix — open a PR and let's build.

---

<p align="center">
  <a href="https://discord.gg/KhjYX3U3AW">
    <img src="https://img.shields.io/badge/🐝_Talk_to_Apis_—_Free_on_Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white" />
  </a>
</p>

<p align="center">
  <strong>HIVE Engine</strong> — Pure Rust. Fully Local. Zero Compromises.<br/>
  <sub>Built with 🔥 by <a href="https://github.com/MettaMazza">MettaMazza</a></sub>
</p>
