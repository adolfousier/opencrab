# ADR 0004 — Plan mode UX (commands, prompts, surfaces)

- **Status:** Accepted
- **Cluster:** D — Plan mode UX
- **Date:** 2026-07-12
- **Umbrella:** [src/docs/reference/plans/plan-mode/umbrella.md](umbrella.md)
- **Umbrella phases:** 6 (commands / validator / seed / design-track soft-nudge), 7 (prompts / compaction), 8 (TUI + Telegram surfaces + product docs)

## Context

Cluster C gives the engine: NoPlan / Editing / Active, write and bash gates, session `.md` mirror, and the `plan` tool contract (`mode`, `add_tasks`, no auto-approve on first `start`). Users still lack the product surface that makes that engine usable end-to-end: `/plan`, Approve, seed-from-prose, design-track soft-nudge, dual-track prompts, TUI Mission Control, and Telegram Approve/Discard plus Editing prose on the flow message.

Soft-nudge from Cluster A still teaches checklist-shaped ops for “make a plan…”. That window was intentional so dead `create` / `finalize` hints died first; this cluster closes it by swapping the plan hint to design-track and setting durable `pre_init_editing` on plan-keyword match. Compaction and preamble still fight design-track Editing (“NEVER text plans”, Active-only recovery that says `start`). Telegram tool callbacks already use `approve:{id}`; Plan Approve must not collide. Cluster B already owns flow identity plus **title / goal / checklist** sections when live JSON exists — this cluster consumes that builder and attaches Approve, Discard, Editing prose, and Building checklist… chrome on top.

This cluster is the **user-visible Plan mode product** on TUI and Telegram.

## Decision

Ship channel commands, the Approve validator, a visible seed turn that auto-starts task 1, design-track analyzer swap plus durable `pre_init_editing` on plan-keyword match, full dual-track prompts and compaction, TUI Mission Control overlay plus Active strip, and Telegram **Approve/Discard + Editing prose** on the **latest** flow message. Reuse Cluster B’s section builder for title/goal/checklist; do not reimplement those sections here.

Approve and `/execute` are **owner-only** for Telegram group callbacks, and their callback data must use a prefix distinct from tool-approval `approve:{id}` (for example `plan:ok` / `plan:no`). While a turn is running, `/execute` and Approve are **forbidden** — refuse immediately; never queue into mid-turn inject. Seed recovery is split: **empty `tasks`** allows idle retry; **partial `tasks` without successful `start`** requires `/discard` then re-approve in v1.

Depends **hard** on Cluster C. Softly reuses Cluster A’s analyzer (hint swap only) and Cluster B’s flow section builder / ctx / title / goal / checklist.

## Considered options

| Option | Outcome |
|---|---|
| Bot API pin for plan chrome | Rejected — extend the per-turn flow message instead |
| Queue `/execute` while busy | Rejected — forbidden; refuse immediately |
| Second Approve after seed builds checklist | Rejected — user Approves prose only; seed auto-starts task 1 |
| Any group member may Approve / Discard | Rejected — owner-only, same spirit as sensitive tool-approval |
| Idle repair of partial seed without `/discard` | Deferred to v2 — v1 stays discard-only |
| Reuse Telegram flow chrome 1:1 on TUI | Rejected — TUI uses Mission Control overlay + strip |
| Callback data `approve:{id}` for Plan Approve | Rejected — must not collide with tool-approval callbacks |

## Consequences

Design track becomes usable end-to-end: `/plan` or soft-nudge → `init mode=design` → edit `.md` → Approve → seed → Active checklist. Soft-nudge finally matches Product language (plan → design). Partial seed failure remains `/discard`-only in v1. Product README and memory-bank alignment for the full Plan mode story ship with this cluster. Entry UX may still differ by surface (Telegram keeps `/plan` and flow chrome; TUI may lean more on silent keywords); hint injection is shared and that asymmetry is documented here.

## Normative command matrix

These behaviors are locked for both TUI and Telegram unless a row says otherwise. Classify `/execute` and plan Approve callbacks as **refuse-while-busy** — never enqueue into mid-turn inject. `/plan` and `/show-plan` may still follow the channel’s normal busy/queue rules when those differ. `/discard` cancels the in-flight turn when needed, then cleans up.

