# AUXLOCLAW

<div align="center">

**Ultra-High-Performance AI Agent Framework**

*Beats Hermes Agent and Nanobot in speed, efficiency, and capabilities*

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)]()

</div>

---

## Why AUXLOCLAW?

| Metric | Hermes | Nanobot | **AUXLOCLAW** |
|--------|--------|---------|---------------|
| Language | Python | Python | **Rust** |
| Startup | 5-10s | 1-2s | **12ms** |
| Binary | ~500MB | ~50MB | **5.1MB** |
| Tools | Sequential | Sequential | **Parallel DAG** |
| Memory | File-based | SQLite | **3-Tier** |

---

## Features

- **Rust Native** - Zero-cost abstractions, no GC pauses, maximum performance
- **DAG Tool Execution** - Independent tools run in parallel
- **3-Tier Memory** - LRU cache + SQLite + Vector search
- **Multi-Provider** - NVIDIA, OpenAI, Anthropic, OpenRouter, Groq, and more
- **Skill System** - Compatible with [agentskills.io](https://agentskills.io)
- **Zero-Copy Streaming** - Direct SSE passthrough for real-time responses
- **Multi-Channel** - Telegram gateway, Discord gateway, and HTTP API with optional bearer auth

---

## Installation

### From Source

```bash
git clone https://github.com/larsontrey720/auxloclaw.git
cd auxloclaw
/root/.cargo/bin/cargo build --release
cp target/release/auxloclaw /usr/local/bin/auxloclaw
chmod +x /usr/local/bin/auxloclaw
auxloclaw --version
```

*Note: This workspace currently uses the newer Cargo at `/root/.cargo/bin/cargo` because the system Cargo is too old for lockfile v4.*

### Latest Verified Build

- Commit `b86147f` was built and installed to `/usr/local/bin/auxloclaw`.
- Verification command: `/usr/local/bin/auxloclaw --version` returns `auxloclaw 0.1.0`.
- Tests passed with `/root/.cargo/bin/cargo test --all --no-fail-fast`.

### Quick Setup

```bash
# Interactive setup wizard
auxloclaw setup

# Or quick setup with defaults
auxloclaw setup --quick

# Set your API key
export NVIDIA_API_KEY=your-key-here
```

---

## Usage

### Chat

```bash
# One-shot
auxloclaw chat "What is 2+2?"

# Interactive REPL
auxloclaw chat

# With specific model
auxloclaw chat --model gpt-4 "Hello"
```

### Gateway Server

```bash
# Start gateway (default port 18789)
auxloclaw gateway

# Custom port
auxloclaw gateway --port 8080
```

**Authentication**

- Auth is off by default.
- Set `AUXLOCLAW_REQUIRE_AUTH=true` to require bearer auth on all API routes except `/health`.
- Set `AUXLOCLAW_API_KEY=<secret>` and call with `Authorization: Bearer <secret>`.
- Use secrets/env vars rather than hardcoding tokens.

**API Endpoints:**

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/chat` | POST | Chat with AUXLOCLAW |
| `/stream` | POST | Streaming chat |
| `/skills` | GET | List installed skills |
| `/tools` | GET | List available tools |
| `/api/reflect` | GET | Reflect current state |
| `/api/reflections` | GET | List reflections |
| `/api/sessions/:session_id/history` | GET | Session history |

### Skills

```bash
# List skills
auxloclaw skill list

# View skill details
auxloclaw skill view code-review

# Create new skill
auxloclaw skill create my-skill

# Install skill from directory
auxloclaw skill install ./path/to/skill
```

### Providers

```bash
# List providers
auxloclaw provider list

# Test connection
auxloclaw provider test nvidia
```

### Status

```bash
auxloclaw status
```

---

## Configuration

Config file: `~/.auxloclaw/config.toml`

```toml
[agent]
name = "AUXLOCLAW"
default_model = "stepfun-ai/step-3.5-flash"
temperature = 1.0
max_tokens = 8192

[providers.primary]
name = "nvidia"
api_base = "https://integrate.api.nvidia.com/v1"
# api_key via NVIDIA_API_KEY env var

[memory]
database_path = "~/.auxloclaw/memory.db"
hot_cache_size = 1000

[channels.telegram]
enabled = false
# token via TELEGRAM_BOT_TOKEN env var

[channels.discord]
enabled = false
# token via DISCORD_BOT_TOKEN env var

[server]
port = 18789
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `NVIDIA_API_KEY` | NVIDIA API key |
| `OPENAI_API_KEY` | OpenAI API key |
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `TELEGRAM_BOT_TOKEN` | Telegram bot token |
| `DISCORD_BOT_TOKEN` | Discord bot token |
| `AUXLOCLAW_REQUIRE_AUTH` | Require bearer auth on API routes |
| `AUXLOCLAW_API_KEY` | API key for bearer auth |

### Context Pruning

AUXLOCLAW now limits provider request context to recent conversation turns while keeping older history persisted on disk.

Defaults:

```toml
[agent]
recent_history_turns = 10
context_window_tokens = 20000
tool_output_max_chars = 4000
```

Behavior:

- Keeps the last `recent_history_turns` conversational turns in active model context.
- Compresses older history into a short summary marker instead of replaying every prior message.
- Truncates large tool outputs before feeding them back to the model.
- Uses `context_window_tokens` as a final estimated budget guard.
- Still persists full session history in memory storage for recall, reflection, checkpoints, and rollback.

### Capability Registry and Self-Awareness

AUXLOCLAW now has a runtime capability registry that the agent injects into its own system prompt so it knows what it can do during normal chat sessions.

```bash
auxloclaw capabilities
auxloclaw capabilities --json
curl http://localhost:18789/api/capabilities
```

The manifest includes built-in features, registered tools, MCP status, skills/taps, scheduler, plugin hooks, checkpoints, planner DAG, and run database state.

### Planner DAG and Run Database

AUXLOCLAW includes a structured task planner, DAG executor, and SQLite run database for auditable autonomous execution.

```bash
# Create an editable plan skeleton
auxloclaw plan "Fix failing auth tests" --output auth-plan.json

# Execute JSON/YAML plan and record run state
auxloclaw run-plan auth-plan.json

# Inspect persisted runs
auxloclaw runs list
auxloclaw runs show <run-id>
auxloclaw runs export <run-id> --output run.json
```

Plan schema:

```json
{
  "goal": "Fix failing auth tests",
  "strategy": "inspect, patch, verify",
  "steps": [
    {
      "id": "verify",
      "description": "Run tests",
      "tool": "execute_code",
      "args": { "language": "shell", "code": "cargo test --all" },
      "depends_on": [],
      "retries": 1
    }
  ]
}
```

Run history is stored at `~/.auxloclaw/runs.db` by default and records runs, events, plan steps, results, errors, timestamps, and metadata.

### Skills Hub / Taps

AUXLOCLAW can merge skills from multiple registry manifests, called taps. The official Auxlo registry is enabled by default and custom taps live in `~/.config/auxloclaw/skill-taps.json`.

```bash
auxloclaw skill tap list
auxloclaw skill tap add community https://example.com/manifest.json --priority 10
auxloclaw skill tap add pinned https://example.com/manifest.json --sha256 <manifest-sha256>
auxloclaw skill tap remove community
auxloclaw skill search debugging
auxloclaw skill browse
```

Higher-priority taps win when two taps publish the same skill name. Optional `sha256` pins protect tap manifests from unexpected changes.

### Plugin Hooks

AUXLOCLAW can run external plugin commands on lifecycle events. Plugins receive one JSON object on stdin and may return a JSON object on stdout to rewrite messages, rewrite tool args, or cancel tools.

```toml
[plugins]
enabled = true
timeout_secs = 10

[[plugins.plugins]]
name = "audit-log"
enabled = true
command = "python3"
args = ["/home/workspace/auxloclaw/plugins/audit.py"]
hooks = ["startup", "before_message", "after_message", "before_tool", "after_tool"]
timeout_secs = 5
```

Supported hooks: `startup`, `before_message`, `after_message`, `before_tool`, `after_tool`, `shutdown`.

Hook output examples:

```json
{"message":"rewritten user message"}
{"response":"rewritten assistant response"}
{"tool_args":{"code":"echo safe"}}
{"cancel":true,"error":"blocked by plugin"}
```

### Cron Scheduler

AUXLOCLAW can run autonomous recurring jobs inside the gateway process.

```toml
[scheduler]
enabled = true

[[scheduler.jobs]]
name = "daily-summary"
cron = "0 0 9 * * *"
prompt = "Review active sessions and produce a daily summary."
session_id = "scheduler:daily-summary"
enabled = true
run_on_startup = false
timeout_secs = 300
```

Cron expressions use the six-field seconds format: `sec min hour day month weekday`. Scheduled prompts execute through `AgentCore::process`, so they share memory, tools, MCP tools, and approval policy with normal gateway requests.

### MCP Client

AUXLOCLAW can connect to stdio MCP servers and expose their tools as local tools. MCP tools are registered as `mcp_<server_or_prefix>_<tool>` and pass through the same approval policy as built-in tools.

```toml
[mcp]
enabled = true

[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/workspace"]
tool_prefix = "fs"
timeout_secs = 30
include_tools = []
exclude_tools = []

[mcp.servers.env]
# Optional env vars for the server
```

Server fields:

| Field | Description |
|-------|-------------|
| `name` | Unique server name |
| `command` | Stdio MCP server command |
| `args` | Command arguments |
| `env` | Optional environment variables |
| `tool_prefix` | Optional local tool prefix override |
| `include_tools` | Optional allowlist of remote tool names |
| `exclude_tools` | Optional denylist of remote tool names |
| `timeout_secs` | Per-request timeout |

---

## Tool Approval Policy

- `AUXLOCLAW_APPROVAL_MODE=smart|manual|off`
- default is smart
- smart blocks critical destructive patterns, requires approval for high-risk shell/network commands, and blocks private/local URLs for URL-capable tools to reduce SSRF risk
- blocked tool calls return structured JSON with `requires_approval`, `risk`, and `reason`

---

## Skill Development

Skills are markdown-based instruction sets compatible with [agentskills.io](https://agentskills.io/specification).

### Structure

```
skill-name/
├── SKILL.md          # Required: metadata + instructions
├── scripts/          # Optional: executable code
├── references/       # Optional: documentation
└── assets/           # Optional: templates, resources
```

### SKILL.md Format

```markdown
---
name: my-skill
description: What this skill does and when to use it.
allowed-tools: Bash(python:*) Read
---

# My Skill

Instructions for the AI agent...
```

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      AgentCore                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐    │
│  │  Provider   │  │   Memory    │  │ Orchestrator│    │
│  │    Pool     │  │   Engine    │  │   (DAG)     │    │
│  │ ┌─────────┐ │  │ ┌─────────┐ │  │ ┌─────────┐ │    │
│  │ │ Primary │ │  │ │   LRU   │ │  │ │ Tools   │ │    │
│  │ │Fallbacks│ │  │ │ SQLite  │ │  │ │ (para.) │ │    │
│  │ └─────────┘ │  │ │ Vector  │ │  │ └─────────┘ │    │
│  └─────────────┘  │ └─────────┘ │  └─────────────┘    │
│                   └─────────────┘                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐    │
│  │   Skills    │  │  Channels   │  │  Streaming  │    │
│  │  Registry   │  │ TG/Discord  │  │  (SSE)      │    │
│  └─────────────┘  └─────────────┘  └─────────────┘    │
└─────────────────────────────────────────────────────────┘
```

---

## Performance

Benchmarks on 3-core, 14GB RAM:

| Metric | Value |
|--------|-------|
| Binary Size | 5.1 MB |
| Startup Time | 12ms |
| Chat Latency | <100ms |
| First Stream Token | <200ms |
| Memory (idle) | ~10MB |

---

## Comparison with Hermes Agent

### AUXLOCLAW Advantages

- **1000x faster startup** (12ms vs 5-10s)
- **100x smaller binary** (5MB vs 500MB)
- **Parallel tool execution** vs sequential
- **3-tier memory** vs file-based
- **Zero-copy streaming** vs buffered

### Hermes Advantages (for now)

- 118 bundled skills vs 3 starter skills
- Skill auto-install from natural language
- Voice transcription
- More channel integrations
- RL training environments

---

## Roadmap

- [ ] Discord full integration
- [ ] WhatsApp/Signal channels
- [ ] Skill auto-install from NL prompt
- [ ] Voice transcription
- [ ] Web UI dashboard
- [ ] MCP server support

---

## Contributing

Contributions welcome! Please read our contributing guidelines.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing`)
3. Commit changes (`git commit -m 'Add amazing feature'`)
4. Push to branch (`git push origin feature/amazing`)
5. Open a Pull Request

---

## License

MIT License - see [LICENSE](LICENSE) file.

---

<div align="center">

**Built with love by [Auxlo](https://auxlo.xyz)**

</div>