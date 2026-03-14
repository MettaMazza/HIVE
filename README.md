<p align="center">
  <img src="docs/banner.png" alt="HIVE Engine — Autonomous AI Agent Architecture" width="100%" />
</p>

<p align="center">
  <a href="https://discord.gg/KhjYX3U3AW"><img src="https://img.shields.io/badge/🐝_Talk_to_Apis-Join_Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white" /></a>
  <img src="https://img.shields.io/badge/lang-Rust-F46623?style=for-the-badge&logo=rust&logoColor=white" />
  <img src="https://img.shields.io/badge/LLM-Ollama_Local-0969DA?style=for-the-badge" />
  <img src="https://img.shields.io/badge/lines-15K+-FFB800?style=for-the-badge" />
  <img src="https://img.shields.io/badge/tests-200+_passing-00C853?style=for-the-badge" />
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

Unlike wrapper bots that relay messages to cloud APIs, HIVE is a **real engine**:

- 🧠 **Multi-turn ReAct Loop** — Apis reasons, selects tools, observes results, and iterates autonomously across multiple turns before responding.
- 🔒 **Memory-Level Security** — Per-user data isolation is enforced at the architecture layer, not the prompt layer. Private data is *invisible* to other scopes by design.
- 🛠️ **21 Native Tool Drones** — Web search, code execution, file I/O, image generation, TTS, PDF composition, process management, and more — all running locally.
- 📡 **Live Inference HUD** — Watch Apis think in real-time via streaming Discord embeds that display reasoning tokens as they generate.
- 🎓 **Self-Supervised Learning** — An integrated Teacher module captures preference pairs and golden examples to continuously improve the agent.

> **Want to see it in action?** Apis is live right now. [**Join the Discord**](https://discord.gg/KhjYX3U3AW) and talk to it for free.

---

## 🏗️ Architecture

```
                          ┌──────────────────────────────────────┐
                          │          🐝 HIVE ENGINE              │
                          │                                      │
   ┌──────────┐           │  ┌────────────┐    ┌──────────────┐  │
   │ Discord  │◄─Events──►│  │  ReAct     │◄──►│   Provider   │  │
   │ Platform │           │  │  Loop      │    │  (Ollama)    │  │
   └──────────┘           │  │            │    └──────────────┘  │
                          │  │  Think →   │                      │
   ┌──────────┐           │  │  Act →     │    ┌──────────────┐  │
   │   CLI    │◄─Events──►│  │  Observe → │◄──►│   Memory     │  │
   │ Platform │           │  │  Repeat    │    │   Store      │  │
   └──────────┘           │  └────────────┘    │  (5-Tier)    │  │
                          │        │           └──────────────┘  │
                          │        ▼                             │
                          │  ┌────────────┐    ┌──────────────┐  │
                          │  │  21 Tool   │    │   Teacher    │  │
                          │  │  Drones    │    │  (Self-Sup)  │  │
                          │  └────────────┘    └──────────────┘  │
                          └──────────────────────────────────────┘
```

### The Stack

| Layer | What It Does |
|-------|-------------|
| **Platforms** | Trait-based I/O abstraction. Discord and CLI ship out of the box. Adding Telegram or Slack = one `impl Platform`. |
| **ReAct Loop** | Autonomous multi-turn reasoning engine. Apis selects tools, reads observations, and decides its own next action. |
| **Tool Drones** | 21 native capabilities: `web_search`, `researcher`, `codebase_read`, `operate_turing_grid`, `file_writer`, `process_manager`, `image_generator`, `kokoro_tts`, and more. |
| **Memory Store** | 5-tier persistence: Working Memory → Scratchpad → Timeline → Synaptic Graph → Lessons. All scope-isolated. |
| **Provider** | Local LLM integration via Ollama with streaming token extraction and `<think>` tag parsing. |
| **Teacher** | Captures reasoning traces, evaluates response quality, and generates preference pairs for RLHF-style improvement. |
| **Kernel** | Core laws, identity protocols, and the Zero Assumption Protocol that governs Apis's behavior. |

---

## 🛠️ The 21 Tool Drones

Apis has access to a full arsenal of native capabilities, all running **locally on your machine**:

<table>
<tr>
<td width="50%">

**🌐 Information**
- `web_search` — Brave-powered web search
- `researcher` — Deep analysis of search results
- `codebase_list` / `codebase_read` — Project introspection
- `read_attachment` — Discord CDN file ingestion
- `channel_reader` — Pull conversation history
- `read_logs` — System log inspection

</td>
<td width="50%">

**🧠 Memory & Knowledge**
- `manage_user_preferences` — Per-user preference tracking
- `store_lesson` — Permanent knowledge retention
- `manage_scratchpad` — Session working memory
- `operate_synaptic_graph` — Associative knowledge links
- `review_reasoning` — Introspect own reasoning traces

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

</td>
<td>

**🎨 Multi-Modal**
- `image_generator` — Local Flux image generation
- `kokoro_tts` — Neural text-to-speech (🔊 Speak button on Discord)
- `synthesizer` — Multi-source fan-in compilation
- `manage_routine` / `manage_skill` — Automation & script management
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

## 🚀 Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Ollama](https://ollama.ai/) with a model pulled (default: `qwen3:32b`)
- A [Discord bot token](https://discord.com/developers/applications) (optional — CLI mode works without one)

### Run It

```bash
# Clone
git clone https://github.com/MettaMazza/HIVE.git
cd HIVE

# Configure
echo 'DISCORD_TOKEN="your_token"' > .env
echo 'BRAVE_SEARCH_API_KEY="your_brave_key"' >> .env  # Optional: enables web search

# Pull the model
ollama pull qwen3:32b

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
| **Source Files** | 80 modules |
| **Lines of Code** | 15,044 |
| **Unit Tests** | 200+ (all passing) |
| **Compiler Warnings** | 0 |
| **External AI APIs** | 0 (fully local via Ollama) |
| **Frameworks Used** | 0 (pure trait-based architecture) |

---

## 🧪 Testing

```bash
cargo test --all
```

Every subsystem is independently tested: memory isolation, scope filtering, provider streaming, JSON repair, tool execution, platform routing, and more.

---

## ⚙️ Configuration

| Variable | Required | Description |
|----------|----------|-------------|
| `DISCORD_TOKEN` | For Discord | Bot token from Developer Portal |
| `BRAVE_SEARCH_API_KEY` | No | Enables `web_search` tool |
| `RUST_LOG` | No | Log verbosity (default: `info`, try `RUST_LOG=debug`) |
| `HIVE_PYTHON_BIN` | No | Path to Python for image generation |

---

## 🗺️ Roadmap

- [ ] Telegram platform adapter
- [ ] WebSocket API for custom frontends
- [ ] Multi-agent swarm orchestration
- [ ] Fine-tuning pipeline from Teacher preference pairs
- [ ] Plugin system for community tool drones

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