### `/plan`

Sets durable pre-init Editing (toolset, prompt, and gate). It does **not** create an approvable plan by itself; the agent must still call `plan init`. Until `init` succeeds, Approve and `/execute` stay refused. There is no separate `/cancel` in v1 — exit without `init` uses `/discard` (and the TUI equivalent).

### Soft-nudge (shared PromptAnalyzer — Phase D1 swap)

Cluster A already runs the full analyzer on both surfaces for natural-language chat only, with LLM-only hints and slash/skill skip. Phase D1 **swaps** the plan-keyword hint from live-ops checklist shape to design-track (`init mode=design`, write SESSION PLAN `.md`, wait for Approve) and **sets durable `pre_init_editing`** when plan keywords match. Keyword triggers stay; do not trap execute-shaped-only messages (brain preamble A3 still owns that split). Soft-nudge complements `/plan`; it does not replace A3 for execute-shaped work without the word “plan.” Skip slash and skill expansions remain required on both surfaces (already wired in A).

### `/show-plan`

On Telegram, rebuilds or resticks the latest flow message’s plan chrome when a plan exists. On TUI, opens or reloads the Mission Control overlay or Active strip. In NoPlan or pre-init it reports that there is no active plan (or that Plan mode still needs `plan init`).

### `/execute` and Approve (idle paths)

When idle, two paths are allowed:

1. **First approve** (Editing post-init): `init` is done, JSON status is `"Editing"`, and the `.md` passes the Approve validator → set `"Active"` and `approved_at` → freeze `.md` → dispatch the seed turn.
2. **Seed retry** (Active, seed failed with empty `tasks`): status is `"Active"`, `tasks` is still empty, and seed-error chrome is shown → re-dispatch the seed turn only (no second approve, no status transition). The harness may re-run the validator on the frozen `.md`.

Otherwise refuse: pre-init, validator failure, Active with non-empty `tasks` (`/execute` is not applicable — continue the checklist), or NoPlan.

While a turn is in progress, `/execute` and Approve (including the Telegram Approve button) are **forbidden** — refuse with a clear message; never queue.

### `/discard`

| State | Behavior |
|---|---|
| NoPlan | Not applicable |
| Editing pre-init | Clears durable flag / deletes minimal sidecar → NoPlan |
| Editing post-init / Active | Cancel turn if needed; delete plan artifacts (`.md` and `.json`) → NoPlan |

### Busy / idle summary (design-track Approve and `/execute`)

| State | On disk | Harness flag | `/execute` / Approve when idle | `/execute` when busy |
|---|---|---|---|---|
| NoPlan | none | off | refuse | forbidden |
| Editing pre-init | minimal JSON flag only (no `.md`) | on (durable) | refuse | forbidden |
| Editing post-init | `.md` + `.json`, `tasks` empty | on / derived from files | first approve if template valid | forbidden |
| Active (seed failed, empty `tasks`) | `.md` frozen, `tasks` empty | off | seed retry if template valid | forbidden |
| Active (checklist) | `.md` + `.json`, `tasks` non-empty | off | refuse (not applicable) | forbidden |

## Approve validator (strictness locked)

The Phase D1 Approve gate is a **lightweight Rust scan** of headings and field labels on the session `.md`, not a full markdown AST. Seed extraction stays model-driven. That asymmetry is deliberate: a deterministic gate plus LLM mapping.

Refuse Approve and `/execute` when any of the following hold:

- The `.md` is empty.
- `## Context` is missing, or **Problem**, **Target state**, or **Intent** are blank (whitespace-only counts as blank).
- `## Implementation steps` is missing or has zero numbered steps.

Strictness is locked: **any non-empty text after the label is enough** — no placeholder heuristics such as “TBD.” Constraints and Out of scope remain optional. Acceptance criteria (`Done when:`) are **not** required for Approve or seed. Optional sections (`## Risks`, `## Open questions`, `## Notes`) are ignored for the gate and for seed extraction.

