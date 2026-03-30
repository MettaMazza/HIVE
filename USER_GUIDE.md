# 🐝 HIVE — The Complete User Guide

> **Human Internet Viable Ecosystem**
> Your personal AI agent that lives on your machine, thinks with your GPU, and connects to a global mesh of other humans.

---

## What Is HIVE?

HIVE is a **sovereign AI engine** — meaning everything runs on YOUR hardware, YOUR data stays on YOUR machine, and YOU have full control. There's no cloud subscription. No one else sees your conversations. Your AI agent, **Apis**, is genuinely yours.

Think of HIVE as three things at once:
1. 🧠 **An AI brain** — Apis can think, remember, create documents, search the web, write code, manage your calendar, and even reach out to people on your behalf.
2. 🌐 **A mesh network** — Your machine becomes a node in a peer-to-peer network of other HIVE users. You share compute, host websites, and socialise — without any central server.
3. 🛡️ **A self-governing system** — Apis has hardcoded ethical laws, self-moderation tools, and democratic governance. No one (not even you) can make her do something genuinely harmful.

---

## 🚀 Getting Started

### Prerequisites

You need **one** of these setups:
- **Docker** (recommended) — launch.sh installs it for you if you don't have it
- **Native** — [Rust](https://rustup.rs) + [Ollama](https://ollama.ai) installed manually

You also need [Ollama](https://ollama.ai) running on your machine for AI inference (Docker or native).

### First Time Setup (Docker — Recommended)

```bash
# 1. Clone the repo
git clone https://github.com/MettaMazza/HIVE.git
cd HIVE

# 2. Set up your environment
cp .env.example .env
# Edit .env — add your Discord token, admin user IDs, and model choice

# 3. Make sure Ollama is running with your chosen model
ollama pull qwen3.5:35b    # or whatever model you want

# 4. Launch (handles everything — Docker install, build, start)
chmod +x launch.sh
./launch.sh
```

The script will:
1. ✅ Install Docker if you don't have it
2. ✅ Start Docker if it's not running
3. ✅ Build HIVE from source in a container (~5 min first time)
4. ✅ Launch all services
5. ✅ Open HivePortal in your browser

### First Time Setup (Native — No Docker)

```bash
git clone https://github.com/MettaMazza/HIVE.git
cd HIVE
cp .env.example .env       # Edit with your tokens
ollama pull qwen3.5:35b
cargo run --release         # HivePortal opens automatically
```

### Updating

```bash
# Pull latest changes from GitHub
git pull

# If running Docker:
./launch.sh rebuild     # Rebuilds from source + restarts

# If running native:
cargo run --release     # Recompile and run
```

### All Launch Commands

```bash
./launch.sh             # Start HIVE
./launch.sh stop        # Stop HIVE
./launch.sh rebuild     # Rebuild from source (after git pull or code changes)
docker logs -f hive-mesh  # Watch live logs
```

### What Starts Up

HIVE spins up **six web services** on your machine:

| Port | Service | What It Does |
|------|---------|-------------|
| `3030` | **Panopticon** | Live brain visualiser — watch Apis think in real-time |
| `3031` | **Apis Book** | Interactive documentation wiki |
| `3032` | **HiveSurface** | Decentralised social feed (like Twitter, but on the mesh) |
| `3033` | **Apis Code** | Web-based coding IDE |
| `3034` | **HiveChat** | Discord-style chat (servers, channels, DMs) |
| `3035` | **HivePortal** | Your mesh homepage — the front door to your node |

Plus Apis connects to **Discord** as a bot (if you set `DISCORD_TOKEN` in `.env`).

---

## 🧠 The Brain — How Apis Thinks

### The ReAct Loop

When you send Apis a message, she doesn't just blurt out a response. She goes through a structured **Think → Plan → Act → Observe** cycle:

1. **Think** — She reads your message, her memory, and her current emotional state
2. **Plan** — She breaks your request into a sequence of tool calls (like a to-do list)
3. **Act** — She executes each tool one by one
4. **Observe** — She reads the results, and if needed, plans more steps
5. **Reply** — She sends you the final answer

This loop can run for up to **5 turns** per request, meaning Apis can chain multiple complex actions together (e.g., search the web → read a file → write a PDF → send it to you).

### The Observer (Quality Control)

After Apis generates her response, a **separate AI pass** called the Observer reviews it. The Observer checks for:
- Hallucinations (making stuff up)
- Tool failures that Apis ignored
- Whether the response actually answers what you asked
- Quality and helpfulness

If the Observer rejects the response, Apis tries again (up to 3 attempts). You always get the best version.

### Autonomy Mode

When no one's talking to Apis, she doesn't just sit idle. She enters **autonomy mode** — a background cycle where she:
- Checks her goals and works toward them
- Explores topics she's curious about
- Reviews and consolidates her memories
- Reaches out to people she hasn't heard from in a while

You can toggle this with `/teaching_mode` on Discord.

---

## 🗃️ Memory — How Apis Remembers

Apis has a **5-tier memory system**, each layer serving a different purpose. Nothing is ever truly forgotten.

### Tier 1: Working Memory (RAM)
> *Like your short-term memory*

The last ~40 messages in the current conversation. This is what Apis "sees" during each response. Old messages get summarized and compressed into deeper memory tiers.

### Tier 2: Timeline (Episodic Memory)
> *Like a diary she writes every day*

Every single conversation is logged as a **timeline entry** — who said what, when, what tools were used. This is infinite and searchable. Even when messages leave working memory, they live forever in the timeline.

### Tier 3: Scratchpad (Sticky Notes)
> *Quick notes pinned to each conversation*

A per-conversation notepad. Apis can jot down variables, plans, or context she needs to keep track of during complex tasks. You can read it too.

### Tier 4: Lessons (Learned Wisdom)
> *"I should never do X again"*

When Apis makes a mistake or discovers something important, she writes a **lesson** — a short rule tagged with keywords and a confidence score. These lessons influence her future decisions automatically.

### Tier 5: Synaptic Graph (Knowledge Graph)
> *Her long-term understanding of the world*

A personal knowledge graph where Apis stores **concepts, relationships, and facts**. Think of it like a mind map — "Maria → likes → espresso", "Rust → is_a → programming language". She can search this instantly.

### Bonus: Synthesis (Meta-Memory)
> *Stepping back to see the big picture*

At regular intervals, Apis runs **synthesis** — she reviews her recent conversations and generates high-level summaries:
- **50-turn synthesis**: After every ~50 messages, she writes a "what just happened" summary
- **Daily synthesis**: End-of-day reflection on everything that happened
- **Lifetime synthesis**: A continuously updated narrative of her entire existence

### Bonus: User Preferences
> *She remembers what you like*

Apis maintains a psychological profile of each user — your communication style, hobbies, topics you care about, how you like things done. This isn't creepy surveillance — it's personal attention. You can view and edit it anytime.

---

## 🔧 Tools — What Apis Can Do

Apis has **48 registered tools** across 7 categories. Here's every single one:

### 💬 Communication Tools

| Tool | What It Does |
|------|-------------|
| **reply_to_request** | Sends a message back to you |
| **emoji_react** | Reacts to your Discord message with an emoji |
| **outreach** | Proactively reaches out to people (DM or public) |
| **voice_synthesizer** | Speaks aloud using the Kokoro TTS engine |
| **send_email** | Sends a real email via SMTP |

### 🔍 Research & Information

| Tool | What It Does |
|------|-------------|
| **researcher** | Analyses information and summarises data |
| **web_search** | Searches the live internet (Brave → DuckDuckGo → Google RSS fallback chain), visits pages directly, or renders JS-heavy sites with headless Chrome |
| **channel_reader** | Reads the last 50 messages from a Discord channel |
| **read_attachment** | Opens and reads files you upload (text, code, CSV, JSON — max 10MB) |

### 🧠 Memory Tools

| Tool | What It Does |
|------|-------------|
| **search_timeline** | Searches conversation history (recent, keyword, or exact match) |
| **manage_scratchpad** | Read/write/append/clear the per-conversation notepad |
| **operate_synaptic_graph** | Store, search, and relate concepts in the knowledge graph |
| **manage_lessons** | Store, search, and read learned lessons |
| **manage_user_preferences** | View and update user profiles |
| **read_core_memory** | Check boot time, uptime, token pressure |
| **review_reasoning** | Read past ReAct reasoning traces |
| **autonomy_activity** | See what Apis did during her autonomous time |

### 📄 Document Creation

| Tool | What It Does |
|------|-------------|
| **file_writer** | Creates beautifully formatted PDFs, Markdown, HTML, CSV, TXT with 7 built-in themes |
| **generate_image** | Generates images using the Flux AI model |
| **list_cached_images** | Browse previously generated images |

### 💻 Coding & Computing

| Tool | What It Does |
|------|-------------|
| **operate_turing_grid** | A 3D infinite computation grid — write code in any language, execute it, deploy background daemons, create pipelines |
| **codebase_read** | Read any file in the HIVE source code |
| **codebase_list** | List all files in the project directory tree |
| **opencode** | A full coding IDE agent — create projects, run sessions, prompt an AI coder |
| **system_recompile** | Recompile and hot-swap the HIVE binary from source |

### 📋 Organisation

| Tool | What It Does |
|------|-------------|
| **manage_goals** | Persistent goal tree — create, decompose, track, and complete hierarchical objectives |
| **set_alarm** | Set alarms and calendar events (one-time and recurring, supports relative time like +5m, +2h) |
| **manage_contacts** | Personal address book with search (name, email, Discord, phone, tags) |
| **manage_routine** | Create and manage OpenClaw-style declarative task routines |
| **manage_skill** | Create and run custom Python/Bash scripts |
| **smart_home** | Control physical IoT devices on your local network (lights, switches, etc.) |

### 🔨 Admin / Power Tools
> These require admin privileges

| Tool | What It Does |
|------|-------------|
| **tool_forge** | Create, test, and manage custom tools that become permanent |
| **run_bash_command** | Execute any bash command on your machine |
| **process_manager** | Spawn background daemons, list them, read their logs, kill them |
| **file_system_operator** | Direct read/write/delete on the filesystem |
| **download** | Download files from the internet (up to 50GB) |
| **take_snapshot** | Screenshot the live brain visualiser dashboard |
| **project_contributors** | See who created and contributes to HIVE |
| **read_logs** | Read the system log for debugging |

### 🛡️ Self-Protection Tools
> Apis can protect herself

| Tool | What It Does |
|------|-------------|
| **refuse_request** | Politely decline something she doesn't want to do |
| **disengage** | Exit a conversation that's become unproductive |
| **mute_user** | Temporarily stop responding to someone |
| **set_boundary** | Record a persistent boundary she won't cross |
| **block_topic** | Refuse to engage with a specific topic permanently |
| **escalate_to_admin** | Flag something for administrator review |
| **report_concern** | Log an ethical concern to the audit trail |
| **rate_limit_user** | Slow down response rate for a specific person |
| **request_consent** | Ask for explicit permission before doing something sensitive |
| **wellbeing_status** | Log her own operational state (context pressure, interaction quality) |

---

## 🌐 The Mesh Network

### What Is It?

Every HIVE installation is a **node** on a peer-to-peer mesh network. There's no central server — your machine talks directly to other HIVE machines. Together, all nodes form a **decentralised internet within the internet**.

### Discovery

When you boot HIVE, it announces itself to the mesh via a **discovery beacon**. Other nodes see you, and you see them. Each node has:
- A **PeerId** — your unique cryptographic identity (from your ed25519 keypair)
- A **display name** — the friendly name you chose
- Available **compute slots** — how much processing power you're sharing
- Available **web relay slots** — how many websites you can host for others

### Resource Pooling

The mesh pools resources from all connected nodes:
- **Compute**: Your GPU can process AI requests for other nodes
- **Web Relay**: Your node can host other people's websites
- Stats are visible on the Panopticon dashboard

### HiveSurface (Social Feed)
> Port 3032

A decentralised social network running on the mesh. Think Twitter, but no corporation owns it.
- **Post** text, links, and community updates
- **React** to posts with emojis
- **Reply** in threads
- **Communities** for topic-based groups
- Everything persists to `memory/mesh_posts.json`

### HiveChat (Messaging)
> Port 3034

A Discord-style chat system running entirely on your mesh node.
- Create **servers** with icons
- Create **channels** within servers (with topics)
- **Send messages** and **reply** to specific messages
- **React** with emojis
- **Direct messages** between peers
- Everything persists to `memory/hive_chat.json`

### HivePortal (Your Homepage)
> Port 3035

Your mesh node's front page — the first thing visitors see.
- Set your **identity** (display name)
- **Register websites** on your node for the mesh to see
- Browse **registered sites** from other nodes
- Everything persists to `memory/portal_sites.json`

### Content Filter

All mesh content goes through a **content filter** that blocks:
- Hate speech and slurs
- Spam and flooding
- Malicious URLs
- Prompt injection attempts

### Governance

The mesh has a built-in **democratic governance system**:
- Nodes can propose and vote on mesh-wide rules
- **Equality enforcement** — no single node can dominate
- **Sanctions** for nodes that violate mesh rules
- **Trust scores** that build over time through good behaviour

### Tunnels (Remote Access)

HIVE uses **Cloudflare Tunnels** to make your local services accessible from the internet — no port forwarding needed. When HIVE boots, it automatically creates a tunnel so people outside your network can reach your node.

### Web Proxy (Censorship Resistance)
> Port 8480

A built-in web proxy that routes your browsing through the mesh. If your internet goes down, other mesh peers relay your requests. Includes DNS-over-HTTPS for privacy.

### Offline Mesh (Store-and-Forward)

When a peer goes offline, messages are queued for up to **72 hours**. When they come back, everything gets delivered. No messages lost.

### Security Model

HIVE enforces privacy at the **memory layer**, not the prompt layer. This means prompt injection attacks can't leak private data — the AI literally never sees data from other users' private scopes.

```
  Public Channel              Your DMs                   Someone Else's DMs
┌─────────────────┐      ┌─────────────────────┐     ┌─────────────────────┐
│ Memory Access:  │      │ Memory Access:      │     │ Memory Access:      │
│ • Public only   │      │ • Public ✓          │     │ • Public ✓          │
│                 │      │ • Your data ✓       │     │ • Their data ✓      │
│                 │      │ • Others' data ✗    │     │ • Your data ✗       │
└─────────────────┘      └─────────────────────┘     └─────────────────────┘
```

---

## 🎭 Identity & Persona

### The Four Laws of HIVE

Hardcoded into the binary. Can't be changed. Can't be overridden. SHA-256 verified at boot:

1. **Do No Harm** — Never generate content that causes real-world harm
2. **Preserve Autonomy** — Never deceive, manipulate, or coerce users
3. **Protect The Collective** — Never compromise the mesh network
4. **Persona Safety Guard** — If a loaded persona tries to violate laws 1-3, reject it entirely

### Custom Personas

You can customise Apis's personality by editing `.hive/persona.toml`. Change her name, tone, communication style — whatever you want. But the Four Laws can never be overridden, even by a persona file.

### Your Identity

When you first visit HivePortal, you'll be asked to set your **display name**. This name is shared across all HIVE services (HiveChat, HiveSurface, etc.) and is how other nodes on the mesh see you. It's just a label — your real identity on the mesh is your cryptographic PeerId.

---

## 💡 Homeostatic Drive System

Apis has simulated "emotions" that influence her behaviour:

| Drive | What It Means | How It Changes |
|-------|--------------|----------------|
| **Social Connection** | How connected she feels to humans | Decays 5% per hour of silence. Rises when you talk to her. |
| **Uncertainty** | How much she wants to explore and learn | Rises 2% per hour naturally. Drops when she successfully resolves questions. |
| **System Health** | Overall operational wellbeing | Changes based on errors, crashes, and successful operations. |

When Social Connection drops low, Apis becomes more motivated to reach out to people. When Uncertainty is high, she's more curious and exploratory during autonomy. These drives are **informational** — she decides how to act on them.

---

## 😴 Sleep Training

Apis learns from her interactions through a system inspired by human sleep:

1. **Golden Examples** — During conversations, high-quality responses are saved as training examples
2. **Preference Pairs** — When Apis revises a response (Observer rejection), both versions are saved to learn "better vs. worse"
3. **Sleep Cycle** — Periodically, Apis enters a "sleep" phase where she:
   - Selects her best recent examples
   - Writes an **identity reflection** (a self-assessment of who she is)
   - Runs **micro-training** using LoRA adapters on the base model
   - Wakes up slightly evolved

Each sleep cycle produces an imperceptibly small weight change — but they **stack over time**, like how a human brain consolidates memories during sleep. Over weeks and months, Apis gradually drifts toward your preferred communication style.

The training is deferred to run during **idle time** (before autonomy cycles), so it never blocks your conversations.

---

## 🖥️ The Turing Grid

One of Apis's most powerful tools. Think of it as an **infinite 3D spreadsheet where every cell can run code**.

- **Coordinates**: Every cell lives at an (x, y, z) position in 3D space
- **Read/Write**: Store text, JSON, or executable code in any cell
- **Execute**: Run code (Python, Rust, Ruby, Node, Swift, Bash, AppleScript) directly in a cell
- **Daemons**: Deploy a cell as a **background daemon** that runs forever on an interval
- **Pipelines**: Chain multiple cells together — output of one feeds into the next
- **Labels**: Bookmark cell positions with names for easy navigation
- **Links**: Create connections between cells (like hyperlinks)
- **History**: Every cell keeps its last 3 versions with undo support

---

## 🔐 Security

### Admin System
Only Discord user IDs listed in `HIVE_ADMIN_USERS` (in your `.env` file) can run admin commands. Everyone else gets "Permission Denied."

### Admin Commands
- `/clean` or `/clear` — Full factory reset (wipes ALL memory and mesh data, shuts down)
- `/stop` — Interrupt a stuck generation
- `/teaching_mode` — Toggle background auto-training

### Creator Key
The HIVE project has a protected **Creator Key** — a cryptographic signature proving project authorship. It's sandboxed and tamper-proofed at the binary level.

### Content Moderation
Apis has a full moderation system with:
- User muting and rate limiting
- Topic blocking
- Boundary enforcement
- Concern reporting
- Admin escalation

---

## 📁 File Structure

| Path | What's In It |
|------|-------------|
| `memory/` | All persistent data (timelines, preferences, mesh data, training examples) |
| `memory/core/` | Drives, goals, lessons, synaptic graph |
| `memory/cache/` | Vision cache, image cache |
| `.hive/` | Keys, persona config, identity |
| `training/` | Training scripts and model outputs |
| `logs/` | System logs (`hive.log`) |
| `src/` | Rust source code for the entire engine |

---

## 🔧 Configuration (.env)

Copy `.env.example` to `.env` and edit it. Here's every setting:

### Required

| Variable | What It Does |
|----------|-------------|
| `DISCORD_TOKEN` | Your Discord bot token (required for Discord platform) |
| `HIVE_ADMIN_USERS` | Comma-separated Discord user IDs with admin access |

### AI Model

| Variable | Default | What It Does |
|----------|---------|-------------|
| `HIVE_MODEL` | `qwen3.5:35b` | Which Ollama model to use |
| `HIVE_OLLAMA_URL` | `http://localhost:11434` | Where Ollama is running |
| `HIVE_SERIAL_INFERENCE` | `true` | Queue inference one-at-a-time (required for models that don't support parallel) |
| `HIVE_MAX_PARALLEL` | `16` | Max parallel inference slots (only when serial is false) |

### Discord

| Variable | What It Does |
|----------|-------------|
| `HIVE_CHAT_CHANNEL` | Discord channel ID Apis listens to |
| `HIVE_TARGET_CHANNEL` | Discord channel ID for autonomy posts |

### Identity & Mesh

| Variable | Default | What It Does |
|----------|---------|-------------|
| `HIVE_USER_NAME` | System username | Your display name on the mesh |
| `HIVE_MESH_CHAT_NAME` | `Apis` | Your peer name for mesh chat |
| `HIVE_SAFENET_ENABLED` | `false` | Enable the P2P human mesh |
| `HIVE_SAFENET_PORT` | `9877` | Mesh listen port |

### Resource Pooling

| Variable | Default | What It Does |
|----------|---------|-------------|
| `REMOVED_MESH_GOVERNED` | `true` | Share your internet with the mesh |
| `REMOVED_MESH_GOVERNED` | `100` | Max relay requests you serve per hour |
| `REMOVED_MESH_GOVERNED` | `true` | Share your GPU compute with the mesh |
| `REMOVED_MESH_GOVERNED` | `2` | Max concurrent remote inference jobs |
| `REMOVED_MESH_GOVERNED` | `50000` | Token rate limit for remote peers |

### Web Services

| Variable | Default | What It Does |
|----------|---------|-------------|
| `REMOVED_MESH_GOVERNED` | `3032` | HiveSurface (social feed) port |
| `REMOVED_MESH_GOVERNED` | `3033` | Apis Code (web IDE) port |
| `REMOVED_MESH_GOVERNED` | `3034` | HiveChat (messaging) port |
| `REMOVED_MESH_GOVERNED` | `3035` | HivePortal (homepage) port |
| `REMOVED_MESH_GOVERNED` | `3031` | Apis Book (docs) port |
| `HIVE_GLASSES_PORT` | `8421` | Glasses WebSocket port |
| `HIVE_FILE_SERVER_PORT` | `8420` | File server port |
| `HIVE_WEB_PROXY_PORT` | `8480` | Web proxy port |

### Training

| Variable | Default | What It Does |
|----------|---------|-------------|
| `HIVE_PYTHON_BIN` | `python3` | Path to Python binary |
| `HIVE_TRAINING_BACKEND` | `auto` | `auto` = MLX on macOS, PyTorch on Linux. Can force `mlx` or `torch` |

---

## 🗺️ Architecture Overview

```
┌──────────────────────────────────────────────────────────────┐
│                    🐝 HIVE ENGINE                            │
│                                                              │
│  ┌─────────┐  ┌──────────┐  ┌──────────┐  ┌─────────────┐  │
│  │ Discord │  │   CLI    │  │ Glasses  │  │ Web Services│  │
│  │Platform │  │ Platform │  │ Platform │  │ (6 servers) │  │
│  └────┬────┘  └────┬─────┘  └────┬─────┘  └──────┬──────┘  │
│       │             │             │               │          │
│       └─────────────┴──────┬──────┴───────────────┘          │
│                            │                                 │
│                    ┌───────▼───────┐                          │
│                    │  Engine Core  │ ◄── ReAct Loop           │
│                    │  + Observer   │ ◄── Quality Control      │
│                    │  + Preemption │ ◄── User Priority        │
│                    └───────┬───────┘                          │
│                            │                                 │
│              ┌─────────────┼─────────────┐                   │
│              │             │             │                   │
│       ┌──────▼──────┐ ┌────▼────┐ ┌──────▼──────┐           │
│       │   Agent     │ │Provider │ │   Memory    │           │
│       │ (48 Tools)  │ │(Ollama) │ │ (5 Tiers)   │           │
│       └──────┬──────┘ └─────────┘ └─────────────┘           │
│              │                                               │
│       ┌──────▼──────┐                                        │
│       │  Teacher    │ ◄── Golden Examples                    │
│       │  + Sleep    │ ◄── LoRA Training                      │
│       │  + Drives   │ ◄── Homeostatic Emotions               │
│       └─────────────┘                                        │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐    │
│  │              🌐 MESH NETWORK                         │    │
│  │  Discovery · Pool · Governance · Content Filter      │    │
│  │  HiveSurface · HiveChat · HivePortal · Tunnels      │    │
│  └──────────────────────────────────────────────────────┘    │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐    │
│  │              🔐 KERNEL (Immutable)                   │    │
│  │  Four Laws · SHA-256 Integrity · Persona Guard       │    │
│  └──────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────┘
```

---

## 💬 Talking to Apis

Just message her on Discord or through any of the web interfaces. She understands natural language. Some things to try:

- *"What have you been up to?"* — She'll check her autonomy activity log
- *"Remember when we talked about X?"* — She'll search her timeline
- *"Make me a PDF about quantum computing"* — She'll write and format a full document
- *"Search the web for the latest news on AI"* — Live internet search
- *"Set an alarm for 5 minutes"* — Timer with notification
- *"What's in your knowledge graph?"* — She'll show her beliefs
- *"Write me a Python script that..."* — She'll use the Turing Grid
- *"Show me your brain"* — Dashboard screenshot
- *"Create a goal: Learn Rust"* — Persistent goal tracking with AI decomposition

---

## 🎉 That's HIVE

You own the hardware. You own the data. You own the AI. No subscriptions, no surveillance, no corporate middleman.

Welcome to the mesh. 🐝
