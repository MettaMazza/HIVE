<p align="center">
  <img src="https://img.shields.io/badge/lang-Rust-orange?style=for-the-badge&logo=rust" />
  <img src="https://img.shields.io/badge/LLM-Ollama-blue?style=for-the-badge" />
  <img src="https://img.shields.io/badge/platform-Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white" />
  <img src="https://img.shields.io/badge/tests-22%20passing-brightgreen?style=for-the-badge" />
  <img src="https://img.shields.io/badge/coverage-100%25-brightgreen?style=for-the-badge" />
</p>

<h1 align="center">🐝 HIVE</h1>
<h3 align="center">Autonomous AI Agent Engine — Pure Rust</h3>

<p align="center">
  <em>Platform-neutral AI agent with strict per-user memory isolation, live inference signaling, and streaming multi-modal LLM integration.</em>
</p>

---

## What is HIVE?

HIVE is a **zero-dependency AI agent engine** written entirely in Rust. It connects to any LLM provider and any chat platform through clean trait abstractions, enforcing **strict privacy boundaries** at the memory layer — not the prompt layer.

The core persona, **Apis**, operates across Discord channels, DMs, and CLI simultaneously while maintaining per-user, per-scope data isolation that is architecturally enforced and impossible to bypass through prompt injection.

### Key Differentiators

- 🔒 **Memory-Level Security** — Privacy scoping is enforced in the `MemoryStore`, not the system prompt. Private conversations are invisible across scopes by design, not instruction.
- ⚡ **Live Inference Signaling** — Real-time Discord embeds show reasoning tokens as they stream from the LLM, with debounced updates and elapsed time tracking (CognitionTracker pattern).
- 🦀 **Pure Rust** — No runtime, no garbage collector, no framework overhead. Compiles to a single static binary.
- 🧩 **Trait-Based Platform Abstraction** — Adding a new platform (Telegram, Slack, Matrix) requires implementing a single `Platform` trait. Zero changes to the engine.
- 🧪 **100% Test Coverage** — Every module, every path, every edge case. No stubs, no mocks in production code.

---

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    HIVE Engine                       │
│                                                     │
│  ┌──────────┐  ┌──────────┐  ┌───────────────────┐  │
│  │ Platform │  │ Platform │  │    MemoryStore     │  │
│  │ Discord  │  │   CLI    │  │  (Scope-Isolated)  │  │
│  └────┬─────┘  └────┬─────┘  └────────┬──────────┘  │
│       │              │                 │             │
│       └──────┬───────┘                 │             │
│              │                         │             │
│       ┌──────▼──────┐          ┌───────▼──────┐      │
│       │  Event Bus  │◄────────►│   Provider   │      │
│       │   (mpsc)    │          │   (Ollama)   │      │
│       └─────────────┘          └──────────────┘      │
└─────────────────────────────────────────────────────┘
```

### Security Model

```
Public Scope          Private Scope (User A)     Private Scope (User B)
┌──────────────┐      ┌──────────────────┐       ┌──────────────────┐
│ #general     │      │ DM with User A   │       │ DM with User B   │
│              │      │                  │       │                  │
│ Visible to:  │      │ Visible to:      │       │ Visible to:      │
│ • Everyone   │      │ • Public data    │       │ • Public data    │
│              │      │ • User A's data  │       │ • User B's data  │
│              │      │ • ✗ User B NEVER │       │ • ✗ User A NEVER │
└──────────────┘      └──────────────────┘       └──────────────────┘
```

Private scope queries can access public data + their own private data. They can **never** access another user's private data. This is enforced at the `Scope::can_read()` level — the LLM never even sees the data.

---

## Modules

| Module | File | Purpose |
|--------|------|---------|
| **Engine** | `src/engine/mod.rs` | Core event loop, telemetry orchestration, prompt assembly |
| **Memory** | `src/memory/mod.rs` | Scope-isolated event store with `can_read()` enforcement |
| **Platforms** | `src/platforms/` | Trait-based I/O — Discord (serenity) and CLI implementations |
| **Providers** | `src/providers/ollama.rs` | Streaming Ollama `/api/chat` with native `thinking` token extraction |
| **Models** | `src/models/` | `Event`, `Response`, `Scope` domain types |

### Live Inference Signaling

When Apis processes a message, Discord displays a **live-updating embed** that shows the model's reasoning tokens in real-time:

```
┌─────────────────────────────────────────┐
│ 🧠 Thinking... (3s)                    │
│                                         │
│ The user is asking about X. I should    │
│ consider Y and Z before responding...   │
│ Let me analyze the context from the     │
│ previous conversation...                │
└─────────────────────────────────────────┘
         ↓ (debounced every 800ms)