The light template B outline itself is owned by Cluster C / [src/docs/reference/plans/plan-mode/json-spec.md](json-spec.md); this cluster consumes it as the validator and seed contract.

## Checklist seed from prose (design track)

After user Approve on Track A, a synthetic implement turn converts the approved `.md` into JSON `tasks[]`. Extraction is model-driven, not a Rust markdown parser; the light template makes one tool call reliable enough.

### Sequence (locked)

1. Harness runs the Approve validator on the `.md`.
2. On pass: set Active, record `approved_at` (first approve only), freeze the `.md`.
3. Dispatch **one visible** agent turn with a synthetic user message carrying the locked implement-turn prompt so history and compaction can recover the intent.
4. Model reads the session `.md`, calls `add_tasks` once with the full array from `## Implementation steps` (1:1 with numbered steps), calls `start` on task 1, and continues normal Active execution in the **same** turn.

Transition to Active **before** the seed turn’s tool loop so `add_tasks` is not blocked by Editing rules.

### Implement-turn prompt intent (locked)

The approved SESSION PLAN is at `{absolute_md_path}`. Read `## Implementation steps`. Emit exactly one `add_tasks` with all tasks (1:1 with numbered steps). Map `Done when:` bullets to `acceptance_criteria` when present. Omit dependencies unless step prose explicitly requires ordering (depends on / after / blocked by). Then call `start`. Do **not** edit project files until `start` succeeds.

### Seed tool policy (locked)

Allow reads and `plan` (`add_tasks`, `start`). Deny project writes, bash, spawn, and other mutators until `start` succeeds — same spirit as the Editing gate, but with Active status.

### Auto-start (locked)

After successful `add_tasks`, the seed turn must call `start` immediately. The user Approves **prose only** — there is no second Approve or “review checklist before go” step. The checklist appears in the flow message or TUI strip as soon as seed completes, while task 1 execution begins in the same turn.

### Visibility

The seed turn is a normal visible turn: clock, tools, and intermediates appear in the Telegram flow message as today. Ctx footer stays on the flow message only, never on the final answer.

### Empty vs partial seed recovery (locked)

| Outcome | Keep status | Chrome | Recovery |
|---|---|---|---|
| Seed ends with **empty** `tasks` | Active | Error in status chrome; hide Building checklist… | Idle `/execute` or Approve retry only; may re-validate frozen `.md`; do not auto-start project writes |
| `tasks` **partially** populated without successful `start` | failed seed | Error chrome | **`/discard` then re-approve** in v1 — no idle repair |
| Seed succeeds (`tasks` non-empty + `start`) | Active | Checklist chrome | Continue checklist |

Checklist track and workflow import (Track C) **skip** this seed step because tasks already exist in JSON. Track C import has no seed turn.

### Building checklist… machine (locked)

Show **Building checklist…** while the session is Active, the seed turn is in flight, and `tasks` is still empty. Hide it when `tasks` becomes non-empty **or** when the seed turn ends with an error (show the error in chrome instead). The same machine applies to the TUI strip and the Telegram flow plan section.

## Phases

### Phase D1 — Commands, validator, seed, design-track soft-nudge (was umbrella Phase 6)

Wire `/plan`, `/show-plan`, `/execute`, `/discard` on TUI and Telegram per the command matrix above. Implement the Approve validator (Rust `.md` scan for Context labels + numbered steps). On idle Approve/`/execute` after pass: set Active + `approved_at` (first approve), dispatch the visible seed turn with synthetic user message, seed tool policy, `add_tasks` then `start`. Wire empty-tasks retry versus partial-seed `/discard` recovery. `/execute` and Approve while busy are forbidden (never queue).

Swap Cluster A’s plan hint to design-track and set durable `pre_init_editing` on plan-keyword match. Skip slash/skill expansions remain in force (already required in A). Ship enough [src/brain/prompt_builder.rs](../../../../brain/prompt_builder.rs) changes to stop “NEVER text plans” from fighting design track before Approve/seed E2E; the full prompt rewrite can wait for Phase D2. No CLI REPL slashes.

