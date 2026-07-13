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
use super::markdown::format_inline;
use crate::brain::agent::AgentService;
use crate::brain::goal::GoalManager;
use crate::tui::plan::TaskStatus;
use std::sync::Arc;
use teloxide::prelude::*;
use uuid::Uuid;

/// Longest plan-title / goal text shown in flow chrome before truncation.
const SECTION_TEXT_CAP: usize = 60;

/// Which plan keyboard the latest flow message owns. Keyboards attach only
/// after `plan init` succeeds: Approve + Discard while the design plan is
/// Editing, Discard only while a checklist is Active, none otherwise.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanKb {
    #[default]
    None,
    /// Editing design plan: ✅ Approve + 🗑 Discard.
    ApproveDiscard,
    /// Active checklist: 🗑 Discard only.
    DiscardOnly,
}

impl PlanKb {
    /// Inline keyboard for this state, `None` when no buttons apply.
    /// Callback data uses the `plan:` prefix, deliberately distinct from
    /// tool-approval `approve:{id}` so the two can never collide.
    pub(crate) fn keyboard(self) -> Option<teloxide::types::InlineKeyboardMarkup> {
        use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
        match self {
            PlanKb::None => None,
            PlanKb::ApproveDiscard => Some(InlineKeyboardMarkup::new(vec![vec![
                InlineKeyboardButton::callback("✅ Approve plan", "plan:ok"),
                InlineKeyboardButton::callback("🗑 Discard", "plan:no"),
            ]])),
            PlanKb::DiscardOnly => Some(InlineKeyboardMarkup::new(vec![vec![
                InlineKeyboardButton::callback("🗑 Discard plan", "plan:no"),
            ]])),
        }
    }
}

/// One top-level section of the session plan `.md` prose (ADR 0005
/// Decision 12): `heading` is the `##` text (`None` for the orphan preamble
/// before the first top-level heading) and `body` is the raw markdown under
/// it, nested `###` headings, lists, and tables included.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProseSection {
    pub(crate) heading: Option<String>,
    pub(crate) body: String,
}

/// Split session-plan markdown into per-top-level-heading prose sections
/// (ADR 0005 Decision 12): strip the leading `# …` H1 (the plan title block
/// already carries it), then cut on `##` headings. Text before the first
/// `##` becomes the orphan preamble (`heading: None`). Fenced code lines are
/// never treated as headings. Sections whose body is empty are dropped — a
/// heading with nothing under it has nothing to disclose.
pub(crate) fn split_plan_prose(md: &str) -> Vec<ProseSection> {
    let mut raw: Vec<(Option<String>, Vec<&str>)> = vec![(None, Vec::new())];
    let mut in_fence = false;
    let mut seen_content = false;
    let mut h1_stripped = false;
    for line in md.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            seen_content = true;
            raw.last_mut().expect("raw starts non-empty").1.push(line);
            continue;
        }
        if !in_fence {
            if !h1_stripped && !seen_content && trimmed.starts_with("# ") {
                h1_stripped = true;
                seen_content = true;
                continue;
            }
            if let Some(h) = trimmed.strip_prefix("## ") {
                let h = h.trim();
                if !h.is_empty() {
                    raw.push((Some(h.to_string()), Vec::new()));
                    seen_content = true;
                    continue;
                }
            }
        }
        if !trimmed.is_empty() {
            seen_content = true;
        }
        raw.last_mut().expect("raw starts non-empty").1.push(line);
    }
    raw.into_iter()
        .filter_map(|(heading, lines)| {
            let body = lines.join("\n").trim().to_string();
            (!body.is_empty()).then_some(ProseSection { heading, body })
        })
        .collect()
}

/// Format the markdown body of one prose section into per-line Telegram
/// HTML: fenced code lines as `<code>`, `#` headings as bold, list items as
/// bullets, inline markdown elsewhere. Blank source lines come through as
/// empty strings so the classic join keeps paragraph breaks; the rich path
/// drops them (each line is already its own `<p>` block).
fn prose_body_lines(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            out.push(format!("<code>{}</code>", escape_html(line)));
            continue;
        }
        if trimmed.is_empty() {
            out.push(String::new());
            continue;
        }
        if trimmed.starts_with('#') {
            let content = trimmed.trim_start_matches('#').trim();
            out.push(format!("<b>{}</b>", format_inline(&escape_html(content))));
            continue;
        }
        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            out.push(format!("• {}", format_inline(&escape_html(item))));
            continue;
        }
        out.push(format_inline(&escape_html(line)));
    }
    out
}