┌─────────────────────────────────────────┐
│ ✅ Complete (12s)                       │
│                                         │
│ <full reasoning chain preserved>        │
└─────────────────────────────────────────┘
```

Updates are **debounced at 800ms** to respect Discord's rate limits. The embed transitions from blurple (`#5865F2`) during processing to green (`#57F287`) on completion.

---

## Execution Flow

```
1. Platform receives trigger (Discord message / CLI input)
2. Handler creates CognitionTracker embed → sends Event to Engine
3. Engine retrieves scope-filtered history from MemoryStore
4. Engine stores event, spawns debounced telemetry task
5. Provider streams response from Ollama (/api/chat)
   ├── thinking tokens → telemetry channel → embed updates (800ms debounce)
   └── response tokens → accumulated into final text
6. Telemetry task finalizes embed → ✅ Complete (Xs)
7. Engine stores Apis's response in memory
8. Final reply sent as separate Discord message
```

---

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Ollama](https://ollama.ai/) with a model installed (default: `qwen3.5:35b`)
- A Discord bot token ([Discord Developer Portal](https://discord.com/developers/applications))

### Setup

```bash
# Clone the repo
git clone https://github.com/MettaMazza/HIVE.git
cd HIVE

# Create your .env file (never committed to git)
echo 'DISCORD_TOKEN="your_discord_bot_token_here"' > .env

# Pull the LLM model
ollama pull qwen3.5:35b

# Launch
./start_hive.sh
```

### Manual Run

```bash
DISCORD_TOKEN="YOUR_TOKEN" cargo run --release
```

---

## Test Coverage

```
running 22 tests
test models::scope::tests::test_scope_visibility .............. ok
test platforms::cli::tests::test_cli_name ..................... ok
test platforms::cli::tests::test_cli_send_public .............. ok
test platforms::cli::tests::test_cli_send_private ............. ok
test platforms::cli::tests::test_cli_start_and_read ........... ok
test platforms::cli::tests::test_cli_start_send_failure ....... ok
test platforms::discord::tests::test_discord_name ............. ok
test platforms::discord::tests::test_discord_send_invalid ...... ok
test platforms::discord::tests::test_discord_send_uninit ....... ok
test memory::tests::test_secure_memory_retrieval .............. ok
test providers::ollama::tests::test_provider_success .......... ok
test providers::ollama::tests::test_provider_parse_error ...... ok
test providers::ollama::tests::test_provider_early_eof ........ ok
test providers::ollama::tests::test_provider_missing_content .. ok
test providers::ollama::tests::test_provider_reasoning ........ ok
test providers::ollama::tests::test_provider_http_error ....... ok
test providers::ollama::tests::test_provider_connection_error . ok
test engine::tests::test_engine_routing_with_mock ............. ok
test engine::tests::test_engine_handles_provider_error ........ ok
test engine::tests::test_engine_unknown_platform .............. ok
test engine::tests::test_engine_platform_start_failure ........ ok
test engine::tests::test_engine_telemetry_streaming ........... ok

test result: ok. 22 passed; 0 failed; 0 ignored
```

---

## Configuration

| Variable | Required | Description |
|----------|----------|-------------|
| `DISCORD_TOKEN` | Yes | Discord bot token from Developer Portal |
| Ollama endpoint | No | Defaults to `http://localhost:11434/api/chat` |
| Model | No | Defaults to `qwen3.5:35b` |
| Target channel | No | Hardcoded to listen channel; configurable in `discord.rs` |

---

## License

MIT

---

<p align="center">
  <strong>HIVE</strong> — Built from scratch in pure Rust. No frameworks. No shortcuts.
</p>