Telegram Approve/Discard callbacks: **owner-only** in groups (same spirit as sensitive tool-approval); non-owners get a short refuse (toast or ephemeral ack) and plan state does not change; DMs with the owner are unaffected. Callback data must **not** use tool `approve:{id}` — use a distinct prefix (for example `plan:ok` / `plan:no`).

Use the golden session-plan fixture (below) for Approve/seed tests where helpful.

**Done when:**

- Commands are wired on TUI and Telegram.
- `/execute`/Approve busy refuse works (never queues).
- First approve + empty-tasks seed-retry paths work.
- Partial seed requires `/discard` then re-approve.
- Pre-init discard works.
- Validator and visible seed work (auto-start task 1).
- Analyzer plan hint is design-track on both surfaces; plan keywords set durable `pre_init_editing`.
- Track C import has no seed turn.
- Group Approve/Discard is owner-only; plan callbacks use a prefix distinct from tool `approve:{id}`.

### Phase D2 — Prompts and compaction (was umbrella Phase 7)

Implement Prompt policy in full. In prompt copy, SESSION PLAN means design prose in the session `.md` (Editing only). CHECKLIST means executable `tasks[]` in JSON (Active only). Plan mode is the product feature spanning design → Approve → execute, or checklist/import straight to Active. Do not paste a plan in chat; while Editing, write the SESSION PLAN to the `.md` path using light template B.

#### Track selection (locked)

| User signal | Track | Model action |
|---|---|---|
| Says plan / design / review / approve-first | Design | Editing, `init mode=design`, write `.md`, wait for Approve |
| Execute-shaped multi-step, no “plan” word | Checklist (proactive) | `init` + tasks → Active → `start` before project writes |
| User supplies task list | Checklist | `init` with tasks (or import) |
| Pure Q&A, research, single-step fix | None | No forced plan tool |
| Ambiguous | Ask | One question: design first or checklist? |
| Yolo + explicit `/plan` slash | Design (review gate) | Editing, `init mode=design`, write `.md`, wait for Approve; `/execute` resumes full-rush |
| Yolo / cron / `run` / a2a + design ask (no slash) | Refuse design | Checklist or import only |

A3 keeps the useful “don’t wing a large refactor” guardrail for execute-shaped work, but does not force `plan init` on every multi-step question. Audit-only requests stay out of Plan mode unless the user says plan or asks for a design. When design language and execute-shaped language coexist, design wins when “plan” is explicit; otherwise execute-shaped cues favor checklist.

#### Harness injections

- Each Editing turn gets a new Editing reminder: absolute `.md` path, light-template Context and Implementation steps required before Approve, no `start` / `complete` / bash / project writes, wait for Approve (`format_editing_reminder`).
- The existing Active reminder (`format_plan_reminder`) keeps today’s copy and checklist nudge, with status matchers already updated to Active in Cluster C. “Unchanged Active reminder” means wording and checklist-nudge behavior, not legacy enum variants.
- Compaction recovery branches on session plan state: omit plan recovery for NoPlan; for Editing, re-read `.md` and do **not** `start` or edit project files; for Active, call `plan start` (no args) to resurface the task, then continue the checklist.
- Whenever a session plan artifact exists (`.md` and/or live `.json`), the auto-compaction continuation summary must include plan state (NoPlan / Editing / Active), absolute `.md` path if present, and one line on what to do next (edit `.md` versus run checklist). Recovery prompt text alone is not enough — the summary is what survives after compaction clears history.

Replace `CRITICAL: PLAN TOOL USAGE` and “NEVER text plans” with SESSION PLAN versus CHECKLIST rules and the track-selection table. Align [src/brain/tools/plan_tool.rs](../../../../brain/tools/plan_tool.rs) `description()` with the preamble. Remove `task_manager` from catalog and preamble (deprecated; new work uses `mode=checklist`). Prefer `add_tasks` in tool copy once stable; mark `add_task` deprecation in docs only after that switch. Add `architect` to spawn and team JSON enums where docs already prefer that name; keep `plan` as a valid alias (do not remove it) — both already map to the same agent type in [src/brain/tools/subagent/agent_type.rs](../../../../brain/tools/subagent/agent_type.rs).