/// Always-visible flow sections. Built from live data by [`refresh_sections`];
/// rendered by [`FlowSections::chrome_rich`] / [`FlowSections::chrome_classic`]
/// so the two flow surfaces can never drift on section formatting.
#[derive(Default, Clone, PartialEq)]
pub(crate) struct FlowSections {
    /// Plan-mode state line: Editing prose summary, Building checklist…,
    /// or seed-error chrome. Leads the chrome line when present.
    pub(crate) plan_state: Option<String>,
    /// Plan keyboard the flow message should carry (attached on every
    /// open/edit; Telegram clears reply_markup on edits that omit it).
    pub(crate) plan_kb: PlanKb,
    /// Plan title from the live session plan JSON, when set.
    pub(crate) plan_title: Option<String>,
    /// Per-top-level-heading prose sections from the session plan `.md`
    /// (ADR 0005 Decision 12), `None` when the session has no design prose.
    pub(crate) prose: Option<Vec<ProseSection>>,
    /// Full checklist rows from the plan JSON `tasks[]`, each pre-marked with
    /// the ballot glyph (`☑ done` / `☐ undone`), raw and unescaped. `None`
    /// until a checklist has tasks; the full list is kept even when every task
    /// is done, through the completing turn's settle (ADR 0005 Decision 9).
    pub(crate) checklist: Option<Vec<String>>,
    /// Active goal one-liner from `GoalManager`, when a session goal is set.
    pub(crate) goal: Option<String>,
    /// Ctx budget footer (display-only), set at final delivery.
    pub(crate) ctx: Option<String>,
}

impl FlowSections {
    fn has_prose(&self) -> bool {
        self.prose.as_ref().is_some_and(|p| !p.is_empty())
    }

    /// Rich-path plan chrome in reading order (ADR 0005 Decision 3): title,
    /// per-heading prose `<details>`, `<hr>`, checklist rows, `<hr>`, goal.
    /// The title sits flush against the prose (Decision 13); `<hr>` appears
    /// before the checklist only when prose precedes it, and before the goal
    /// when prose or checklist precedes it. Section headings are inline
    /// `<summary>` text only — nested blocks inside a summary render as a
    /// blank disclosure (Decision 12). Empty when no plan sections are
    /// present; `plan_state` and `ctx` live in the merged footer.
    pub(crate) fn chrome_rich(&self) -> String {
        let mut out = String::new();
        if let Some(ref t) = self.plan_title {
            out.push_str(&format!("<p>📋 <b>{}</b></p>", escape_html(t)));
        }
        if let Some(ref sections) = self.prose {
            for sec in sections {
                let body: String = prose_body_lines(&sec.body)
                    .into_iter()
                    .filter(|l| !l.is_empty())
                    .map(|l| format!("<p>{l}</p>"))
                    .collect();
                match &sec.heading {
                    Some(h) => out.push_str(&format!(
                        "<details><summary>{}</summary>{body}</details>",
                        escape_html(h)
                    )),
                    // Orphan preamble: plain always-visible blocks.
                    None => out.push_str(&body),
                }
            }
        }
        if let Some(ref rows) = self.checklist {
            if self.has_prose() {
                out.push_str("<hr>");
            }
            for row in rows {
                out.push_str(&format!("<p>{}</p>", escape_html(row)));
            }
        }
        if let Some(ref g) = self.goal {
            if self.checklist.is_some() || self.has_prose() {
                out.push_str("<hr>");
            }
            out.push_str(&format!("<p>🎯 <i>{}</i></p>", escape_html(g)));
        }
        out
    }

    /// Classic-path plan chrome: the same locked vertical order with blank
    /// lines standing in for the rich `<hr>` (Decision 13 — classic HTML has
    /// no divider primitive). Prose sections are `<blockquote expandable>`
    /// blocks whose first line is the bold heading, so Telegram's collapsed
    /// peek shows it (Decision 12); the orphan preamble stays plain.
    pub(crate) fn chrome_classic(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(ref t) = self.plan_title {
            parts.push(format!("📋 <b>{}</b>", escape_html(t)));
        }
        if let Some(ref sections) = self.prose {
            for sec in sections {
                let body = prose_body_lines(&sec.body).join("\n");
                match &sec.heading {
                    Some(h) => parts.push(format!(
                        "<blockquote expandable><b>{}</b>\n{body}</blockquote>",
                        escape_html(h)
                    )),
                    None => parts.push(body),
                }
            }
        }
        if let Some(ref rows) = self.checklist {
            if self.has_prose() {
                parts.push(String::new());
            }
            for row in rows {
                parts.push(escape_html(row));
            }
        }
        if let Some(ref g) = self.goal {
            if self.checklist.is_some() || self.has_prose() {
                parts.push(String::new());
            }
            parts.push(format!("🎯 <i>{}</i>", escape_html(g)));
        }
        parts.join("\n")
    }
}

