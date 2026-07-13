//! Plan-mode command state machine, shared by TUI and Telegram: `/plan`,
//! `/show-plan`, `/execute` / Approve, and `/discard` semantics over the
//! plan lifecycle engine in [`crate::utils::plan_files`].
//!
//! Surfaces own the busy check (Approve and `/execute` are FORBIDDEN while
//! a turn is running: refuse immediately, never queue) and the dispatch of
//! the visible seed turn; everything idle-path and deterministic lives
//! here so the two surfaces cannot drift.

use crate::tui::plan::PlanStatus;
use crate::utils::plan_files::{self, PlanModeState};
use uuid::Uuid;

/// Result of an idle `/execute` / Approve attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApproveOutcome {
    /// The plan transitioned Editing -> Active (or an empty-tasks seed
    /// retry was accepted): dispatch this synthetic user message as one
    /// VISIBLE agent turn now.
    SeedTurn { prompt: String },
    /// The approve was refused; show this text to the user.
    Refused(String),
}

/// Approve validator (strictness locked): a lightweight scan of the
/// session `.md` headings and field labels. Any non-empty text after a
/// label passes; no placeholder heuristics.
pub fn validate_for_approve(md_body: &str) -> Result<(), String> {
    if md_body.trim().is_empty() {
        return Err("the session plan .md is empty".to_string());
    }
    let warnings = plan_files::template_section_warnings(md_body);
    if warnings.is_empty() {
        Ok(())
    } else {
        Err(warnings.join("; "))
    }
}

/// The locked implement-turn prompt dispatched as a synthetic, visible
/// user message after Approve. History and compaction can recover the
/// intent from it.
fn seed_prompt(md_path: &std::path::Path) -> String {
    format!(
        "[SYSTEM: PLAN APPROVED] The user approved the SESSION PLAN at {}. \
         Read its ## Implementation steps section. Emit exactly ONE `plan` \
         add_tasks call with ALL tasks, 1:1 with the numbered steps. Map \
         'Done when:' bullets to acceptance_criteria when present. Omit \
         dependencies unless step prose explicitly requires ordering \
         (depends on / after / blocked by). Then call `plan` start and \
         continue executing the checklist in this same turn. Do NOT edit \
         project files until start succeeds.",
        md_path.display()
    )
}

/// Idle `/execute` / Approve. Two allowed paths (locked):
///
/// 1. First approve (post-init Editing, `.md` passes the validator):
///    set Active, stamp `approved_at`, return the seed turn.
/// 2. Seed retry (Active, design `.md` present, `tasks` still empty):
///    re-dispatch the seed turn only; no second approve, no transition.
///
/// Everything else refuses with a deterministic message. The caller MUST
/// have already refused when a turn is in flight.
pub async fn try_approve(session_id: Uuid) -> ApproveOutcome {
    let md_path = plan_files::plan_md_path(session_id).await;
    match plan_files::plan_mode_state(session_id).await {
        PlanModeState::NoPlan => ApproveOutcome::Refused(
            "No plan to approve: this session has no live plan. Start one with /plan \
             (design) or ask for a checklist."
                .to_string(),
        ),
        PlanModeState::PreInitEditing => ApproveOutcome::Refused(
            "Nothing approvable yet: Plan mode is waiting for `plan init` to create \
             the design document. Let the agent draft it first (or /discard to leave \
             Plan mode)."
                .to_string(),
        ),
        PlanModeState::PostInitEditing => {
            let body = std::fs::read_to_string(&md_path).unwrap_or_default();
            if let Err(why) = validate_for_approve(&body) {
                return ApproveOutcome::Refused(format!(
                    "Plan not ready to approve: {why}. Fill the template in {} first.",
                    md_path.display()
                ));
            }
            let Some(mut plan) = plan_files::load_plan(session_id).await else {
                return ApproveOutcome::Refused(
                    "Plan JSON is unreadable; cannot approve. /discard and start over.".to_string(),
                );
            };
            // First approve: Editing -> Active + approved_at. The .md
            // freezes automatically (the gate keys off Active status).
            plan.approve();
            if let Err(e) = plan_files::save_plan(&plan).await {
                return ApproveOutcome::Refused(format!(
                    "Failed to persist the approval: {e}. The .md is untouched; try again."
                ));
            }
            ApproveOutcome::SeedTurn {
                prompt: seed_prompt(&md_path),
            }
        }
        PlanModeState::Active => {
            let Some(plan) = plan_files::load_plan(session_id).await else {
                return ApproveOutcome::Refused(
                    "Plan JSON is unreadable; cannot retry. /discard and start over.".to_string(),
                );
            };
            if !plan.tasks.is_empty() {
                return ApproveOutcome::Refused(
                    "The checklist is already Active: /execute is not applicable. \
                     Continue the checklist (or /discard to drop the plan)."
                        .to_string(),
                );
            }
            if !md_path.exists() {
                return ApproveOutcome::Refused(
                    "This checklist plan has no design document to seed from. \
                     Add tasks with the plan tool instead."
                        .to_string(),
                );
            }
            // Empty-tasks seed retry: re-validate the frozen .md, then
            // re-dispatch the seed turn. No status transition.
            let body = std::fs::read_to_string(&md_path).unwrap_or_default();
            if let Err(why) = validate_for_approve(&body) {
                return ApproveOutcome::Refused(format!(
                    "Cannot retry the seed: the frozen plan fails validation ({why}). \
                     /discard and re-plan."
                ));
            }
            ApproveOutcome::SeedTurn {
                prompt: seed_prompt(&md_path),
            }
        }
    }
}

