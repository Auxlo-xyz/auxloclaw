# AUXLOCLAW

<div align="center">

**Ultra-High-Performance AI Agent Framework**

[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange.svg)](https://www.rust-lang.org/)
[![Crates.io](https://img.shields.io/crates/v/auxloclaw.svg)](https://crates.io/crates/auxloclaw)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

</div>

---

## Install

```bash
cargo binstall auxloclaw
```

Requires `cargo-binstall`. Install it at https://github.com/cargo-bins/cargo-binstall.

```bash
curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
```

---

## Quick Start

```bash
# Interactive setup wizard
auxloclaw setup

# Set your API key
export NVIDIA_API_KEY=your-key-here

# Chat
auxloclaw chat "Hello"

# Start the gateway server
auxloclaw gateway
```

---

## Features

- **Rust native** -- zero-cost abstractions, no GC pauses
- **DAG tool execution** -- independent tools run in parallel
- **3-tier memory** -- LRU cache + SQLite + vector search
- **Multi-provider** -- NVIDIA, OpenAI, Anthropic, OpenRouter, Groq, and more
- **Skill system** -- compatible with [agentskills.io](https://agentskills.io)
- **Zero-copy streaming** -- direct SSE passthrough for real-time responses
- **Multi-channel** -- Telegram, Discord, and HTTP API with optional bearer auth
- **MCP client** -- connect to stdio MCP servers and use their tools
- **Planner DAG** -- structured task planning with auditable run database
- **Plugin hooks** -- lifecycle event system for external plugins
- **Cron scheduler** -- autonomous recurring jobs inside the gateway
- **Context pruning** -- smart conversation history management
- **Persona system** -- customizable agent personality and behavior
- **Code mode** -- isolated coding agent with its own workspace

---

## CLI Commands

| Command | Description |
|---------|-------------|
| `auxloclaw gateway` | Start the gateway server (default port 18789) |
| `auxloclaw chat [message]` | Chat with the agent (interactive REPL or one-shot) |
| `auxloclaw setup` | Interactive setup wizard |
| `auxloclaw status` | Show system status |
| `auxloclaw code [task]` | Start a coding session in an isolated workspace |
| `auxloclaw model [id]` | Override model/provider settings for your session |
| `auxloclaw skill <sub>` | Manage skills (list, view, create, install, search, browse) |
| `auxloclaw provider <sub>` | Manage providers (list, test) |
| `auxloclaw persona <sub>` | Manage persona (show, set, list) |
| `auxloclaw config <sub>` | Manage configuration (show, set, get, edit) |
| `auxloclaw mcp <sub>` | Manage MCP servers (list, add, remove, enable, disable, tools) |
| `auxloclaw capabilities` | Show runtime capability manifest |
| `auxloclaw plan <goal>` | Create a structured task plan from a goal |
| `auxloclaw run-plan <file>` | Execute a structured task plan DAG |
| `auxloclaw runs <sub>` | Inspect persistent run history (list, show, export) |
| `auxloclaw run <skill>` | Run a skill |
| `auxloclaw update` | Self-update to latest version |

### In-Chat Commands

| Command | Description |
|---------|-------------|
| `/token` | Set/list/remove/get/forget API keys |
| `/mcp` | Add/remove/list/enable/disable MCP servers |
| `/model` | Switch LLM model per session |
| `/code` | Enter coding agent mode |
| `/normal` | Exit coding mode, return to normal chat |
| `/memory` | View agent memory |
| `/stop` | Stop current agent operation |

All commands work in Telegram, Discord, and CLI.

---

## Gateway API

```bash
auxloclaw gateway --port 8080
```

**Authentication**: Off by default. Set `AUXLOCLAW_REQUIRE_AUTH=true` and `AUXLOCLAW_API_KEY=<secret>` to require bearer auth on all routes except `/health`.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/chat` | POST | Chat with AUXLOCLAW |
| `/stream` | POST | Streaming chat (SSE) |
| `/skills` | GET | List installed skills |
| `/tools` | GET | List available tools |
| `/api/capabilities` | GET | Runtime capability manifest |
| `/api/reflect` | GET | Reflect current state |
| `/api/reflections` | GET | List reflections |
| `/api/sessions/:id/history` | GET | Session history |

---

## Configuration

Config file: `~/.auxloclaw/config.toml`

```toml
[agent]
name = "AUXLOCLAW"
default_model = "stepfun-ai/step-3.5-flash"
temperature = 1.0
max_tokens = 8192
recent_history_turns = 10
context_window_tokens = 20000
tool_output_max_chars = 4000

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
| `OPENROUTER_API_KEY` | OpenRouter API key |
| `GROQ_API_KEY` | Groq API key |
| `TELEGRAM_BOT_TOKEN` | Telegram bot token |
| `DISCORD_BOT_TOKEN` | Discord bot token |
| `AUXLOCLAW_REQUIRE_AUTH` | Require bearer auth on API routes |
| `AUXLOCLAW_API_KEY` | API key for bearer auth |

---

## MCP Client

Connect to stdio MCP servers and use their tools natively.

```toml
[mcp]
enabled = true

[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/workspace"]
tool_prefix = "fs"
timeout_secs = 30
```

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

## Skills Hub / Taps

Merge skills from multiple registry manifests. The official Auxlo registry is enabled by default.

```bash
auxloclaw skill tap list
auxloclaw skill tap add community https://example.com/manifest.json --priority 10
auxloclaw skill tap add pinned https://example.com/manifest.json --sha256 <hash>
auxloclaw skill search debugging
auxloclaw skill browse
```

---

## Plugin Hooks

Run external plugin commands on lifecycle events. Plugins receive JSON on stdin and may return JSON on stdout to rewrite messages, rewrite tool args, or cancel tools.

```toml
[plugins]
enabled = true
timeout_secs = 10

[[plugins.plugins]]
name = "audit-log"
enabled = true
command = "python3"
args = ["/path/to/audit.py"]
hooks = ["startup", "before_message", "after_message", "before_tool", "after_tool"]
timeout_secs = 5
```

Supported hooks: `startup`, `before_message`, `after_message`, `before_tool`, `after_tool`, `shutdown`.

---

## Cron Scheduler

Run autonomous recurring jobs inside the gateway process.

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

Cron expressions use six-field seconds format: `sec min hour day month weekday`.

---

## Planner DAG

Structured task planning with auditable execution.

```bash
auxloclaw plan "Fix failing auth tests" --output auth-plan.json
auxloclaw run-plan auth-plan.json
auxloclaw runs list
auxloclaw runs show <run-id>
```

---

## Tool Approval Policy

- `AUXLOCLAW_APPROVAL_MODE=smart|manual|off` (default: `smart`)
- Smart mode blocks critical destructive patterns, requires approval for high-risk shell/network commands, and blocks private/local URLs to reduce SSRF risk.

---

## Skill Development

Skills are markdown-based instruction sets compatible with [agentskills.io](https://agentskills.io/specification).

```
skill-name/
├── SKILL.md          # Required: metadata + instructions
├── scripts/          # Optional: executable code
├── references/       # Optional: documentation
└── assets/           # Optional: templates, resources
```

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
│  └─────────────┘ │ └─────────┘ │  └─────────────┘    │
│                   └─────────────┘                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐    │
│  │   Skills    │  │  Channels   │  │  Streaming  │    │
│  │  Registry   │  │ TG/Discord  │  │  (SSE)      │    │
│  └─────────────┘  └─────────────┘  └─────────────┘    │
└─────────────────────────────────────────────────────────┘
```

---

## Performance

| Metric | Value |
|--------|-------|
| Binary Size | ~21 MB |
| Startup Time | ~12ms |
| Chat Latency | <100ms |
| First Stream Token | <200ms |
| Tests | 71 passing |
| Lines of Rust | ~17,700 |

---

## Roadmap

- [ ] More MCP server integrations (filesystem, Brave search, memory, fetch, postgres)
- [ ] Provider-specific request adapters (Anthropic, Gemini, Cohere native formats)
- [ ] Rate limiting and retry with exponential backoff
- [ ] Streaming partial tool call reassembly
- [ ] Accurate token counting (tiktoken-rs integration)
- [ ] Multi-user / multi-session support
- [ ] Web UI dashboard
- [ ] Voice input/output (Whisper STT + TTS)
- [ ] Docker support
- [ ] Webhook support for external service integrations

---

## Contributing

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