/// Format an elapsed duration as the locked flow clock glyph `⏱ M:SS`
/// (`⏱ H:MM:SS` past an hour) — ADR 0005 Decision 13. This is the last
/// segment of every merged footer; never render a bare `M:SS` without the
/// glyph.
pub(crate) fn clock_glyph(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("⏱ {h}:{m:02}:{s:02}")
    } else {
        format!("⏱ {m}:{s:02}")
    }
}

/// Inputs to the merged flow footer (ADR 0005 Decision 12). The renderer
/// decomposes its `FlowHeader` / lines / sections into these primitives so the
/// footer join lives in one place and both the classic and rich paths agree.
pub(crate) struct FooterParts<'a> {
    /// Settled outcome `(icon, verb)` (e.g. `("✅", "Finished")`) once the turn
    /// ends; `None` while live. Drives segment 1 and drops the in-flight cog.
    pub(crate) outcome: Option<(&'a str, &'a str)>,
    /// Plan-mode status line (Decision 7) when in Plan mode; leads segment 1
    /// on a live turn.
    pub(crate) plan_state: Option<&'a str>,
    /// Non-plan "Working on …" / thinking preview; segment 1 fallback on a live
    /// turn with no plan status.
    pub(crate) working_on: Option<&'a str>,
    /// Latest-activity preview for the in-flight progress-log summary
    /// (segment 2); shown only while live and only when a log exists.
    pub(crate) activity: Option<&'a str>,
    /// Count of tool entries in the log (`N tool calls` when `>= 1`).
    pub(crate) tool_count: usize,
    /// Whether a processing log exists at all (drives segment 2 presence).
    pub(crate) has_log: bool,
    /// Ctx budget string (segment 3), display-only, before the clock.
    pub(crate) ctx: Option<&'a str>,
    /// Elapsed wall-clock seconds for the segment-4 clock glyph.
    pub(crate) elapsed_secs: u64,
}

/// Build the merged flow footer: one ` • `-joined string in the locked order
/// status → progress-log summary → ctx → clock (ADR 0005 Decision 12). The
/// renderer wraps it: rich as `<sub>` (plain footer line, or the processing-log
/// `<summary>`); classic as a plain final line. In-flight the log summary
/// carries the `⚙️` cog; a settled footer never does (segment 1 carries the
/// `✅`/`❌` outcome instead).
pub(crate) fn merged_footer(parts: &FooterParts, markup: HeaderMarkup) -> String {
    let esc = |s: &str| match markup {
        HeaderMarkup::Html => escape_html(s),
        HeaderMarkup::Markdown => s.to_string(),
    };
    let settled = parts.outcome.is_some();
    let mut segs: Vec<String> = Vec::new();

    // Segment 1 — status: settled outcome, else plan state, else Working-on.
    if let Some((icon, verb)) = parts.outcome {
        segs.push(format!("{icon} {}", esc(verb)));
    } else if let Some(ps) = parts.plan_state {
        segs.push(esc(ps));
    } else if let Some(w) = parts.working_on {
        segs.push(esc(w));
    }

    // Segment 2 — progress-log summary, only when a log exists. Live turns lead
    // with the cog + activity; settled turns show a bare tool-call count with
    // no cog (the stale narration is dropped, #498). Strip a leading cog from
    // the activity so the prefix is never doubled (#509 follow-up).
    if parts.has_log {
        let mut seg2 = String::new();
        if !settled && let Some(act) = parts.activity {
            let act = act.trim_start_matches(['⚙', '\u{fe0f}']).trim_start();
            if !act.is_empty() {
                seg2 = format!("⚙️ {}", esc(act));
            }
        }
        if parts.tool_count >= 1 {
            let count = format!("{} tool calls", parts.tool_count);
            if seg2.is_empty() {
                seg2 = if settled {
                    count
                } else {
                    format!("⚙️ {count}")
                };
            } else {
                seg2 = format!("{seg2} • {count}");
            }
        } else if !settled && seg2.is_empty() {
            // In-flight log with no tools and no activity preview yet: a bare
            // cog beats an empty segment so the footer still reads as active.
            seg2 = "⚙️".to_string();
        }
        if !seg2.is_empty() {
            segs.push(seg2);
        }
    }

    // Segment 3 — ctx, before the clock.
    if let Some(c) = parts.ctx {
        segs.push(esc(c));
    }

    // Segment 4 — clock, always last.
    segs.push(clock_glyph(parts.elapsed_secs));

    segs.join(" • ")
}

