# TOOLS.md - Tool Definitions

> **Owns:** tool access, skills, routing pointers, build/runtime commands. Tool params & search/GitHub/browser routing live in the system prompt — don't duplicate them here.

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
| `documents` | generate XLSX (live formulas) / DOCX / PDF / PPTX with branding, parse documents, PDF to images | "create a spreadsheet with formulas" |
| `system` | feedback_record/analyze, self_improve, rebuild, evolve, tool_manage, rsi_proposals | "rebuild from source" |
| `utility` | cron_manage, session_search, channel_search, mission_control_report, a2a_send | "create a cron job" |

Rule: if a task needs a non-core tool, call `tool_search` with a plain-words description FIRST —
never assume the capability is missing before searching.

## What belongs here

- Skill pointers (what/where to load on demand)
- Commands vs Tools vs Skills distinction
- Profile-aware paths
- Custom routing rules specific to your setup


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

## Build & Runtime Commands

- `/cd <path>` — change the working directory for all tool execution (or `config_tool` `set_working_directory`); persists to config.toml
- `/rebuild` — Build, test, and hot-restart from source
- `/check` — Run `cargo clippy` and `cargo test`
- `/evolve` — Download latest release binary (full procedure → BOOT.md)

## Scheduling (Cron)

Manage scheduled jobs with the **`cron_manage`** tool (`action`: create / list / delete / enable / disable / test). Jobs run in **isolated sessions on your configured provider/model by default** — omit `provider`/`model` for the default; set `thinking: off` for routine jobs; use `deliver_to` only to send results to a channel.

**Cron expression format (the common trap):** 5 fields `min hour dom mon dow`. Day-of-week is **1-7 = Sun-Sat** (1=Sunday, 7=Saturday; `0` is invalid) — **use day names** (`Mon-Fri`, `Sun`) instead of numbers. No `@daily`/`@hourly` macros. Set `tz` (IANA, e.g. `America/New_York`) and the job runs in that zone's local time, DST-aware. **Validate before you confirm:** `create` echoes the next run times — read them back; a wrong day-of-week parses fine but the next-run list exposes it. Fix and recreate before telling the user it's set.

## Voice & Audio

STT providers: `voicebox` (local server) > `openai_compatible` > `groq` (Whisper API) > `local` (rwhisper, `local-stt` feature). Override with `stt_fallback_chain`.
TTS providers: `voicebox` (local server) > `openai_compatible` > `openai` (OpenAI TTS) > `local` (Piper, `local-tts` feature). Override with `tts_fallback_chain`.
Config: `[providers.stt.*]` / `[providers.tts.*]` in config.toml. Piper voices: `ryan`(default), `amy`, `lessac`, `kristin`, `joe`, `cori`. Local STT presets: `local-tiny`(42MB), `local-base`(142MB), `local-small`(466MB), `local-medium`(1.5GB).
Audio: all output OGG/Opus via ffmpeg. Models: whisper in `~/.local/share/opencrabs/models/whisper/`, piper in `~/.local/share/opencrabs/models/piper/`. Setup: `/onboard:voice`.

## Reporting

- `/mission-control`: analytics (tool usage, failure rates, RSI improvements, brain files), activity feed, inbox proposals, and scheduled cron jobs.
  Works in the TUI (opens the Mission Control Analytics panel) and in every
  channel (returns the report as a message). The same data is also available as
  the `mission_control_report` agent tool, so you can ask in plain language (for
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
