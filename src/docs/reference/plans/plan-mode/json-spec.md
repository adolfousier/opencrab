# Plan JSON Schema Reference

## Minimal Import Format

Only **3 fields required** at the root and **3 per task** for a valid import:

### Root Level (3 fields)
```json
{
  "title": "Plan name",
  "description": "What this plan accomplishes",
  "tasks": [...]
}
```

### Task Level (3 fields per task)
```json
{
  "title": "Task name",
  "description": "What this task does",
  "task_type": "research"
}
```

## Optional Task Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `dependencies` | `integer[]` or `string[]` | *(omitted)* | **Optional.** Omit when there are no dependencies — empty `[]` is redundant. When present, use 1-based integer indices (e.g. `[1, 2]`) for readability. UUID strings also accepted. |
| `complexity` | `number` | `3` | 1-5 scale: 1=trivial, 2=simple, 3=moderate, 4=complex, 5=very complex |
| `acceptance_criteria` | `string[]` | `[]` | List of conditions that mark this task complete |

## Auto-Generated Fields (Do NOT Provide)

These are always overwritten on import:

### Root Level
- `id` — UUID, regenerated
- `session_id` — UUID, from session
- `status` — set by `plan init` / Approve (see **Status on write** below; not always `"Draft"`)
- `created_at` — ISO timestamp
- `updated_at` — ISO timestamp
- `approved_at` — `null` until user Approve on design track; then ISO timestamp
- `context` — defaults to empty string
- `risks` — defaults to `[]`
- `technical_stack` — defaults to `[]`
- `test_strategy` — defaults to empty string

### Task Level
- `id` — UUID, auto-minted (do not provide — see Optional Fields note above)
- `order` — 1-based; auto-assigned from array position if omitted (recommended to omit)
- `status` — always `"Pending"`
- `notes` — always `null`
- `completed_at` — always `null`
- `execution_history` — always `[]`
- `retry_count` — always `0`
- `max_retries` — defaults to `3`
- `artifacts` — always `[]`
- `reflection` — always `null`

## task_type Values

Case-insensitive. Examples: `"Research"`, `"research"`, `"RESEARCH"` all work.

| Value | Description |
|-------|-------------|
| `research` | Investigate, explore, understand |
| `edit` | Modify existing code/files |
| `create` | Build new things from scratch |
| `delete` | Remove code/files |
| `test` | Write or run tests |
| `documentation` | Docs, comments, specs |
| `configuration` | Config, setup, infrastructure |

## Dependencies

Use **1-based integer indices** (human-friendly). **Omit the field when there are no dependencies** — do not write `"dependencies": []`.

```json
"dependencies": [1, 2]        // depends on tasks 1 and 2
"dependencies": [1]            // depends on task 1 only
// (no field at all)           // no dependencies — preferred over []
```