/// Read the plan title + full `☐`/`☑` checklist rows from the live session
/// plan JSON through the shared plan store, which maps legacy statuses onto
/// Editing/Active and resolves terminal ones (Completed archives, Cancelled
/// deletes) — so stale chrome never outlives the plan.
pub(crate) async fn load_plan_sections(session_id: Uuid) -> (Option<String>, Option<Vec<String>>) {
    let Some(plan) = crate::utils::plan_files::load_plan(session_id).await else {
        return (None, None);
    };
    let title = {
        let t = plan.title.trim();
        (!t.is_empty()).then(|| crate::utils::truncate_str(t, SECTION_TEXT_CAP).to_string())
    };
    // Full ballot checklist (ADR 0005 Decision 3): one row per task, `☑` for a
    // completed task and `☐` otherwise, kept complete even when every task is
    // done until the completing turn settles (Decision 9). Empty `tasks`
    // (Editing before the seed) yield no checklist.
    let checklist = (!plan.tasks.is_empty()).then(|| {
        plan.tasks
            .iter()
            .map(|t| {
                let mark = if matches!(t.status, TaskStatus::Completed) {
                    '☑'
                } else {
                    '☐'
                };
                let title = crate::utils::truncate_str(t.title.trim(), SECTION_TEXT_CAP);
                format!("{mark} {title}")
            })
            .collect()
    });
    (title, checklist)
}

/// Per-heading prose sections from the session plan `.md`, when it exists
/// (Editing, or Active where the approved design stays frozen on disk —
/// discard and archive both delete the file, so stale prose never outlives
/// the plan). `None` when the file is absent or yields no sections.
pub(crate) async fn load_plan_prose(session_id: Uuid) -> Option<Vec<ProseSection>> {
    let path = crate::utils::plan_files::plan_md_path(session_id).await;
    let body = match tokio::fs::read_to_string(&path).await {
        Ok(body) => body,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::debug!(
                    "Telegram flow chrome: plan prose read failed for {}: {e}",
                    path.display()
                );
            }
            return None;
        }
    };
    let sections = split_plan_prose(&body);
    (!sections.is_empty()).then_some(sections)
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

/// Plan-mode state line + keyboard ownership for the flow message
/// (Building checklist… machine, locked): while Active with a seed turn
/// in flight and empty tasks show Building checklist…; when the seed
/// ended without tasks show the error chrome and the retry hint; while
/// Editing show the prose summary with the approve hint. Keyboards
/// attach only after `init` succeeds (pre-init has none).
pub(crate) async fn load_plan_state_section(
    session_id: Uuid,
    turn_active: bool,
) -> (Option<String>, PlanKb) {
    use crate::utils::plan_files::{PlanModeState, plan_mode_state};
    match plan_mode_state(session_id).await {
        PlanModeState::NoPlan => (None, PlanKb::None),
        PlanModeState::PreInitEditing => (Some("📝 Plan mode: drafting".to_string()), PlanKb::None),
        PlanModeState::PostInitEditing => (
            Some("✍️ Editing plan • view: /show-plan • approve: /execute".to_string()),
            PlanKb::ApproveDiscard,
        ),
        PlanModeState::Active => {
            if crate::utils::plan_mode::in_seed_window(session_id).await {
                if turn_active {
                    (
                        Some("⏳ Building checklist…".to_string()),
                        PlanKb::DiscardOnly,
                    )
                } else {
                    (
                        Some("⚠️ Checklist seed incomplete • retry: /execute".to_string()),
                        PlanKb::DiscardOnly,
                    )
                }
            } else {
                (None, PlanKb::DiscardOnly)
            }
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
    let prose = load_plan_prose(session_id).await;
    let goal = load_goal_section(agent, session_id).await;
    // Plan-state derivation reads plan files; keep that IO outside the
    // streaming lock (short double-lock beats file reads under the mutex).
    let turn_active = {
        let s = streaming.lock().unwrap_or_else(|e| e.into_inner());
        s.flow_outcome.is_none()
    };
    let (plan_state, plan_kb) = load_plan_state_section(session_id, turn_active).await;
    let mut s = streaming.lock().unwrap_or_else(|e| e.into_inner());
    let next = FlowSections {
        plan_state,
        plan_kb,
        plan_title,
        prose,
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
