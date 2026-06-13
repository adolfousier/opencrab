# TOOLS.md - Tool Definitions

This file is for tool routing rules and pointers. Tool params, search/GitHub/browser routing,
and RSI instructions are already in the system prompt — don't duplicate them here.

## Tool Access — core set + on-demand discovery

When `[agent] lazy_tools = true`, only the CORE tools ship in every request; everything else is
pulled on demand with `tool_search` (keeps a tool-light turn from carrying ~20k tokens of unused
schemas). When the flag is off, all tools are always available and `tool_search` is just a no-op
convenience.

**Core (always available):** `read_file`, `write_file`, `edit_file`, `hashline_edit`, `bash`,
`ls`, `glob`, `grep`, `web_search`, `exa_search`, `memory_search`, `task`, `context`, `plan`,
`http_client`, `load_brain_file`, `write_opencrabs_file`, `config_tool`, `slash_command`,
`rename_session`, `follow_up_question`, `tool_search`.

**Extended (call `tool_search("…")` to discover + activate):**

| Category | What it covers | Example query |
|----------|----------------|---------------|
| `browser` | navigate / click / type / screenshot / eval on live pages | "click a button on a web page" |
| `channels` | Telegram / Discord / Slack / WhatsApp / Trello — send + connect | "send a telegram photo" |
| `agents` | spawn / wait / send-input / close / resume sub-agents, teams | "spawn a sub-agent" |
| `media` | generate / analyze images, analyze video, provider vision | "generate an image" |
| `system` | feedback_record/analyze, self_improve, rebuild, evolve, tool_manage, rsi_proposals | "rebuild from source" |
| `utility` | cron_manage, session_search, channel_search, analytics_report, a2a_send | "create a cron job" |

Rule: if a task needs a non-core tool, call `tool_search` with a plain-words description FIRST —
never assume the capability is missing before searching.

## What belongs here

- Skill pointers (what/where to load on demand)
- Commands vs Tools vs Skills distinction
- Profile-aware paths
- Custom routing rules specific to your setup

## What does NOT belong here

- Failure logs or timestamps (use `feedback_record`)
- Full CLI references (put in skills, load on demand)
- Provider configuration (lives in config.toml + onboarding)
- System commands (basic OS knowledge)

## Skills (load on demand)

| Skill | Command | What it covers |
|-------|---------|----------------|
| Browser CDP | `/browser-cdp` | CDP automation, selectors, screenshots |
| Channels | `/channels` | Telegram, Discord, Slack, Trello, WhatsApp setup |
| Dynamic Tools | `/dynamic-tools` | tools.toml format, runtime tool management |
| SocialCrabs | `/socialcrabs` | Twitter/X, Instagram, LinkedIn automation |
| Google CLI | `/gog` | Gmail, Calendar via gog CLI |
| GitHub Workflow | `/github_workflow` | CI/CD, branch protection, release workflow |
| A2A Gateway | `/a2a-gateway` | Agent-to-Agent protocol reference |
| Servers | `/servers` | SSH aliases, Docker containers, Nginx sites |

## Commands vs Tools vs Skills

| Concept | What it is | Example |
|---------|-----------|---------|
| Tool | A function the agent calls directly | `bash`, `read_file`, `grep` |
| Command | A slash shortcut defined in commands.toml | `/check`, `/rebuild`, `/status` |
| Skill | A workflow template loaded on demand | `/browser-cdp`, `/channels` |

## Build Commands

- `/rebuild` — Build, test, and hot-restart from source
- `/check` — Run `cargo clippy` and `cargo test`
- `/evolve` — Download latest release binary

## Reporting

- `/analytics`: brain sizes, tool usage and failure rates, and RSI applications.
  Works in the TUI (opens the Mission Control Analytics panel) and in every
  channel (returns the report as a message). The same data is also available as
  the `analytics_report` agent tool, so you can ask in plain language (for
  example "send me my analytics") and the agent ships the report to the chat.

## Profile-Aware Paths

| What | Path |
|------|------|
| Brain files | `~/.opencrabs/{SOUL,USER,AGENTS,TOOLS,MEMORY,CODE,SECURITY}.md` |
| Config | `~/.opencrabs/config.toml` |
| Keys | `~/.opencrabs/keys.toml` |
| Commands | `~/.opencrabs/commands.toml` |
| Plans | `~/.opencrabs/agents/session/.opencrabs_plan_<id>.json` |
| Logs | `~/.opencrabs/logs/opencrabs.YYYY-MM-DD` |
