# OpenCrab

**High-Performance Terminal AI Orchestration Agent for Software Development**

> A terminal-native AI orchestration agent written in Rust with Ratatui. Inspired by [OpenClaw](https://github.com/anthropics/claude-code).

[![Rust Edition](https://img.shields.io/badge/rust-2024_edition-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-FSL--1.1--MIT-blue.svg)](LICENSE.md)
[![CI](https://github.com/adolfousier/opencrab/actions/workflows/ci.yml/badge.svg)](https://github.com/adolfousier/opencrab/actions/workflows/ci.yml)
[![GitHub Stars](https://img.shields.io/github/stars/adolfousier/opencrab?style=social)](https://github.com/adolfousier/opencrab)

```
   ___                    ___           _
  / _ \ _ __  ___ _ _    / __|_ _ __ _| |__
 | (_) | '_ \/ -_) ' \  | (__| '_/ _` | '_ \
  \___/| .__/\___|_||_|  \___|_| \__,_|_.__/
       |_|
    🦀 Shell Yeah! AI Orchestration at Rust Speed.
```

**Author:** [Adolfo Usier](https://github.com/adolfousier)

---

## Table of Contents

- [Screenshots](#-screenshots)
- [Core Features](#-core-features)
- [Interactive Approval System](#-interactive-approval-system)
- [Supported AI Providers](#-supported-ai-providers)
- [Quick Start](#-quick-start)
- [Authentication Methods](#-authentication-methods)
- [Using Local LLMs](#-using-local-llms)
- [Configuration](#-configuration)
- [Tool System](#-tool-system)
- [Plan Mode](#-plan-mode)
- [Keyboard Shortcuts](#-keyboard-shortcuts)
- [Debug and Logging](#-debug-and-logging)
- [Architecture](#-architecture)
- [Project Structure](#-project-structure)
- [Development](#-development)
- [Platform Notes](#-platform-notes)
- [Disclaimers](#-disclaimers)
- [Contributing](#-contributing)
- [License](#-license)
- [Acknowledgments](#-acknowledgments)

---

## 📸 Screenshots

### Main Interface
![OpenCrab Main Interface](src/docs/screenshots/main-screen.png)
*Interactive chat interface with syntax highlighting and real-time streaming*

### Deep Code Analysis
![Deep Code Analysis](src/docs/screenshots/deeply-analyse.png)
*Comprehensive code analysis with detailed insights and suggestions*

### AI Thinking Mode
![AI Thinking Mode](src/docs/screenshots/thinking-mode.png)
*Watch the AI reasoning process in real-time as it analyzes your code*

### Help & Commands
![Help Screen](src/docs/screenshots/help-screnn.png)
*Built-in help system and keyboard shortcuts for efficient navigation*

---

## 🎯 Core Features

| Feature | Description |
|---------|-------------|
| **Built-in Tools** | Read/write files, execute commands, grep, glob, web search, and more |
| **Interactive Approval** | Permission dialogs for dangerous operations — full control over what AI can do |
| **Syntax Highlighting** | 100+ languages with line numbers via syntect |
| **Local LLM Support** | Run with LM Studio, Ollama, or any OpenAI-compatible endpoint — 100% private |
| **Multi-Provider** | Anthropic Claude (with OAuth), OpenAI, Qwen, and OpenAI-compatible APIs |
| **Session Context** | Persistent conversation memory with SQLite storage |
| **Streaming** | Real-time character-by-character response generation |
| **Cost Tracking** | Per-message token count and cost displayed in header |
| **Plan Mode** | Structured task decomposition with approval workflow |
| **Multi-line Input** | Paste entire functions, send with Ctrl+Enter |
| **Markdown Rendering** | Rich text formatting with code blocks and headings |
| **Debug Logging** | Conditional file logging with `-d` flag, clean workspace by default |

---

## 🔒 Interactive Approval System

When the AI wants to modify files or execute commands, OpenCrab pauses and asks for your permission:

```
┌────────────────────────────────────────────────────┐
│ ⚠️  PERMISSION REQUIRED                            │
├────────────────────────────────────────────────────┤
│ 🔒 Permission Request                              │
│                                                    │
│ Claude wants to use the tool: write_file          │
│                                                    │
│ Description: Write content to a file...            │
│                                                    │
│ ⚠️  Capabilities:                                   │
│    • WriteFiles                                    │
│    • SystemModification                            │
│                                                    │
│ Parameters:                                        │
│    path: "config.json"                             │
│    content: "{ \"debug\": true }"                  │
│                                                    │
│ [A]pprove  [D]eny  [V]iew Details  [Esc] Cancel  │
└────────────────────────────────────────────────────┘
```

**Security model:**

- **Dangerous operations always require approval:** file writes, shell commands, system modifications
- **Safe operations proceed automatically:** file reads, information queries
- **Full transparency:** view exact parameters before deciding, toggle detailed JSON view with `V`
- **Auto-deny timeout:** 5-minute countdown with visual color-coded timer (green/yellow/red)
- **Keyboard:** `A`/`Y` approve, `D`/`N` deny, `V` view details, `Esc` cancel

---

## 🌐 Supported AI Providers

### Anthropic Claude

**Models:** `claude-opus-4-6`, `claude-sonnet-4-5-20250929`, `claude-haiku-4-5-20251001`, plus legacy Claude 3.x models

**Authentication:**

| Method | Env Variable | Header |
|--------|-------------|--------|
| **OAuth / Claude Max** (recommended) | `ANTHROPIC_MAX_SETUP_TOKEN` | `Authorization: Bearer` + `anthropic-beta: oauth-2025-04-20` |
| Standard API Key | `ANTHROPIC_API_KEY` | `x-api-key` |

OAuth tokens are auto-detected by the `sk-ant-oat` prefix. When `ANTHROPIC_MAX_SETUP_TOKEN` is set, it takes priority over `ANTHROPIC_API_KEY`.

Set a custom model with `ANTHROPIC_MAX_MODEL` (e.g., `claude-opus-4-6`).

**Features:** Streaming, tools, cost tracking, automatic retry with backoff

### OpenAI

**Models:** GPT-4 Turbo, GPT-4, GPT-3.5 Turbo

**Setup:** `export OPENAI_API_KEY="sk-YOUR_KEY"`

Compatible with any OpenAI-compatible API endpoint via `OPENAI_BASE_URL`.

### Qwen (via OpenAI-compatible)

**Setup:** Configure via `QWEN_API_KEY` and `QWEN_BASE_URL`.

### OpenAI-Compatible Local / Cloud APIs

| Provider | Status | Setup |
|----------|--------|-------|
| **LM Studio** | Tested | `OPENAI_BASE_URL="http://localhost:1234/v1"` |
| **Ollama** | Compatible | `OPENAI_BASE_URL="http://localhost:11434/v1"` |
| **LocalAI** | Compatible | `OPENAI_BASE_URL="http://localhost:8080/v1"` |
| OpenRouter | Compatible | `OPENAI_BASE_URL="https://openrouter.ai/api/v1"` |
| Groq | Compatible | `OPENAI_BASE_URL="https://api.groq.com/openai/v1"` |

**Provider priority:** Qwen > OpenAI > Anthropic (fallback). The first provider with a configured API key is used.

---

## 🚀 Quick Start

### Prerequisites

- **Rust (2024 edition)** — [Install Rust](https://rustup.rs/)
- **An API key** from at least one supported provider
- **SQLite** (bundled via sqlx)
- **Linux:** `build-essential`, `pkg-config`, `libssl-dev`, `libchafa-dev`

### Install & Run

```bash
# Clone
git clone https://github.com/adolfousier/opencrab.git
cd opencrab

# Set up credentials (pick one)
cp .env.example .env
# Edit .env with your API key(s)

# Build
cargo build --release

# Run
cargo run
```

OpenCrab auto-loads `.env` via `dotenvy` at startup — no need to manually export variables.

### CLI Commands

```bash
# Interactive TUI (default)
cargo run
cargo run -- chat

# Non-interactive single command
cargo run -- run "What is Rust?"
cargo run -- run --format json "List 3 programming languages"
cargo run -- run --format markdown "Explain async/await"

# Configuration
cargo run -- init              # Initialize config
cargo run -- config            # Show current config
cargo run -- config --show-secrets

# Database
cargo run -- db init           # Initialize database
cargo run -- db stats          # Show statistics

# Keyring (secure OS credential storage)
cargo run -- keyring set anthropic YOUR_KEY
cargo run -- keyring get anthropic
cargo run -- keyring list

# Debug mode
cargo run -- -d                # Enable file logging
cargo run -- -d run "analyze this"

# Log management
cargo run -- logs status
cargo run -- logs view
cargo run -- logs view -l 100
cargo run -- logs clean
cargo run -- logs clean -d 3
```

**Output formats** for non-interactive mode: `text` (default), `json`, `markdown`

---

## 🔑 Authentication Methods

### Option A: OAuth / Claude Max (Recommended for Claude)

```bash
# In .env file:
ANTHROPIC_MAX_SETUP_TOKEN=sk-ant-oat01-YOUR_OAUTH_TOKEN
ANTHROPIC_MAX_MODEL=claude-opus-4-6
```

The `sk-ant-oat` prefix is auto-detected. OpenCrab will use `Authorization: Bearer` with the `anthropic-beta: oauth-2025-04-20` header.

### Option B: Standard API Key

```bash
# In .env or exported:
ANTHROPIC_API_KEY=sk-ant-api03-YOUR_KEY
OPENAI_API_KEY=sk-YOUR_KEY
```

### Option C: OS Keyring (Secure Storage)

```bash
cargo run -- keyring set anthropic YOUR_API_KEY
# Encrypted by OS (Windows Credential Manager / macOS Keychain / Linux Secret Service)
# Automatically loaded on startup, no plaintext files
```

**Priority:** Keyring > `ANTHROPIC_MAX_SETUP_TOKEN` > `ANTHROPIC_API_KEY` > config file

---

## 🏠 Using Local LLMs

OpenCrab works with any OpenAI-compatible local inference server for **100% private, zero-cost** operation.

### LM Studio (Recommended)

1. Download and install [LM Studio](https://lmstudio.ai/)
2. Download a model (e.g., `qwen2.5-coder-7b-instruct`, `Mistral-7B-Instruct`, `Llama-3-8B`)
3. Start the local server (default port 1234)
4. Configure OpenCrab:

```bash
# .env or environment
OPENAI_API_KEY="lm-studio"
OPENAI_BASE_URL="http://localhost:1234/v1"
```

Or via `opencrab.toml`:

```toml
[providers.openai]
enabled = true
base_url = "http://localhost:1234/v1/chat/completions"
default_model = "qwen2.5-coder-7b-instruct"   # Must EXACTLY match LM Studio model name
```

> **Critical:** The `default_model` value must exactly match the model name shown in LM Studio's Local Server tab (case-sensitive).

### Ollama

```bash
ollama pull mistral
# Configure:
OPENAI_BASE_URL="http://localhost:11434/v1"
OPENAI_API_KEY="ollama"
```

### Recommended Models

| Model | RAM | Best For |
|-------|-----|----------|
| Qwen-2.5-7B-Instruct | 16 GB | Coding tasks |
| Mistral-7B-Instruct | 16 GB | General purpose, fast |
| Llama-3-8B-Instruct | 16 GB | Balanced performance |
| DeepSeek-Coder-6.7B | 16 GB | Code-focused |
| TinyLlama-1.1B | 4 GB | Quick responses, lightweight |

**Tips:**
- Start with Q4_K_M quantization for best speed/quality balance
- Set context length to 8192+ in LM Studio settings
- Use `Ctrl+N` to start a new session if you hit context limits
- GPU acceleration significantly improves inference speed

### Cloud vs Local Comparison

| Aspect | Cloud (Anthropic) | Local (LM Studio) |
|--------|-------------------|-------------------|
| Privacy | Data sent to API | 100% private |
| Cost | Per-token pricing | Free after download |
| Speed | 1-2s (network) | 2-10s (hardware-dependent) |
| Quality | Excellent (Claude 4.x) | Good (model-dependent) |
| Offline | Requires internet | Works offline |

See [LM_STUDIO_GUIDE.md](src/docs/guides/LM_STUDIO_GUIDE.md) for detailed setup and troubleshooting.

---

## 📝 Configuration

### Configuration File (`opencrab.toml`)

OpenCrab searches for config in this order:
1. `./opencrab.toml` (current directory)
2. `~/.config/opencrab/opencrab.toml` (Linux/macOS) or `%APPDATA%\opencrab\opencrab.toml` (Windows)
3. `~/opencrab.toml`

Environment variables override config file settings. `.env` files are auto-loaded.

```bash
# Initialize config
cargo run -- init

# Copy the example
cp config.toml.example ~/.config/opencrab/opencrab.toml
```

### Example: Hybrid Setup (Local + Cloud)

```toml
[database]
path = "~/.opencrab/opencrab.db"

# Local LLM for daily development
[providers.openai]
enabled = true
base_url = "http://localhost:1234/v1/chat/completions"
default_model = "qwen2.5-coder-7b-instruct"

# Cloud API for complex tasks
[providers.anthropic]
enabled = true
default_model = "claude-opus-4-6"
# API key via env var or keyring
```

### Environment Variables

| Variable | Provider | Description |
|----------|----------|-------------|
| `ANTHROPIC_MAX_SETUP_TOKEN` | Anthropic (OAuth) | OAuth Bearer token (takes priority) |
| `ANTHROPIC_MAX_MODEL` | Anthropic | Custom default model |
| `ANTHROPIC_API_KEY` | Anthropic | Standard API key |
| `OPENAI_API_KEY` | OpenAI / Compatible | API key |
| `OPENAI_BASE_URL` | OpenAI / Compatible | Custom endpoint URL |
| `QWEN_API_KEY` | Qwen | API key |
| `QWEN_BASE_URL` | Qwen | Custom endpoint URL |

---

## 🔧 Tool System

OpenCrab includes a built-in tool execution system. The AI can use these tools during conversation:

| Tool | Description | Requires Approval |
|------|-------------|-------------------|
| `read_file` | Read file contents with syntax awareness | No |
| `write_file` | Create or modify files | **Yes** |
| `edit_file` | Precise text replacements in files | **Yes** |
| `bash` | Execute shell commands | **Yes** |
| `ls` | List directory contents | No |
| `glob` | Find files matching patterns | No |
| `grep` | Search file contents with regex | No |
| `web_search` | Search the web | No |
| `execute_code` | Run code in various languages | **Yes** |
| `notebook_edit` | Edit Jupyter notebooks | **Yes** |
| `parse_document` | Extract text from PDF, DOCX, HTML | No |
| `task_manager` | Manage agent tasks | No |
| `http_request` | Make HTTP requests | **Yes** |
| `session_context` | Access session information | No |
| `plan` | Create structured execution plans | No |

**Example session:**

```
You: "Read src/main.rs"
OpenCrab: [reads file with syntax highlighting]

You: "Add error handling to the database connection"
OpenCrab: [approval dialog] → [modifies file with write tool]

You: "Run cargo test"
OpenCrab: [approval dialog] → [executes] ✅ 145 tests passed
```

---

## 📋 Plan Mode

Plan Mode breaks complex tasks into structured, reviewable, executable plans.

### Workflow

1. **Request:** Ask the AI to create a plan using the plan tool
2. **AI creates:** Structured tasks with dependencies, complexity estimates, and types
3. **Review:** Press `Ctrl+P` to view the plan in a visual TUI panel
4. **Decide:**
   - `Ctrl+A` — Approve and execute
   - `Ctrl+R` — Reject the plan
   - `Ctrl+I` — Request changes (returns to chat with context)
   - `Esc` — Go back without changes

### Plan States

Plans progress through: **Draft** → **PendingApproval** → **Approved** → **InProgress** → **Completed**

Tasks have 10 types: Research, Edit, Create, Delete, Test, Refactor, Documentation, Configuration, Build, Other

Each task tracks: status (Pending/InProgress/Completed/Skipped/Failed/Blocked), dependencies, complexity (1-5), and timestamps.

### Example

```
You: Use the plan tool to create a plan for implementing JWT authentication.
     Add tasks for: adding dependencies, token generation, validation
     middleware, updating login endpoint, and writing tests.
     Call operation=finalize when done.

OpenCrab: [Creates plan with 5 tasks, dependencies, complexity ratings]
         ✓ Plan finalized! Press Ctrl+P to review.
```

```
┌─────────────────────────────────────────────────────────────┐
│ 📋 Plan: JWT Authentication                                 │
│ Status: Pending Approval • Tasks: 5 • Complexity: Medium    │
├─────────────────────────────────────────────────────────────┤
│ 1. [⏹] Add jsonwebtoken dependency (⭐⭐)                   │
│ 2. [⏹] Implement token generation (⭐⭐⭐⭐) → depends on #1 │
│ 3. [⏹] Build validation middleware (⭐⭐⭐⭐⭐) → depends on #2│
│ 4. [⏹] Update login endpoint (⭐⭐⭐) → depends on #2       │
│ 5. [⏹] Write integration tests (⭐⭐⭐) → depends on #3, #4 │
├─────────────────────────────────────────────────────────────┤
│ [Ctrl+A] Approve  [Ctrl+R] Reject  [Ctrl+I] Changes  [Esc]│
└─────────────────────────────────────────────────────────────┘
```

**Tip for local LLMs:** Be explicit about tool usage — say "use the plan tool with operation=create" rather than "create a plan".

See [Plan Mode User Guide](src/docs/PLAN_MODE_USER_GUIDE.md) for full documentation.

---

## ⌨️ Keyboard Shortcuts

### Global

| Shortcut | Action |
|----------|--------|
| `Ctrl+C` | Quit |
| `Ctrl+H` | Show help screen |
| `Ctrl+N` | New session |
| `Ctrl+L` | List/switch sessions |
| `Page Up/Down` | Scroll chat history |
| `Escape` | Clear input / close overlay |

### Chat Mode

| Shortcut | Action |
|----------|--------|
| `Ctrl+Enter` | Send message |
| `Enter` | New line in input |

### Plan Mode

| Shortcut | Action |
|----------|--------|
| `Ctrl+P` | View current plan |
| `Ctrl+A` | Approve plan |
| `Ctrl+R` | Reject plan |
| `Ctrl+I` | Request changes |
| `↑` / `↓` | Scroll through plan |

---

## 🔍 Debug and Logging

OpenCrab uses a **conditional logging system** — no log files by default.

```bash
# Enable debug mode (creates log files)
opencrab -d
cargo run -- -d

# Logs stored in .opencrab/logs/ (auto-gitignored)
# Daily rolling rotation, auto-cleanup after 7 days

# Management
opencrab logs status    # Check logging status
opencrab logs view      # View recent entries
opencrab logs clean     # Clean old logs
opencrab logs clean -d 3  # Clean logs older than 3 days
```

**When debug mode is enabled:**
- Log files created in `.opencrab/logs/`
- DEBUG level with thread IDs, file names, line numbers
- Daily rolling rotation

**When disabled (default):**
- No log files created
- Only warnings and errors to stderr
- Clean workspace

---

## 🏗️ Architecture

```
Presentation Layer
    ↓
CLI (Clap) + TUI (Ratatui + Crossterm)
    ↓
Application Layer
    ↓
Service Layer (Session, Message, Agent, Plan)
    ↓
Data Access Layer (SQLx + SQLite)
    ↓
Integration Layer (LLM Providers, LSP, MCP)
```

**Key Technologies:**

| Component | Crate |
|-----------|-------|
| Async Runtime | Tokio |
| Terminal UI | Ratatui + Crossterm |
| CLI Parsing | Clap (derive) |
| Database | SQLx (SQLite) |
| Serialization | Serde + TOML |
| HTTP Client | Reqwest |
| Syntax Highlighting | Syntect |
| Markdown | pulldown-cmark |
| LSP Client | Tower-LSP |
| Provider Registry | Crabrace |
| Error Handling | anyhow + thiserror + color-eyre |
| Logging | tracing + tracing-subscriber |
| Security | zeroize + keyring |

---

## 📁 Project Structure

```
opencrab/
├── src/
│   ├── main.rs           # Entry point
│   ├── lib.rs            # Library root
│   ├── error.rs          # Error types
│   ├── logging.rs        # Conditional logging system
│   ├── app/              # Application lifecycle
│   ├── cli/              # Command-line interface (Clap)
│   ├── config/           # Configuration (TOML + env + keyring)
│   │   └── crabrace.rs   # Provider registry integration
│   ├── db/               # Database layer (SQLx + SQLite)
│   ├── services/         # Business logic (Session, Message, File, Plan)
│   ├── llm/              # LLM integration
│   │   ├── agent/        # Agent service + context management
│   │   ├── provider/     # Provider implementations (Anthropic, OpenAI, Qwen)
│   │   ├── tools/        # Tool system (read, write, bash, glob, grep, etc.)
│   │   └── prompt/       # Prompt engineering
│   ├── tui/              # Terminal UI (Ratatui)
│   ├── lsp/              # LSP integration
│   ├── mcp/              # Model Context Protocol
│   ├── events/           # Event handling
│   ├── message/          # Message types
│   ├── sync/             # Synchronization utilities
│   ├── macros/           # Rust macros
│   ├── utils/            # Utilities (retry, etc.)
│   ├── migrations/       # SQLite migrations
│   ├── tests/            # Integration tests
│   ├── benches/          # Criterion benchmarks
│   └── docs/             # Documentation + screenshots
├── Cargo.toml
├── config.toml.example
├── .env.example
└── LICENSE.md
```

---

## 🛠️ Development

### Build from Source

```bash
# Development build
cargo build

# Release build (optimized, LTO, stripped)
cargo build --release

# Small release build
cargo build --profile release-small

# Run tests
cargo test

# Run benchmarks
cargo bench

# Format + lint
cargo fmt
cargo clippy -- -D warnings
```

### Feature Flags

| Feature | Description |
|---------|-------------|
| `openai` | Enable async-openai integration |
| `aws-bedrock` | Enable AWS Bedrock runtime |
| `all-llm` | Enable all LLM provider features |
| `profiling` | Enable pprof flamegraph profiling (Unix only) |

### Performance

| Metric | Value |
|--------|-------|
| Startup time | < 50ms |
| Memory (idle) | ~15 MB |
| Memory (100 messages) | ~20 MB |
| Database ops | < 10ms (session), < 5ms (message) |

---

## 🐛 Platform Notes

### Linux

```bash
sudo apt-get install build-essential pkg-config libssl-dev libchafa-dev
```

### macOS

No additional dependencies required.

### Windows

Requires CMake, NASM, and Visual Studio Build Tools for native crypto dependencies:

```bash
# Option 1: Install build tools
# - CMake (add to PATH)
# - NASM (add to PATH)
# - Visual Studio Build Tools ("Desktop development with C++")

# Option 2: Use WSL2 (recommended)
sudo apt-get install build-essential pkg-config libssl-dev
```

See [BUILD_NOTES.md](src/docs/guides/BUILD_NOTES.md) for detailed troubleshooting.

---

## ⚠️ Disclaimers

### Development Status

OpenCrab is under active development. While functional, it may contain bugs or incomplete features.

### Token Cost Responsibility

**You are responsible for monitoring and managing your own API usage and costs.**

- API costs from cloud providers (Anthropic, OpenAI, etc.) are your responsibility
- Set billing alerts with your provider
- Consider local LLMs for cost-free operation
- Use the built-in cost tracker to monitor spending

### Support

Cloud API issues, billing questions, and account problems should be directed to the respective providers. OpenCrab provides the tool; you manage your API relationships.

---

## 🤝 Contributing

Contributions welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

```bash
# Setup
git clone https://github.com/adolfousier/opencrab.git
cd opencrab
cargo build
cargo test
# Make changes, then submit a PR
```

---

## 📄 License

**FSL-1.1-MIT License**

- **Functional Source License (FSL) 1.1** — First 2 years
- **MIT License** — After 2 years from release

See [LICENSE.md](LICENSE.md) for details.

---

## 🙏 Acknowledgments

- **[OpenClaw](https://github.com/anthropics/claude-code)** — Inspiration
- **[Crabrace](https://crates.io/crates/crabrace)** — Provider registry
- **[Ratatui](https://ratatui.rs/)** — Terminal UI framework
- **[Anthropic](https://anthropic.com/)** — Claude API

---

## 📞 Support

- **Issues:** [GitHub Issues](https://github.com/adolfousier/opencrab/issues)
- **Discussions:** [GitHub Discussions](https://github.com/adolfousier/opencrab/discussions)
- **Docs:** [src/docs/](src/docs/)

---

**Built with Rust 🦀 by [Adolfo Usier](https://github.com/adolfousier)**