/// `/plan`: set durable pre-init Editing. It does not create an
/// approvable plan; the agent must still call `plan init`.
pub async fn enter_plan_mode(session_id: Uuid) -> String {
    match plan_files::plan_mode_state(session_id).await {
        PlanModeState::PostInitEditing => {
            "A design plan is already being edited. Refine it, then approve with \
             /execute (or /discard it)."
                .to_string()
        }
        PlanModeState::Active => "A checklist is already Active for this session. Continue it, or \
             /discard it before planning something new."
            .to_string(),
        PlanModeState::PreInitEditing | PlanModeState::NoPlan => {
            match plan_files::set_pre_init_editing(session_id).await {
                Ok(()) => "📋 Plan mode on. Describe what you want planned: the agent \
                           will explore, then draft a design document for your approval. \
                           Project writes stay blocked until you approve. Leave with \
                           /discard."
                    .to_string(),
                Err(e) => format!("Could not enter Plan mode: {e}"),
            }
        }
    }
}

/// `/discard`: clear the pre-init flag or delete plan artifacts,
/// returning the session to NoPlan. The caller cancels an in-flight turn
/// first when needed.
pub async fn discard(session_id: Uuid) -> String {
    match plan_files::plan_mode_state(session_id).await {
        PlanModeState::NoPlan => "No live plan to discard.".to_string(),
        PlanModeState::PreInitEditing => {
            plan_files::discard_plan(session_id).await;
            "Plan mode off (pre-init flag cleared).".to_string()
        }
        PlanModeState::PostInitEditing | PlanModeState::Active => {
            plan_files::discard_plan(session_id).await;
            "🗑️ Plan discarded: design document and checklist removed. The session \
             has no live plan."
                .to_string()
        }
    }
}

/// Cap for the design prose echoed by `/show-plan`. Telegram's hard message
/// limit is 4096 chars; leave headroom for the title, ready-line, and the
/// document path rendered around the body.
const PLAN_PROSE_CAP: usize = 3000;

/// `/show-plan`: a text summary of the current plan state. Surfaces may
/// additionally restick chrome (Telegram) or open the overlay (TUI).
pub async fn show_plan(session_id: Uuid) -> String {
    match plan_files::plan_mode_state(session_id).await {
        PlanModeState::NoPlan => "No active plan for this session.".to_string(),
        PlanModeState::PreInitEditing => {
            "Plan mode is on (pre-init): no design document yet. The agent still \
             needs to run `plan init`."
                .to_string()
        }
        PlanModeState::PostInitEditing => {
            let md = plan_files::plan_md_path(session_id).await;
            let body = std::fs::read_to_string(&md).unwrap_or_default();
            let title = plan_files::load_plan(session_id)
                .await
                .map(|p| p.title)
                .unwrap_or_default();
            let ready = match validate_for_approve(&body) {
                Ok(()) => "Ready to approve: /execute.".to_string(),
                Err(why) => format!("Not approvable yet: {why}."),
            };
            // Surface the design prose itself, not just its path: a reviewer on
            // a chat channel has no file access, so the document body IS the
            // thing under review (#540). Truncated to stay inside one chat
            // message; the untruncated text is always on disk at `md`.
            let trimmed = body.trim();
            let prose = if trimmed.is_empty() {
                "(design document is still empty)".to_string()
            } else {
                crate::utils::truncate_str(trimmed, PLAN_PROSE_CAP).to_string()
            };
            format!(
                "📋 Editing design plan{}\n{ready}\n\n{prose}\n\nDocument: {}",
                if title.is_empty() {
                    String::new()
                } else {
                    format!(": {title}")
                },
                md.display()
            )
        }
        PlanModeState::Active => {
            let Some(plan) = plan_files::load_plan(session_id).await else {
                return "Plan JSON is unreadable.".to_string();
            };
            format_active_checklist(&plan)
        }
    }
}

/// The Active-checklist summary shared by `/show-plan` and the TUI plan
/// overlay: the seed-incomplete notice when tasks are empty, otherwise the
/// checklist with progress. Pure (takes the already-loaded plan), so the
/// sync render path can reuse it without touching the async store.
pub fn format_active_checklist(plan: &crate::tui::plan::PlanDocument) -> String {
    if plan.tasks.is_empty() {
        return format!(
            "📋 {} is Active but the checklist is still empty (seed did not \
             finish). Retry with /execute, or /discard.",
            plan.title
        );
    }
    let done = plan
        .tasks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                crate::tui::plan::TaskStatus::Completed | crate::tui::plan::TaskStatus::Skipped
            )
        })
        .count();
    let lines = plan
        .tasks
        .iter()
        .map(|t| format!("{} {}. {}", t.status.icon(), t.order, t.title))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "📋 {} (Active, {done}/{} done)\n{lines}",
        plan.title,
        plan.tasks.len()
    )
}

/// Whether the session is in the seed window: Active design plan whose
/// checklist has not successfully started (empty tasks, or tasks present
/// but none ever left Pending). Surfaces show Building checklist… chrome
/// while a seed turn is in flight in this window.
pub async fn in_seed_window(session_id: Uuid) -> bool {
    if plan_files::plan_mode_state(session_id).await != PlanModeState::Active {
        return false;
    }
    if !plan_files::plan_md_path(session_id).await.exists() {
        return false;
    }
    plan_files::load_plan(session_id).await.is_some_and(|p| {
        p.status == PlanStatus::Active
            && p.tasks
                .iter()
                .all(|t| matches!(t.status, crate::tui::plan::TaskStatus::Pending))
    })
}
