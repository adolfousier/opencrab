//! Flow-message chrome: the always-visible sections (plan title, checklist
//! progress, active goal, ctx footer) rendered onto the per-turn flow
//! message, plus the shared live-header tick used by the main handler and
//! the crash-recovery resume loop.
//!
//! Telegram turn chrome is the per-turn flow message (`open_group_msg_id`),
//! not a Bot API pin and not a separate pre-block status bubble. Sections
//! read live data as-is: the session plan JSON for title and checklist, and
//! `GoalManager` for the active goal one-liner. Empty sections are omitted.

use super::flow::{HeaderMarkup, StreamingState, humanize_duration, open_flow, refresh_flow};
use super::handler::escape_html;
use crate::brain::agent::AgentService;
use crate::brain::goal::GoalManager;
use crate::tui::plan::{PlanDocument, PlanStatus, TaskStatus};
use std::sync::Arc;
use teloxide::prelude::*;
use uuid::Uuid;

/// Longest plan-title / goal text shown in flow chrome before truncation.
const SECTION_TEXT_CAP: usize = 60;

/// Always-visible flow sections. Built from live data by [`refresh_sections`];
/// rendered into one compact `•`-separated chrome line so the three flow
/// renderers can never drift on section formatting.
#[derive(Default, Clone, PartialEq)]
pub(crate) struct FlowSections {
    /// Plan title from the live session plan JSON, when set.
    pub(crate) plan_title: Option<String>,
    /// Checklist progress from the plan JSON `tasks[]`, e.g. `2/7 tasks`.
    pub(crate) checklist: Option<String>,
    /// Active goal one-liner from `GoalManager`, when a session goal is set.
    pub(crate) goal: Option<String>,
    /// Ctx budget footer (display-only), set at final delivery.
    pub(crate) ctx: Option<String>,
}

impl FlowSections {
    /// One compact chrome line (`📋 title • 2/7 tasks • 🎯 goal • ctx …`),
    /// styled and escaped for the renderer's dialect. `None` when every
    /// section is empty, so callers omit the line entirely.
    pub(crate) fn chrome_line(&self, markup: HeaderMarkup) -> Option<String> {
        let esc = |s: &str| match markup {
            HeaderMarkup::Html => escape_html(s),
            HeaderMarkup::Markdown => s.to_string(),
        };
        let mut segs: Vec<String> = Vec::new();
        if let Some(ref t) = self.plan_title {
            segs.push(format!("📋 {}", markup.bold(&esc(t))));
        }
        if let Some(ref c) = self.checklist {
            segs.push(markup.italic(&esc(c)));
        }
        if let Some(ref g) = self.goal {
            segs.push(format!("🎯 {}", markup.italic(&esc(g))));
        }
        if let Some(ref x) = self.ctx {
            segs.push(markup.italic(&esc(x)));
        }
        (!segs.is_empty()).then(|| segs.join(" • "))
    }
}

/// Read plan title + checklist progress from the live session plan JSON
/// (`.opencrabs_plan_{session}.json`), as-is: no status-enum migration, and
/// legacy shapes deserialize through the same serde defaults the plan tool
/// uses. Terminal plans (completed / cancelled / rejected) yield nothing so
/// stale chrome never outlives the plan.
pub(crate) async fn load_plan_sections(session_id: Uuid) -> (Option<String>, Option<String>) {
    let path = crate::config::opencrabs_home()
        .join("agents")
        .join("session")
        .join(format!(".opencrabs_plan_{session_id}.json"));
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(_) => return (None, None), // no plan file for this session
    };
    let plan: PlanDocument = match serde_json::from_str(&content) {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!("Telegram flow chrome: unreadable plan JSON at {path:?}: {e}");
            return (None, None);
        }
    };
    if matches!(
        plan.status,
        PlanStatus::Completed | PlanStatus::Cancelled | PlanStatus::Rejected
    ) {
        return (None, None);
    }
    let title = {
        let t = plan.title.trim();
        (!t.is_empty()).then(|| crate::utils::truncate_str(t, SECTION_TEXT_CAP).to_string())
    };
    let total = plan.tasks.len();
    let done = plan
        .tasks
        .iter()
        .filter(|t| matches!(t.status, TaskStatus::Completed))
        .count();
    // Progress only while something is still open; a fully ticked checklist
    // adds nothing the settled header does not already say.
    let checklist = (total > 0 && done < total).then(|| format!("{done}/{total} tasks"));
    (title, checklist)
}