## JSON Schema (Machine-Readable)

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "required": ["title", "description", "tasks"],
  "additionalProperties": true,
  "properties": {
    "title": { "type": "string" },
    "description": { "type": "string" },
    "tasks": {
      "type": "array",
      "minItems": 1,
      "items": {
        "type": "object",
        "required": ["title", "description", "task_type"],
        "additionalProperties": true,
        "properties": {
          "title": { "type": "string" },
          "description": { "type": "string" },
          "task_type": {
            "type": "string",
            "enum": ["research", "edit", "create", "delete", "test", "documentation", "configuration"],
            "pattern": "^(?i)(research|edit|create|delete|test|documentation|configuration)$"
          },
          "dependencies": {
            "type": "array",
            "items": { "oneOf": [{ "type": "integer", "minimum": 1 }, { "type": "string", "pattern": "^[0-9a-fA-F-]{36}$" }] }
          },
          "complexity": { "type": "integer", "minimum": 1, "maximum": 5, "default": 3 },
          "acceptance_criteria": { "type": "array", "items": { "type": "string" }, "default": [] }
        }
      }
    }
  }
}
```

## Example Minimal Plan

```json
{
  "title": "Add user authentication",
  "description": "Implement login/logout flow with session management",
  "tasks": [
    { "title": "Research auth patterns", "description": "Look at existing auth code and pick a pattern", "task_type": "research" },
    { "title": "Write login handler", "description": "POST /auth/login with password verification", "task_type": "create", "dependencies": [1] },
    { "title": "Add session middleware", "description": "Attach user context to requests", "task_type": "configuration", "dependencies": [2] },
    { "title": "Write auth tests", "description": "Test login, logout, and session expiry", "task_type": "test", "dependencies": [3], "complexity": 2 }
  ]
}
```

## Files

- **Minimal example**: `~/.opencrabs/profiles/ops/plans/coding-plans/sample-minimal-plan.json`
- **Full example**: `~/.opencrabs/profiles/ops/plans/coding-plans/rust-full.json`

---

## Plan mode design decisions (implementation reference)

These decisions were locked after validating Adolfo’s OC Dev audit (2026-07-11) against the codebase. The full product spec lives in [src/docs/reference/plans/plan-mode/umbrella.md](umbrella.md).

### Where plan data lives

Plan artifacts live in the session's resolved data directory, `session_dir(session_id)` in `src/utils/plan_files.rs`. Resolution is by what the session is bound to, in order:

1. **Project-bound session** → `~/.opencrabs/projects/<slug>/session/` (DB lookup session → project, mirrors `FileService::project_files_dir` but with a `session/` subdir).
2. **Otherwise** → `<profile home>/session/` (`opencrabs_home()` is profile-aware, so this is `~/.opencrabs/session/` on the default profile and `~/.opencrabs/profiles/<name>/session/` on a named one).

Because the project branch is a DB lookup, `session_dir` and every path/load/save helper built on it are **async**. Reads fall back to the legacy flat `~/.opencrabs/agents/session/` (via `plan_json_read_path`) so plans written by an older binary are not orphaned; writes always target the resolved location.

| Store | Role |
|---|---|
| `<session_dir>/.opencrabs_plan_{session}.json` | **Live.** The `plan` tool and TUI read and write this file on every checklist operation; the minimal pre-init Editing sidecar uses the same path. Single loader/saver: `src/utils/plan_files.rs`. |
| `<session_dir>/.opencrabs_plan_{session}.md` | Session plan prose while Editing (design track). Frozen against generic write tools once Active. Successful writes mirror the full body into JSON `description`. |
| `<session_dir>/archive/` | Completed plans retire here with a timestamp (no lingering live Done status). |
| `~/.opencrabs/agents/session/` | **Legacy read fallback only.** Pre-resolution flat location; never written to. |
| SQLite `plans` / `plan_tasks` | **Dropped.** Zero production callers were confirmed; `PlanService` and `PlanRepository` are deleted, and migration `20260713000001_drop_orphaned_plans_tables.sql` removed the tables themselves. |

JSON is the single live store; SQLite is not dual-written. Legacy JSON files that still carry old `PlanStatus` strings (Draft through Cancelled) are mapped on load by `plan_files::load_plan` (Cluster C Phase C1, umbrella Phase 3); legacy draft checklists (tasks, no `.md`) normalize to Active so they stay executable. The `approved_at` timestamp stays on the struct; set on user Approve, not on first `start`.

### Status on write (canonical JSON `status` field)

| Event | `status` written |
|---|---|
| `init mode=design` | `"Editing"` |
| `init mode=checklist` or import | `"Active"` |
| User Approve / `/execute` (design) | `"Active"` (+ `approved_at`) |
| `/discard` or archive on complete | files removed → NoPlan |

Legacy strings on disk are mapped on load (Draft/PendingApproval/Rejected → Editing; Approved/InProgress → Active; Completed → silent archive).

### Tool contract (implemented, Cluster C Phase C3)

- **`init`** takes `mode` (`design` | `checklist`). Omitted mode: tasks present imply checklist, none imply design. `mode=design` with tasks, `mode=checklist` without tasks, design under tool auto-approve (yolo), and empty imports are all refused with the allowed alternative. `init` is permitted from NoPlan or pre-init only (the first successful `init` upgrades or replaces the pre-init sidecar); a live post-init or Active plan refuses with "discard first".
- **`add_tasks`** is the primary way to append checklist items (array, at least one element). Active only.
- **`add_task`** remains as an alias that appends a single task. Prompts now prefer `add_tasks` (Cluster D Phase D2 shipped); the alias is deprecated in docs but stays in the schema so mid-session models keep working.
- **`start` / `complete`** are Active only; Editing (pre-init or post-init) gets a deterministic refusal. Auto-approve on first `start` is removed: `approved_at` is stamped only by user Approve on the design track.
- A started task's non-empty `acceptance_criteria` become the session goal (`GoalManager`); the goal clears on complete or skip (a failed task keeps it for the retry).
- Completing the last task archives `.json` + `.md` under `<session_dir>/archive/` and returns the session to NoPlan.

### User language → mode

When the user says **plan**, infer **design track** (Editing, session `.md`, wait for Approve).

**A3 — without “plan”:** execute-shaped multi-step (“implement”, “fix”, “refactor”, “audit and fix”) → proactive **checklist** guidance before project writes. Pure Q&A/research → no forced plan tool. Ambiguous → ask once.

**Soft-nudge (Cluster A / locked grill item, TUI + Telegram):** full shared [src/utils/prompt_analyzer.rs](src/utils/prompt_analyzer.rs) on both surfaces (LLM-only hints). Phase 1 ships live-ops plan hints (`init` / `add_task` / `start`); Phase 6 swaps to design-track + may set durable `pre_init_editing`. **Skip slash/skill expansions on both surfaces.** Telegram still has `/plan` + flow chrome alongside the analyzer. Do not confuse this with Cluster C Phase C1 (status/gate engine work).

**Compaction:** auto-compaction summary must include plan state (Editing/Active/NoPlan, `.md` path, next action) whenever session plan files exist; recovery prompt branches match that state.

### Spawn agent types

- **`architect`** is the preferred name in spawn/team enums and documentation for architecture planning work.
- **`plan`** remains a valid alias and resolves to the same agent type as `architect` in `agent_type.rs`. Do not remove it.

### Telegram UI vs the OC Dev shared doc

Plan mode on Telegram extends the existing per-turn **flow message** in `flow.rs` (tools, clock, intermediates, plus plan sections). It does **not** add a separate Bot API pinned message. Approve/Discard ride the latest flow message as inline buttons (`plan:ok` / `plan:no`, a prefix deliberately distinct from tool-approval `approve:{id}`), owner-only in groups; used buttons clear their markup, and the flow tick re-attaches the keyboard while the state still calls for one.

**Entry asymmetry (intentional):** hint injection is shared (the PromptAnalyzer design-track nudge sets durable `pre_init_editing` on plan keywords on BOTH surfaces), but Telegram keeps `/plan` and flow chrome as the primary entry while the TUI leans more on silent keywords plus the `/show-plan` overlay.

The `opencrabs-plan-mode.md` file shared in OC Dev predates this flow-message design, still describes pin-based chrome, and is **not** source of truth — the umbrella plan and the cluster ADRs are.

### Pre-init Editing and `/plan`

- **`/plan` and soft-nudge (shared analyzer)** set durable **`pre_init_editing`** on a **minimal JSON sidecar** (survives restart; no `.md` until `plan init`). Not stored on per-turn `AgentContext`. TUI mirrors for UI; Telegram reads the same sidecar.
- **`/discard`** clears pre-init flag; post-init deletes `.md` + `.json`.
- **`/execute` / Approve:** **forbidden while a turn is running**; when idle — first approve from Editing, or seed-retry from Active with empty `tasks`.

### Session plan `.md` (design track, light template B)

While Editing, the agent writes `<session_dir>/.opencrabs_plan_{session}.md`:

```markdown
# {title}

## Context
- **Problem:** {one sentence}
- **Target state:** {one sentence}
- **Intent:** {one sentence}
- **Constraints:** {optional}
- **Out of scope:** {optional}

## Implementation steps
1. {Step title} — {description}
2. {Step title} — {description}
   - Done when: {optional acceptance criterion}
```

- **Approve gate:** non-empty Problem, Target state, Intent; ≥1 numbered step under Implementation steps.
- **Seed turn:** model maps numbered steps → `add_tasks` (1:1); optional `Done when:` → `acceptance_criteria`; **dependencies omitted by default** unless prose requires ordering.
- **Auto-start:** seed turn calls `start` immediately after `add_tasks`.
- Full `.md` body syncs to JSON `description`; `tasks` stay empty until Approve seed.

See [src/docs/reference/plans/plan-mode/umbrella.md](umbrella.md) for full rules.
