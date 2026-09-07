# OpenCrabs Ontology — Shared Vocabulary (SSOT)

> **Owns:** the project's shared/core vocabulary — one concept = one name, defined once, used everywhere.
> **Status:** AS IS (describes terms as the codebase uses them today)
> **Audience:** every contributor — human or LLM agent — writing code, docs, issues, or PRs in this repo.

**Law of this file:** a concept gets ONE name. If you need to name a new concept, propose it here first. If an existing term fits, use it — never invent a synonym. Domain slices (brain, plan-mode) stay canonical inside their own docs; this file defines the shared core and links out. Definitions describe what the code DOES (AS IS) — behavior changes go through issues, not redefinitions.

**Why an ontology matters here:** the primary readers of this repo's docs and prompts are LLM agents. Unanchored vocabulary produces invented synonyms, misrouted tool calls, and conflated concepts (the codebase already carries `tool_name_heal.rs` and `phantom.rs` to absorb exactly this failure class). One grounded name per concept is cheaper than healing.

## How to read this file

| Mark | Meaning |
|---|---|
| *(term)* | the canonical name — use exactly this |
| **Not:** X, Y | known synonyms/wrong names — do not use them |
| → | deeper definition lives in the linked slice; that slice is canonical for that domain |

## Core concepts

### Runtime & identity

**OpenCrabs** — the binary and project: an AI orchestration agent with tools, channels, memory, and skills.

**Profile** — an isolated instance configuration rooted at `~/.opencrabs/profiles/<name>/` (own config, keys, brain files, sessions). The default install runs at `~/.opencrabs/` with no profile subdirectory. **Not:** "instance", "workspace".

**Session** — one conversation with its own context window, history, and tool loop, identified by a UUID (`session_id`). Sessions belong to a profile. **Not:** "chat" (a chat is a channel-side conversation; one chat can drive a session, sessions can exist without a chat).

**Channel** — a messaging surface connected to OpenCrabs (Telegram, Discord, Slack, WhatsApp, Trello). The harness routes inbound channel messages into sessions and outbound replies back. **Not:** "surface", "connection".

**A2A** — Agent-to-Agent protocol: JSON-RPC 2.0 peer communication between OpenCrabs and remote agents (agent card discovery, task send/get/cancel). See `src/a2a/`.

**Cron job** — a scheduled task in the `cron_jobs` table, polled by the scheduler and executed in the user's active session. Defined by prompt + schedule; delivery goes through a configured channel. See `src/cron/`. **Not:** "timer", "task" (a task is a plan checklist row).

### Brain & directives

**Directive** — any information that shapes agent behavior: rules, tool definitions, skills, commands, config, project knowledge. → brain slice (`BRAIN_CONSTITUTION.md`).

**Brain file** — a markdown file in the profile home that shapes agent behavior; each declares what it owns in an `Owns:` header. Core files are always in the system prompt; contextual files load on demand via `load_brain_file`. → `BRAIN_CONSTITUTION.md` §1 (canonical for all brain-file subtypes). **Not:** "memory file" (memory is one brain file among several).

**Skill** — a reusable multi-step workflow defined in `SKILL.md` with YAML frontmatter; user skills live in `<profile home>/skills/<name>/`, built-in skills are embedded in the binary. **Not:** "command" (a command is a slash mapping, below).

**Command** — a user-defined `/<name>` slash command in `commands.toml`, mapping to a prompt or system action. **Not:** "skill".

**Dynamic tool** — a runtime tool definition in `tools.toml` adding a callable tool without recompiling. Distinct from an extended tool, which is compiled into the binary and lazily surfaced via `tool_search`.

### Tools & the loop

**Tool** — a callable function the model invokes through the structured tool-call API. **Not:** "function", "action".

**Core tool** — schema sent to the LLM on every request. **Extended tool** — schema surfaced only after `tool_search` activates it. → `BRAIN_CONSTITUTION.md` §1.

**Tool call** — one structured invocation of a tool (emitted between turns; results arrive as tool results). A tool call that is *described* in prose but never emitted is a phantom tool call — the codebase's phantom-heal machinery exists because of it. **Not:** "command run".

**Tool result** — the output returned to the model after a tool call. The only evidence a tool executed.

### Memory

**Memory search** — retrieval over past daily logs (`memory` scope), brain files (`brain` scope), and indexed external paths (`external` scope). See `src/brain/tools/memory_search.rs`. **Not:** "recall" as a synonym for the tool (recall is the ranking subsystem inside it).

**Daily log** — the per-day memory file (`memory/YYYY-MM-DD.md`) that records what happened in past sessions. Distinct from brain files (rules/policy).

### Plan mode

**Plan** — a session's structured plan: design prose plus an executable checklist, stored as JSON sidecar (`.opencrabs_plan_<session-id>.json`) mirrored to a session `.md`. Managed by the `plan` tool. **Not:** "task list" alone (a plan holds the design and its execution track), "roadmap".

**Session plan / checklist** — the two representations of a plan: the design `.md` (Editing status, user-reviewable) and the executable `tasks[]` (Active status). **Not:** "plan file", "todo list".

**Design track / checklist track** — how a plan starts: design goes to Editing and waits for user Approve; a checklist with inline tasks goes Active immediately. → plan-mode slice (`plans/plan-mode/README.md`).

**ADR** — Architecture Decision Record: a numbered doc capturing one decision cluster with phases and done criteria (used by the plan-mode slices). → `plans/plan-mode/README.md`.

### Fleet & process terms

These live with the ops skill outside this repo (`fleet-directives.md` §Glossary: carrier, fan-out, lane, roster, CI gate, ORDER gates, single-flight, GREEN/RED, S2/S3). They are listed here by NAME ONLY so repo readers know the terms exist and where they are defined — this file does not copy them (single-writer law).

## Maintenance

- **Adding a term:** PR-only. Define AS IS, add NOT-references for any synonym you found in the wild, link the canonical slice if one exists.
- **Changing a definition:** the definition must match what the code does. If code and ontology disagree, file an issue — do not redefine the term to match intent.
- **Drift check:** when a domain slice renames or retires a term, the same PR updates the references here.
- Do not duplicate a domain slice's terms here — link them (one concept, one home).