Surfaces to update include [src/utils/prompt_analyzer.rs](../../../../utils/prompt_analyzer.rs) (already swapped in D1), [src/brain/prompt_builder.rs](../../../../brain/prompt_builder.rs), [src/brain/tools/plan_tool.rs](../../../../brain/tools/plan_tool.rs), and [src/brain/agent/service/compaction_prompts.rs](../../../../brain/agent/service/compaction_prompts.rs).

**Done when:**

- Track selection matches the locked table.
- Editing recovery never says `start`.
- Active recovery still drives the checklist.
- “Plan” in user text routes to design; execute-shaped multi-step without “plan” still triggers proactive checklist guidance.
- Compaction summary includes plan state when artifacts exist.
- `add_tasks` is primary in tool copy.
- Spawn/team enums accept `architect` with `plan` as alias.
- `task_manager` is gone from catalog and preamble.

### Phase D3 — TUI and Telegram surfaces (was umbrella Phase 8)

#### TUI

Ship Mission Control–style overlay while Editing: scrollable `.md` body and a footer Approve/Discard that is **not** tool-policy `/approve`. While Active, show the checklist strip above input, Building checklist… until seed completes (or seed-error chrome), and Discard via `/discard`. `/show-plan` opens or reloads the overlay or strip. Show an Editing or Active badge. Distinguish pre-init versus post-init chrome (pre-init has no Approve and no approvable `.md`). Rely on Cluster C’s `send_message` / `reload_plan` arms so Editing is not cleared and idle Active (including empty-task seed failure) is not deleted — do not re-implement that enum collapse here. Make `discard_plan_file` delete both `.md` and `.json` when wiring Discard. UX review with Adolfo ([#12656](https://t.me/c/3627148483/12656)) before calling done — do not only wire the existing `plan_widget`.

#### Telegram

Attach **Approve/Discard** and **Editing prose** (session `.md` body, collapsed when appropriate) to the **latest** flow message built in Cluster B. Title / goal / checklist sections already come from B — refresh them through the same section builder; do not duplicate that work. If Cluster B did not ship, also migrate ctx onto the flow (fallback).

Keyboards attach only after `init` succeeds. While Editing, Approve and Discard sit on the latest flow message (live or settled), with Approve refused while a turn is running. While Active, Discard only. Strip `reply_markup` from previous flow messages when a newer message becomes the keyboard owner. Before `init`, there is no plan keyboard. Optional flow-message id on JSON helps restick and keyboard ownership. No Bot API pin.

Collapse and always-visible **title / goal / checklist / ctx** chrome follow Cluster B (#480 option 1, 1.5s `refresh_flow`). This cluster only adds Editing-prose collapse policy and Approve/Discard attachment: expand-on-demand may hold full `.md` prose (plus B’s checklist lines and goal criteria). Telegram Editing has **no** Mission Control overlay (locked default) — collapsed plan summary in the flow header, full `.md` via expand or `/show-plan` restick.

Wire **Building checklist…** / seed-error chrome into the flow plan section for the locked seed machine. Owner-only Approve/Discard and distinct callback prefix land here if not already complete from D1 wiring. Final answer stays clean prose with no ctx footer (B moved ctx; do not re-attach it).

**Done when:**

- Telegram Approve/Discard + Editing prose work on the latest flow message.
- Title/goal/checklist continue to render via B’s builder (or are present if B shipped in the same release).
- Ctx is on flow only.
- Building checklist… matches the locked machine on both surfaces.
- TUI overlay/strip are UX-reviewed with Adolfo.
- Pre-init discard works on both surfaces.
- Docs and cleanup for this slice are done (see below).

### Docs and cleanup (ship with D, finish with D3)

Update product README with the dual-track Plan mode user story (design → Approve → seed; checklist/import → Active). Finish [src/docs/reference/plans/plan-mode/json-spec.md](json-spec.md) design decisions that are UX-facing: flow message versus pin (title/goal/checklist already documented under Cluster B), spawn `architect` with `plan` as alias, and `add_task` deprecation after Phase D2 prefers `add_tasks`.

Document shared analyzer plus intentional **entry asymmetry** (`/plan` versus keywords): hint injection is shared; Telegram keeps `/plan` and flow chrome while TUI may lean more on silent keywords.

Record that the OC Dev pin-era `opencrabs-plan-mode.md` is **not** source of truth — the umbrella plan and these ADRs are. Remove dead pin paths, stale `task_manager` references (with D2), and leftover Approve/keyboard comments that describe Bot API pin. Complete the `approved_at` consumer audit once Approve sets the field (Cluster C grepped readers after auto-approve-on-start died; this cluster finishes the audit when the field is actually set). Update CHANGELOG and memory bank / CLAUDE.md for commands, seed, overlays, and Telegram Approve/prose.

## Golden session-plan fixture (for tests)

Use this fixture for Approve validator and seed-turn tests:

```markdown
# Add session plan Approve gate

## Context
- **Problem:** Plan mode has no structured design doc before execution.
- **Target state:** Users Approve a `.md` plan, then checklist runs automatically.
- **Intent:** Ship Track A for TUI and Telegram in v1.

## Implementation steps
1. Add Approve validator — implement Rust scan in Phase 6 harness.
2. Wire seed turn — synthetic user message, add_tasks, start.
   - Done when: empty tasks triggers idle retry via /execute (forbidden while turn running).
```

## Verification (Phases 6–8)

- **Phase D1 / 6:** Approve validator; visible seed turn; `/execute` forbidden when busy; idle seed-retry for empty `tasks`; partial seed requires `/discard`; discard pre-init and post-init; design-track analyzer swap on both surfaces; owner-only group callbacks; callback prefix ≠ `approve:{id}`.
- **Phase D2 / 7:** Compaction summary includes plan state; recovery branches by state; design-track preamble complete; `task_manager` removed; `architect` enum with `plan` alias; `add_tasks` primary in tool copy.
- **Phase D3 / 8:** Telegram Approve/Discard + Editing prose; title/goal/checklist via Cluster B; ctx on flow only; Building checklist… machine; TUI overlay/strip + Adolfo UX review (C’s enum arms already hold); product README and memory bank for Plan mode.

## Deviation policy (owned here for UX paths)

If status or flow send/edit fails, keep the `.md` as source of truth, surface the error, and let `/show-plan` retry. Do not block Planning on UI success.

If the seed turn finishes with empty `tasks`, keep Active, surface the error, and allow idle `/execute` retry only; refuse `/execute` while a turn is running. If `tasks` is partial without a successful `start`, show an error and require `/discard` to recover in v1.

When attaching plan keyboards to the latest flow message, clear `reply_markup` on any previously tracked flow message ids for that chat or session.

Do not re-spec clock, tools, intermediates, or Cluster B’s title/goal/checklist/ctx chrome — match existing flow behavior and only add Approve, Editing prose, and Building checklist… around it. Do not change the 1.5s refresh cadence to preserve expand state (locked option 1, #480, owned with B). Over budget, use existing freeze or split only.

Do not add checklist items in Editing, revive `propose_plan`, reintroduce Telegram pin as a second surface, or expand v1 channels without a scope change. If a lifecycle or security contradiction appears, stop and ask.

## Out of scope for this cluster

Engine status/tool work (Cluster C). Analyzer extract/live-ops (Cluster A) except the Phase D1 hint swap and `pre_init_editing` on plan keywords. Flow pre-block merge, ctx-on-flow, and title/goal/checklist sections (Cluster B) except consuming B’s section builder and attaching Approve/prose/Building checklist… on top. CLI plan slash commands and Discord, Slack, WhatsApp, and Trello remain deferred for v1.

## Implementation notes

Umbrella sections Commands, Checklist seed, Soft-nudge (Phases 1 and 6 / Clusters A and D1), Prompt policy, Surfaces (TUI + Telegram Approve parts), Building checklist… machine, Deviation policy (busy forbid, partial discard), Verification Phases 6–8, and the golden session-plan fixture are normative for this ADR. Prefer day-to-day execution from this document; keep the Cursor umbrella plan as the full product-model SSOT until implementation finishes.