/// Active goal one-liner from `GoalManager`, when the session has an
/// active goal.
pub(crate) async fn load_goal_section(agent: &AgentService, session_id: Uuid) -> Option<String> {
    let mgr = GoalManager::new(agent.context().clone());
    match mgr.get_goal(session_id).await {
        Ok(Some(goal)) if goal.state == "active" => {
            Some(crate::utils::truncate_str(goal.goal_text.trim(), SECTION_TEXT_CAP).to_string())
        }
        Ok(_) => None,
        Err(e) => {
            tracing::debug!("Telegram flow chrome: goal lookup failed: {e}");
            None
        }
    }
}

/// Reload the plan/goal sections from live data and store them on the
/// streaming state. Returns true when they changed (the flow needs a
/// re-render). The ctx section is owned by final delivery and preserved.
pub(crate) async fn refresh_sections(
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
    agent: &AgentService,
    session_id: Uuid,
) -> bool {
    let (plan_title, checklist) = load_plan_sections(session_id).await;
    let goal = load_goal_section(agent, session_id).await;
    let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
    let next = FlowSections {
        plan_title,
        checklist,
        goal,
        ctx: s.sections.ctx.clone(),
    };
    if s.sections == next {
        false
    } else {
        s.sections = next;
        true
    }
}

/// One live-header tick, shared by the main handler and resume edit loops so
/// they cannot drift: while the flow is open, roll the duration, the
/// thinking / Working-on preview, and the plan/goal sections, refreshing the
/// message when anything changed; while no flow is open and the turn is
/// still working, open the flow header-only on this first activity tick
/// (the pre-block status bubble is gone; the flow header owns early-turn
/// status).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn tick_flow_header(
    bot: &Bot,
    chat: ChatId,
    thread_id: Option<teloxide::types::ThreadId>,
    streaming: &Arc<std::sync::Mutex<StreamingState>>,
    agent: &AgentService,
    session_id: Uuid,
    show_status: bool,
    turn_done: bool,
    preview: Option<String>,
    mut needs_refresh: bool,
) {
    let open_block = {
        let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        s.open_group_msg_id
    };
    if open_block.is_some() {
        if show_status {
            let changed = {
                let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
                let elapsed = s.turn_started_at.elapsed().as_secs();
                let mut changed = false;
                let duration = (elapsed > 0).then(|| humanize_duration(elapsed));
                if duration.is_some() && s.flow_status != duration {
                    s.flow_status = duration;
                    changed = true;
                }
                if s.header_preview != preview {
                    s.header_preview = preview;
                    changed = true;
                }
                changed
            };
            needs_refresh |= changed;
            needs_refresh |= refresh_sections(streaming, agent, session_id).await;
        }
        if needs_refresh {
            refresh_flow(bot, chat, streaming).await;
        }
    } else {
        if needs_refresh {
            refresh_flow(bot, chat, streaming).await;
        }
        if show_status && !turn_done {
            // Merge pre-flow into the flow message: first activity tick opens
            // the flow header-only, thinking / Working-on riding the header.
            {
                let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
                s.header_preview = preview;
                let elapsed = s.turn_started_at.elapsed().as_secs();
                if elapsed > 0 {
                    s.flow_status = Some(humanize_duration(elapsed));
                }
            }
            refresh_sections(streaming, agent, session_id).await;
            open_flow(bot, chat, thread_id, streaming).await;
        }
    }
}
