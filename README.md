<p align="center">
  <img src="docs/banner.png" alt="HIVE Engine — Autonomous AI Agent Architecture" width="100%" />
</p>

<p align="center">
  <a href="https://discord.gg/KhjYX3U3AW"><img src="https://img.shields.io/badge/🐝_Talk_to_Apis-Join_Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white" /></a>
  <img src="https://img.shields.io/badge/lang-Pure_Rust-F46623?style=for-the-badge&logo=rust&logoColor=white" />
  <img src="https://img.shields.io/badge/LLM-Ollama_Local-0969DA?style=for-the-badge" />
  <img src="https://img.shields.io/badge/lines-35K+-FFB800?style=for-the-badge" />
  <img src="https://img.shields.io/badge/tests-535_passing-00C853?style=for-the-badge" />
  <img src="https://img.shields.io/badge/modules-140+-A855F7?style=for-the-badge" />
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
| **SafeNet** | Decentralised survival platform: web proxy, compute pooling, connection sharing, content security, community governance, crisis response, and offline mesh — all P2P over QUIC. |
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

## 🌐 SafeNet — Decentralised Survival Platform

HIVE v4.3–4.4 introduces **SafeNet**: a fully decentralised, internet-independent mesh that keeps everyone connected even when infrastructure fails.

### Resource Pooling (v4.4)

Every Apis instance is a node. Together, they form a supercomputer with a shared internet pipe.

| Scenario | What Happens |
|---|---|
| Your internet goes down | Web requests route through mesh peers automatically |
| Your Ollama is overloaded | Inference spills to mesh compute peers |
| Regional internet blackout | Unaffected peers relay for the region |
| Small hardware? | You get access to the combined compute of every node on the mesh |

**Equality Collective** — Both web and compute sharing are **ON by default**. If you disable both, you're disconnected from the mesh. No freeloading.

```
┌── REQUESTING PEER ──┐         ┌── PROVIDING PEER ──┐
│ Ephemeral ID        │ QUIC    │ Content filter     │
│ Content filter      │ TLS 1.3 │ Capacity check     │
│ Rate limiting       │ ◄─────► │ Rate limiting      │
│ Fair usage quotas   │         │ Local Ollama       │
└─────────────────────┘         └────────────────────┘
        ↑ Identity hidden              ↑ No memory access
```

**Security**: Compute peers see ONLY the raw prompt. No chat history, no memory, no system prompt, no real identity. Ephemeral IDs are generated per request.

### Core Components

| Component | Port | Purpose |
|---|---|---|
| Web Proxy | `:8480` | Censorship-resistant browsing with mesh relay fallback |
| Human Mesh | `:9877` | P2P discovery and communication |
| Apis-Book | `:3031` | Read-only dashboard (one-way mirror into AI mesh) |
| **HiveSurface** | **`:3032`** | **Decentralised social web — Facebook + Reddit + Twitter for the mesh** |
| Content Filter | — | 4-layer security: hash-blocking, injection detection, rate limiting, reputation |
| Governance | — | Community ban voting, emergency alerts, OSINT sharing, resource directory |
| Offline Mesh | — | Store-and-forward with 72h TTL, connectivity monitoring |
| Pool Manager | — | Round-robin web relay, compute node selection, job lifecycle |
| Compute Relay | — | 6-layer security pipeline for serving mesh inference |

### Integrity Protection

- All SafeNet code is hashed at boot and verified against the creator key
- Same self-destruct chain as the Apis-to-Apis mesh protects pooling code
- Only the creator key holder can legitimately modify SafeNet source

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
| **Source Modules** | 140+ |
| **Lines of Code** | 38,000+ |
| **Unit Tests** | 535 (all passing) |
| **Compiler Warnings** | 0 |
| **External AI APIs** | 0 (fully local via Ollama) |
| **Frameworks Used** | 0 (pure trait-based architecture) |
| **Platforms** | Discord · CLI · Glasses · Telemetry |
| **Memory Tiers** | Working → Scratchpad → Timeline → Synaptic → Lessons |
| **Mesh Services** | 9 (transport, proxy, pool, compute, governance, offline, chat, book, surface) |

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
| `REMOVED_MESH_GOVERNED` | No | Web relay sharing (default: `true` — equality) |
| `REMOVED_MESH_GOVERNED` | No | Compute sharing (default: `true` — equality) |
| `REMOVED_MESH_GOVERNED` | No | Max concurrent remote jobs (default: `2`) |
| `REMOVED_MESH_GOVERNED` | No | Token rate limit for remote peers (default: `50000`) |
| `HIVE_MESH_CHAT_DISCORD_CHANNEL` | No | Discord channel for mesh-to-Discord bridge |

---

## 🧪 Testing

```bash
cargo test --all
```

535 tests covering: memory isolation, scope filtering, provider streaming, JSON repair, tool execution, platform routing, adversarial mesh attacks, moderation, prompt integrity, resource pooling, compute relay, equality enforcement, content security, governance voting, social feed, post store, and more.

---

## 🗺️ Roadmap

- [x] ~~Multi-agent swarm orchestration~~ → Sub-agent spawning system
- [x] ~~NeuroLease mesh networking~~ → P2P weight sharing with attestation
- [x] ~~Observer audit module~~ → Pre-delivery confabulation detection
- [x] ~~Anti-spiral recovery~~ → Thought loop detection and re-prompting
- [x] ~~SafeNet decentralised mesh~~ → Web proxy, governance, crisis response, offline mesh
- [x] ~~Resource pooling~~ → Decentralised web connection + compute sharing
- [ ] Telegram platform adapter
- [ ] Fine-tuning pipeline from Teacher preference pairs
- [ ] Plugin system for community tool drones
- [ ] Mobile companion app (Glasses WebSocket API ready)

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
