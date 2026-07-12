# ADR 0002 — Telegram flow chrome merge

- **Status:** Accepted
- **Cluster:** B — Telegram flow chrome (umbrella **Phase 2**)
- **Date:** 2026-07-12
- **Umbrella:** [src/docs/reference/plans/plan-mode/umbrella.md](umbrella.md)
- **Index:** [src/docs/reference/plans/plan-mode/README.md](README.md)

## Context

A Telegram turn today can show two status surfaces. Until the first tool call or folded intermediate arrives, the bot posts a separate pre-block bubble (`status_msg_id`) with thinking or Working-on text. When the Processing log opens (`open_group_msg_id`), that bubble is deleted in the edit loop — not inside `open_flow` — and users watch status jump from one message to another. Plan title, checklist, goals, and the ctx footer cannot safely land on the pre-block because it disappears the moment the flow opens.

The ctx footer today rides on the final answer (and related delivery paths), so the reply is polluted with session metadata that belongs on live chrome. Resumed sessions never create `status_msg_id`, but they also lack the main handler’s processing-driven activity ticks and live-duration `refresh_flow` while tools run, so resume turns look “dead” relative to a normal turn.

Empty `flow_entries` currently make the renderers return an empty string, so `open_flow` no-ops. Header-only messages — activity, thinking, Working-on, duration, and later plan or ctx summaries without a tool body — are impossible until that early return is fixed.

Cluster D will later attach Approve/Discard keyboards and Editing `.md` prose to the same flow message. That work is unsafe while a disposable pre-block bubble still owns early-turn status, and while title / goal / checklist have no section home on the flow. This cluster deliberately moves **title / goal / checklist** onto the flow from live JSON and `GoalManager` now, and leaves prose plus Approve chrome for D.

This cluster ships early because it delivers one status surface, ctx on flow, clean final answers, and live plan sections from existing JSON with low product risk. It removes the dual-status footgun before Approve/prose land, and it builds the shared section renderer Phase 8 (Cluster D) extends. Phase 2 has **no functional dependency** on Cluster A or on the Editing/Active lifecycle in Cluster C; it does not replace Phases 3–7.

## Vocabulary (Telegram chrome)

Prefer **flow message** over “living UI” or “session-status.” Identity stays one new flow message per turn; do not reuse `open_group_msg_id` across turns.

| Plan / product term | Code symbol | Lifetime / role after this cluster |
|---|---|---|
| Flow message (Processing log) | `open_group_msg_id` | Per turn; the only live turn chrome — tools, intermediates, activity header, plan and ctx sections |
| Status message (header line) | `latest_activity_preview()` → `flow_header_text` | Flow header: `⚙️ {activity} • {count} • {duration}`; also hosts thinking / Working-on before tools |
| Pre-block status bubble (legacy) | `status_msg_id` | Removed here — today ephemeral; deleted in the edit loop when a block is open or the turn ends |
| Streaming placeholder | `msg_id` + `▋` | Separate bottom bubble until final delivery when needed; answer streaming, not the pre-block status bubble |
| Duration in header | `StreamingState.flow_status` | Misleading field name — stores the elapsed string only |

A **checklist** is the JSON `tasks[]` array. Prompts and UI must not call the checklist “the plan.” Plan **title** and incomplete checklist progress come from the live session JSON; the **active goal** line comes from [src/brain/goal/manager.rs](../../../../brain/goal/manager.rs) when a session goal is set.

## Decision

Merge pre-flow into the flow message: **one live message per turn**. On the first activity tick (about 1.5s, same cadence as today), or earlier if a tool arrives first, call `open_flow` and put thinking or Working-on preview in the flow header via the same `flow_header_text` path used for tool activity.

**Header-only render is locked.** All three `render_flow_*` paths (classic HTML, rich details, rich markdown) and `open_flow` / `refresh_flow` must allow empty `flow_entries`: emit a header (activity, thinking, Working-on, duration, and section summaries such as ctx or checklist progress) without requiring a body. Today an empty body returns `""` and `open_flow` no-ops — confirm and fix the early returns around [src/channels/telegram/flow.rs](../../../../channels/telegram/flow.rs) ~331–333 and ~946–948 before coding. Settled no-tool turns still get a flow message (`✅ Finished` plus header chrome).

Do not create, edit, or delete `status_msg_id`. Remove that field and the delete-when-block-open path from [src/channels/telegram/handler.rs](../../../../channels/telegram/handler.rs), [src/channels/telegram/flow.rs](../../../../channels/telegram/flow.rs), and [src/channels/telegram/resume.rs](../../../../channels/telegram/resume.rs) once the merge ships. Subsequent ticks only `refresh_flow` the same message (duration, activity, tools, ctx, checklist).

Move the **ctx footer** onto the flow message and off the final answer. Ctx is display-only — not DB or TTS. Wire the move in [src/channels/telegram/delivery.rs](../../../../channels/telegram/delivery.rs) and the flow renderer so the final answer stays clean prose.

Port the same **open-early + live duration `refresh_flow`** path into [src/channels/telegram/resume.rs](../../../../channels/telegram/resume.rs). Today resume never creates `status_msg_id` but also lacks processing-driven ticks and duration refresh while tools run — do not treat “delete an unused field” as enough.

Ship a **shared flow section builder** that renders, when present and non-empty (omit empty sections):

- **Plan title** from live `.opencrabs_plan_{session}.json` when the title is set
- **Active goal** one-liner from `GoalManager` when a session goal is set
- **Incomplete checklist** with progress from JSON `tasks[]` (read-only mirror of today’s live plan data — for example `2/7` in outer chrome)

Do not require Editing/Active enum migration for this cluster; read today’s JSON as-is (including legacy Active / InProgress shapes). Prefer one section builder over three divergent HTML/details/markdown copies, with thin format wrappers if needed.

Keep the **streaming answer placeholder** (`▋`) unless it clearly duplicates the flow header after the merge. It is answer streaming, not the pre-block status bubble, and stays on the kill list only if the duplication is obvious after ship. Do not change the **1.5s refresh cadence** (#480 collapse reset remains accepted). Keep **restick when buried** (#451), freeze or split when over budget, and settled headers (`✅ Finished` / failed / timed out) at turn end — match existing clock, tools, and intermediates behavior; only add plan, goal, checklist, and ctx chrome around them.

For plan sections that refresh on clock ticks, prefer a **collapsed checklist body** with title and progress in outer chrome (summary or header), not inside expanded bodies. Expand-on-demand may hold full checklist lines and goal criteria; those may re-collapse on the next duration tick, and that is expected under #480. Always-visible chrome after this cluster is plan title, checklist progress, active goal one-liner, and ctx footer. Full Editing `.md` prose collapse policy belongs to Cluster D; this cluster only establishes the collapsed-body-versus-chrome pattern for title / goal / checklist.

This cluster does **not** implement Plan mode lifecycle, `/plan` slashes, Approve/Discard keyboards, Editing `.md` prose sections, `.md` templates, the Building checklist… seed machine, tool `mode`, status enum migration, or the TUI overlay (those stay Cluster D or C — see Out of scope).

### Turn timeline after the merge

1. Typing indicator as today.
2. First activity opens the flow message with thinking, Working-on, or tool activity plus duration (header-only if no tools yet).
3. Tools and intermediates fold into that message with `refresh_flow` every 1.5s.
4. Optional streaming placeholder at the bottom if the model streams before or during the block.
5. Turn end delivers a final answer **without** ctx, settles the flow header, and keeps ctx on the settled flow message.

No-tool turns that last about 1.5s or more open a header-only Processing log and settle it — same path as tool turns, without requiring tools first.

### Collapse and restick (locked)

Refresh and collapse policy is locked to **option 1 (#480)**: keep the 1.5s `refresh_flow` loop and live duration in the header via `humanize_duration` / `turn_started_at`. Accept that Telegram clients (especially Desktop) may reset expand or collapse state on any edit — the client replaces the full message ([src/channels/telegram/flow.rs](../../../../channels/telegram/flow.rs) around 514–519), so we cannot patch header-only. Live clock beats stable expand. Plan mode must not rely on users keeping plan sections expanded during ticks.

Rejected alternatives for this cluster include content-only edits with a stale clock, a split header/body message pair, and `is_open` hacks without client expand telemetry.

Restick when buried (#451) continues to apply to the single live flow message. Do not re-introduce Bot API pin as a second Telegram surface beside the flow message — Adolfo’s audit and the umbrella Out of scope both reject pin-era chrome.

### Soft handoff to Cluster D

Cluster D (Phase 8) reuses this cluster’s flow message and **title / goal / checklist** sections, then adds Editing prose and Approve/Discard onto the same message. Opening the flow early for activity does **not** by itself show Approve or plan sections that require `init` — keyboards attach only after `init` succeeds (D). Building checklist… / seed-error chrome also wire into the flow plan section in D. If this cluster somehow did not ship, D would also migrate ctx; after B ships, D must not re-attach ctx to the final answer.

TUI streaming chrome (spinner, `streaming_reasoning`, `active_tool_group` in [src/tui/render/chat.rs](../../../../tui/render/chat.rs)) is a different architecture. Telegram plan chrome does not map 1:1 onto it; TUI plan overlay and strip are separate (`plan_widget.rs`) and stay out of this cluster.

## Considered options

| Option | Outcome |
|---|---|
| Keep pre-block bubble; only attach plan chrome when flow opens | Dual status UX remains; plan sections cannot land early |
| Only open flow when tools appear | Weakens “one live message for every long turn”; pure Q&A stays typing-only |
| Synthetic body line instead of header-only render | Works with today’s empty-check but pollutes the Processing log body |
| Header-only renderers (chosen) | Clean single message; requires renderer + `open_flow` changes first |
| Defer title/goal/checklist to Cluster D | Leaves B without standalone plan visibility; rejected — move them here |
| Kill streaming `▋` in the same ship | Risk of removing answer-stream UX; keep unless it clearly duplicates the merged header |
| Change 1.5s cadence to preserve expand state | Rejected under #480 — live clock beats stable expand |
| Resume: only delete unused `status_msg_id` | Insufficient — resume still lacks open-early and duration refresh |

## Consequences

Telegram turns feel like one coherent processing surface. Final answers stay clean prose. No-tool turns that last about 1.5s or more still open and settle a flow message. Existing Active (or legacy InProgress) JSON plans already show title, goal, and checklist on the flow without waiting for Plan mode UX. Cluster D adds Approve keyboards, Editing prose, and seed chrome onto this section builder. Resume sessions stop looking “dead” relative to the main handler path.

Remaining risks after the merge: the 1.5s refresh may still reset client expand state (accepted under #480 — keep plan progress in header or summary); resume parity is easy to under-ship if someone only deletes the unused field; the three renderers need one shared section builder or they will drift; and ctx must actually leave the final answer in delivery paths, not only appear on the live flow.

## Phases

This cluster is a **single phase** (umbrella Phase 2) with ordered sub-steps inside one ship. Keep the tree buildable and meet done criteria before treating B complete.

### Phase B1 — Header-only render, merge pre-flow, ctx, resume, title/goal/checklist

1. **Fix empty-body early returns.** Change all three `render_flow_*` paths and `open_flow` / `refresh_flow` in [src/channels/telegram/flow.rs](../../../../channels/telegram/flow.rs) so empty `flow_entries` still emit a header (activity / thinking / Working-on / duration / section summaries). Confirm the empty-string returns around ~331–333 (renderer) and ~946–948 (`open_flow` no-op when `html.is_empty()`) before coding.
2. **Open early on the first activity tick.** In [src/channels/telegram/handler.rs](../../../../channels/telegram/handler.rs), on the first activity tick (or earlier if a tool arrives), open the flow message and put thinking / Working-on in the header. Stop creating, editing, or deleting `status_msg_id` (today deletion is in the edit loop when a block is open or the turn ends — remove that path with the field).
3. **Settle no-tool long turns.** Turns with no tools that last about ≥1.5s open a header-only flow and settle it (`✅ Finished` plus header chrome), same path as tool turns.
4. **Move ctx onto the flow.** Move ctx from the final answer onto the flow message in [src/channels/telegram/delivery.rs](../../../../channels/telegram/delivery.rs) and the flow renderer. Final answers have no ctx footer.
5. **Resume parity.** Port open-early + live duration `refresh_flow` into [src/channels/telegram/resume.rs](../../../../channels/telegram/resume.rs) — not merely delete an unused `status_msg_id` field. Resume must share the same live header refresh path as the main handler while tools run.
6. **Shared section builder.** Introduce one shared flow section builder for ctx, **plan title**, **goal**, and **checklist**, with later hooks for Cluster D prose / Approve chrome. Prefer one builder over three divergent HTML/details/markdown copies.
7. **Title / goal / checklist from live data.** When live `.opencrabs_plan_{session}.json` exists: show **title** when set; show **incomplete checklist + progress** when `tasks` has incomplete items; show an **active goals** line from `GoalManager` when a session goal is set. Omit empty sections. Do not require Editing/Active enum migration — read today’s JSON as-is. Prefer collapsed checklist body with title and progress in outer chrome when sections refresh on clock ticks.
8. **Preserve existing flow behavior.** Do not re-spec clock, tools, or intermediates. Keep restick (#451), freeze/split over budget, and the settled outcome headers. Do not change the 1.5s refresh cadence.

**Out of scope for this cluster (soft handoff):**

| Item | Owns it |
|---|---|
| Pre-init harness flag / durable `pre_init_editing` | Cluster C |
| Status enum migration (Editing / Active / NoPlan) | Cluster C |
| Tool `mode` (`design` / `checklist`), `.md` mirror | Cluster C |
| `/plan`, `/show-plan`, `/execute`, `/discard` slashes | Cluster D |
| Approve/Discard keyboards (owner-only in groups) | Cluster D |
| Editing `.md` prose section on the flow | Cluster D |
| Building checklist… seed machine / seed-error chrome | Cluster D |
| TUI Mission Control overlay / Active strip | Cluster D |
| Bot API pin as a second surface | Out of product scope (never) |

**Done when:**

- Header-only `open_flow` / `refresh_flow` works with empty entries.
- No `status_msg_id` bubble appears in normal or resume turns.
- Activity and thinking appear only on the flow message.
- No-tool long turns (≥ ~1.5s) open and settle flow.
- Ctx is on the flow message only; final answers have no ctx.
- Plan **title**, **goal**, and **checklist** sections render from live JSON / GoalManager when data exists; empty sections are omitted.
- Shared section builder is in place and extensible for Cluster D Approve/prose.
- Tools, intermediates, and restick do not regress.
- Streaming `▋` still works unless deliberately removed after proving header duplication.
- Docs and cleanup for this slice are done (see below).

### Docs and cleanup (ship with B1)

Document that Telegram turn chrome is the **per-turn flow message**, not a Bot API pin and not a separate pre-block status bubble. Document that title / goal / checklist live on that flow message from this cluster onward. Update channel/Telegram docs and comments that still describe `status_msg_id` or dual-status UX. Add a CHANGELOG entry for the merge, header-only render, ctx-on-flow, resume parity, and plan sections. Touch memory bank / CLAUDE.md only where they describe Telegram processing chrome. Product README dual-track story and pin-era OC Dev note cleanup stay with Cluster D; this cluster only corrects chrome vocabulary (flow ≠ pin, no pre-block bubble).

### Verification (umbrella Phase 2)

Confirm in implementation review or manual Telegram checks:

- Merge pre-flow into flow without `status_msg_id`.
- Header-only open works.
- Ctx on flow only; final answer clean.
- Resume on the same open-early + `refresh_flow` path.
- Title / goal / checklist from live JSON when present.
- Section builder extensible for D.
- Flow-chrome docs and CHANGELOG updated.

## Implementation notes

Confirm empty-body early returns around [src/channels/telegram/flow.rs](../../../../channels/telegram/flow.rs) ~331–333 and ~946–948 before coding. Prefer one section builder over three divergent HTML/details/markdown copies. Collapse policy remains #480 option 1 (keep plan progress in header or summary; accept client expand reset on 1.5s refresh). Restick (#451) continues against the single live message.

Primary files:

- [src/channels/telegram/flow.rs](../../../../channels/telegram/flow.rs) — renderers, `open_flow`, `refresh_flow`, `status_msg_id` field removal, section builder
- [src/channels/telegram/handler.rs](../../../../channels/telegram/handler.rs) — activity tick open-early, stop pre-block create/edit/delete
- [src/channels/telegram/delivery.rs](../../../../channels/telegram/delivery.rs) — ctx off final answer, onto flow
- [src/channels/telegram/resume.rs](../../../../channels/telegram/resume.rs) — open-early + live duration refresh parity
- [src/brain/goal/manager.rs](../../../../brain/goal/manager.rs) — active goal line source

Read first: this ADR; umbrella Surfaces / Telegram / Merge pre-flow / flow message vocabulary / Phase 2 / Verification Phase 2; current `status_msg_id` paths in handler and resume; delivery ctx footer append sites.

## Cross-cutting items (belong in README, not this ADR)

The following are B-adjacent but owned by the cluster index or other ADRs — do not expand them into B scope:

- **Cluster map and ship order** (A ∥ B; C → D hard) — [src/docs/reference/plans/plan-mode/README.md](README.md)
- **Vocabulary shared across clusters** (session plan vs checklist vs `plan` tool vs Architect) — umbrella + README
- **Title/goal/checklist moved from D into B** — already reflected in the README cluster map; keep that one-liner there so D authors do not re-implement sections
- **Deviation policy** for flow send/edit failures, over-budget freeze/split, and “do not re-spec clock/tools/intermediates” — umbrella; cite, do not duplicate as B-only rules
- **TUI ≠ Telegram chrome architecture** — mention as out-of-scope here; full TUI UX lives in ADR 0004
