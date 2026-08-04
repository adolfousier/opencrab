//! Persistent per-session plan card (#580): a single Telegram message that
//! shows the plan title + checklist and the Approve/Discard keyboard, edited in
//! place across the creation/execution/completion turns instead of re-rendered
//! inside each per-turn flow block. Tracked cross-turn on [`TelegramState`], so
//! there is exactly one card at a time rather than one checklist per turn.

use super::TelegramState;
use super::flow_chrome::{
    GoalSection, PlanKb, ProseSection, load_goal_section, load_plan_prose, load_plan_sections,
};
use super::handler::escape_html;
use super::send::message_in_thread;
use crate::brain::agent::AgentService;
use crate::config::Config;
use crate::utils::truncate_chars;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{MessageId, ParseMode, ThreadId};
use uuid::Uuid;

/// Total character budget for prose bodies on the classic card. The card
/// carries the title, checklist rows, and keyboard inside Telegram's 4096-char
/// message cap; sections past the budget are dropped (full prose via
/// /show-plan). The rich path (`sendRichMessage`, 32K chars) needs no budget.
const CARD_PROSE_BUDGET: usize = 2400;

/// Goal text budget (chars) on the classic card. The goal renders as a
/// collapsed expandable (ADR 0005 Decision 12), so the cap only trims the
/// expanded body, never the visible chrome.
const GOAL_TEXT_CAP: usize = 600;

/// Collapsible wrapper style for prose sections and goals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CollapsibleStyle {
    /// Classic `sendMessage` (4096 chars): `<blockquote expandable>`,
    /// prose truncated to `CARD_PROSE_BUDGET`.
    BlockquoteExpandable,
    /// Rich `sendRichMessage` (32K chars): `<details><summary>`, no truncation.
    DetailsSummary,
}

/// Unified plan card renderer. The two surfaces (classic sendMessage and rich
/// sendRichMessage) share title, checklist, and goal logic; only the
/// collapsible tag pair and truncation budget differ.
///
/// Returns `None` when the session has no plan content (no title and no
/// checklist) — the caller removes the card in that case.
fn render_plan_card(
    style: CollapsibleStyle,
    title: Option<&str>,
    checklist: Option<&[String]>,
    prose: Option<&[ProseSection]>,
    goal: Option<&GoalSection>,
) -> Option<String> {
    let mut out = String::new();

    // Title: identical for both styles.
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        out.push_str(&format!("📋 <b>{}</b>", escape_html(t)));
    }

    // Prose: style-dependent collapsible tags + truncation budget.
    // Locked order: title, prose expandables, checklist rows, goal.
    if let Some(sections) = prose.filter(|s| !s.is_empty()) {
        let mut budget: Option<usize> = match style {
            CollapsibleStyle::BlockquoteExpandable => Some(CARD_PROSE_BUDGET),
            CollapsibleStyle::DetailsSummary => None,
        };
        for sec in sections {
            if budget == Some(0) {
                break;
            }
            // Truncate raw text BEFORE HTML conversion so the collapsible
            // tags are always well-formed (truncating rendered HTML can
            // cut mid-tag, causing Telegram to strip rich formatting).
            let (body, chars_used) = match budget {
                Some(remaining) => {
                    let truncated = truncate_chars(&sec.body, remaining);
                    (truncated, truncated.chars().count())
                }
                None => (sec.body.as_str(), 0),
            };
            budget = budget.map(|b| b.saturating_sub(chars_used));
            if !out.is_empty() {
                out.push('\n');
            }
            match (&sec.heading, style) {
                (Some(h), CollapsibleStyle::BlockquoteExpandable) => {
                    let html = super::rich::markdown_to_html(body);
                    out.push_str(&format!(
                        "<blockquote expandable><b>{}</b>\n{html}</blockquote>",
                        escape_html(h),
                    ))
                }
                (Some(h), CollapsibleStyle::DetailsSummary) => {
                    let html = super::rich::markdown_to_html_p(body);
                    out.push_str(&format!(
                        "<details><summary><b>{}</b></summary>{html}</details>",
                        escape_html(h),
                    ))
                }
                (None, CollapsibleStyle::DetailsSummary) => {
                    let html = super::rich::markdown_to_html_p(body);
                    out.push_str(&html)
                }
                (None, _) => {
                    let html = super::rich::markdown_to_html(body);
                    out.push_str(&html)
                }
            }
        }
    }

    // Checklist: identical for both styles.
    if let Some(rows) = checklist {
        for row in rows {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&escape_html(row));
        }
    }

    // Goal: style-dependent wrapper + truncation on classic only.
    if let Some(g) = goal {
        let text = g.text.trim();
        if !text.is_empty() {
            let has_prose = prose.is_some_and(|p| !p.is_empty());
            if !out.is_empty() {
                out.push('\n');
                if checklist.is_some() || has_prose {
                    out.push('\n');
                }
            }
            let goal_html = match style {
                CollapsibleStyle::BlockquoteExpandable => {
                    let capped = escape_html(truncate_chars(text, GOAL_TEXT_CAP));
                    format!(
                        "<blockquote expandable>{} {capped}</blockquote>",
                        g.prefix(true)
                    )
                }
                CollapsibleStyle::DetailsSummary => format!(
                    "<details><summary>{} goal</summary>\n{}</details>",
                    g.prefix(true),
                    escape_html(text)
                ),
            };
            out.push_str(&goal_html);
        }
    }

    (!out.is_empty()).then_some(out)
}

/// Classic sendMessage card: `<blockquote expandable>` collapsibles, 4096-char
/// budget with per-section prose truncation.
pub(crate) fn render_plan_card_html(
    title: Option<&str>,
    checklist: Option<&[String]>,
    prose: Option<&[ProseSection]>,
    goal: Option<&GoalSection>,
) -> Option<String> {
    render_plan_card(
        CollapsibleStyle::BlockquoteExpandable,
        title,
        checklist,
        prose,
        goal,
    )
}

/// Rich `sendRichMessage` card: `<details><summary>` collapsibles, 32K-char
/// limit, no truncation — prose renders in full.
pub(crate) fn render_plan_card_rich_html(
    title: Option<&str>,
    checklist: Option<&[String]>,
    prose: Option<&[ProseSection]>,
    goal: Option<&GoalSection>,
) -> Option<String> {
    render_plan_card(
        CollapsibleStyle::DetailsSummary,
        title,
        checklist,
        prose,
        goal,
    )
}

/// Result of a plan card edit attempt.
enum EditOutcome {
    /// Card saved successfully (or content unchanged).
    Saved,
    /// Rate-limited: card writes suppressed for a duration.
    Suppressed,
    /// Card gone/unusable: caller should try creating fresh.
    Gone,
}

/// Classify a plan card edit failure and take the appropriate state action.
/// Handles "message is not modified" (silent success) and rate-limiting
/// (suppress future writes). Returns `Gone` when the card needs recreating.
async fn handle_edit_failure(
    error: &str,
    state: &TelegramState,
    session_id: Uuid,
    chat: ChatId,
    thread_id: Option<ThreadId>,
    signature: &str,
    mid: MessageId,
) -> EditOutcome {
    if error.contains("message is not modified") {
        state
            .set_plan_card(session_id, chat, thread_id, mid, signature.to_string())
            .await;
        return EditOutcome::Saved;
    }
    if let Some(wait) = super::rate_limit::parse_retry_after(error) {
        tracing::warn!(
            "Telegram plan card edit throttled for session {session_id}: {error} — \
             pausing card writes for {}s",
            wait.as_secs()
        );
        state
            .suppress_plan_card(session_id, wait + super::rate_limit::RETRY_MARGIN)
            .await;
        return EditOutcome::Suppressed;
    }
    tracing::debug!("Telegram plan card edit failed ({mid:?}): {error} — recreating");
    state.take_plan_card(session_id).await;
    EditOutcome::Gone
}

/// Classify a plan card create failure. Suppresses future writes on rate-limit,
/// warns on other errors.
async fn handle_create_failure(error: &str, state: &TelegramState, session_id: Uuid) {
    if let Some(wait) = super::rate_limit::parse_retry_after(error) {
        tracing::warn!(
            "Telegram plan card create throttled for session {session_id}: {error} — \
             pausing card writes for {}s",
            wait.as_secs()
        );
        state
            .suppress_plan_card(session_id, wait + super::rate_limit::RETRY_MARGIN)
            .await;
    } else {
        tracing::warn!("Telegram plan card create failed: {error}");
    }
}

/// Create or update the session's plan card to reflect the live plan state,
/// carrying `plan_kb`. Removes the card when the plan is gone.
///
/// When `rich_messages` is enabled, the card is sent via `sendRichMessage`
/// (32K char limit, native `<details><summary>` collapsibles). On any rich
/// API failure, falls back to the classic HTML `sendMessage` path (4096 chars,
/// `<blockquote expandable>`).
pub(crate) async fn refresh_plan_card(
    bot: &Bot,
    chat: ChatId,
    thread_id: Option<ThreadId>,
    state: &Arc<TelegramState>,
    agent: &AgentService,
    session_id: Uuid,
    plan_kb: PlanKb,
) {
    // Telegram asked us to wait. The card is chrome, so skipping an update
    // beats renewing the flood-control window on every refresh (#814).
    // Checked BEFORE taking the lock, so a throttled session releases waiters
    // immediately instead of queueing them behind a write that will not happen.
    if state.plan_card_suppressed(session_id).await {
        return;
    }

    // Serialise everything below (#822). The sequence is check-whether-a-card-
    // is-tracked, decide edit-or-post, record the id — and with nothing held
    // across it two concurrent refreshes both saw no card, both posted, and the
    // second id overwrote the first. The loser was left visible in the chat but
    // untracked, so it could never be edited or deleted again.
    //
    // Held across the API calls, not just the map reads: releasing before the
    // create is exactly what leaves the window open.
    let card_lock = state.plan_card_lock(session_id).await;
    let _guard = card_lock.lock().await;
    let (title, checklist) = load_plan_sections(session_id).await;
    let prose = load_plan_prose(session_id).await;
    let goal = if checklist.is_some() {
        load_goal_section(agent, session_id)
            .await
            .map(|(text, completed)| GoalSection { text, completed })
    } else {
        None
    };
    let use_rich = Config::current().channels.telegram.rich_messages;

    // Try rich path first when enabled: sendRichMessage (32K, native
    // <details><summary> collapsibles) with reply_markup for the keyboard.
    if use_rich
        && let Some(rich_html) = render_plan_card_rich_html(
            title.as_deref(),
            checklist.as_deref(),
            prose.as_deref(),
            goal.as_ref(),
        )
    {
        let kb_val = plan_kb
            .keyboard()
            .and_then(|m| serde_json::to_value(m).ok());
        let rich_sig = format!("rich:{rich_html}\u{1}{plan_kb:?}");
        if let Some((mid, last_sig)) = state.plan_card(session_id).await {
            if last_sig == rich_sig {
                return;
            }
            match super::rich::api::edit_rich_html(
                bot.token(),
                chat.0,
                mid.0,
                &rich_html,
                kb_val.as_ref(),
            )
            .await
            {
                Ok(()) => {
                    state
                        .set_plan_card(session_id, chat, thread_id, mid, rich_sig)
                        .await;
                    return;
                }
                Err(e) => {
                    let outcome = handle_edit_failure(
                        &e.to_string(),
                        state,
                        session_id,
                        chat,
                        thread_id,
                        &rich_sig,
                        mid,
                    )
                    .await;
                    match outcome {
                        EditOutcome::Saved | EditOutcome::Suppressed => return,
                        EditOutcome::Gone => { /* fall through to create */ }
                    }
                }
            }
        }
        // No live card or edit failed: create fresh via rich API.
        match super::rich::api::send_rich_html_id(
            bot.token(),
            chat.0,
            thread_id,
            &rich_html,
            kb_val.as_ref(),
        )
        .await
        {
            Ok(mid) => {
                state
                    .set_plan_card(session_id, chat, thread_id, MessageId(mid), rich_sig)
                    .await;
                return;
            }
            Err(e) => {
                tracing::warn!("Rich plan card create failed: {e} — falling back to HTML");
            }
        }
    }

    // Classic HTML path (sendMessage, 4096 chars, <blockquote expandable>).
    let Some(html) = render_plan_card_html(
        title.as_deref(),
        checklist.as_deref(),
        prose.as_deref(),
        goal.as_ref(),
    ) else {
        remove_plan_card_locked(bot, chat, state, session_id).await;
        return;
    };
    let kb = plan_kb.keyboard();
    let signature = format!("{html}\u{1}{plan_kb:?}");

    if let Some((mid, last_sig)) = state.plan_card(session_id).await {
        if last_sig == signature {
            return;
        }
        let mut req = bot
            .edit_message_text(chat, mid, html.clone())
            .parse_mode(ParseMode::Html);
        if let Some(ref k) = kb {
            req = req.reply_markup(k.clone());
        }
        match req.await {
            Ok(_) => {
                state
                    .set_plan_card(session_id, chat, thread_id, mid, signature)
                    .await;
                return;
            }
            Err(e) => {
                let outcome = handle_edit_failure(
                    &e.to_string(),
                    state,
                    session_id,
                    chat,
                    thread_id,
                    &signature,
                    mid,
                )
                .await;
                match outcome {
                    EditOutcome::Saved | EditOutcome::Suppressed => return,
                    EditOutcome::Gone => { /* fall through to create */ }
                }
            }
        }
    }

    // No live card (or it was unusable): post a fresh one at the bottom.
    let mut req = message_in_thread(bot, chat, thread_id, html).parse_mode(ParseMode::Html);
    if let Some(ref k) = kb {
        req = req.reply_markup(k.clone());
    }
    match req.await {
        Ok(m) => {
            state
                .set_plan_card(session_id, chat, thread_id, m.id, signature)
                .await
        }
        Err(e) => {
            handle_create_failure(&e.to_string(), state, session_id).await;
        }
    }
}

/// Delete the session's plan card and stop tracking it. Used both as terminal
/// removal (discard / plan gone) and — followed by a later [`refresh_plan_card`]
/// — as a re-stick so the next card posts fresh at the bottom of the
/// conversation, keeping exactly one card visible as it follows the turns down.
pub(crate) async fn remove_plan_card(
    bot: &Bot,
    chat: ChatId,
    state: &Arc<TelegramState>,
    session_id: Uuid,
) {
    // Same lock as refresh (#822). Removal clears tracking, so a refresh
    // interleaving here is guaranteed to see no card and post one, which is
    // the widest form of the race.
    //
    // Callers that ALREADY hold the lock must use remove_plan_card_locked
    // instead: the lock is not reentrant, so re-acquiring it deadlocks.
    let card_lock = state.plan_card_lock(session_id).await;
    let _guard = card_lock.lock().await;
    remove_plan_card_locked(bot, chat, state, session_id).await;
}

/// Removal body, for callers already holding the per-session card lock.
///
/// Split out because `refresh_plan_card` takes the lock and then needs to
/// remove on its no-content path. Calling the lock-taking version there
/// deadlocked the Telegram handler outright.
async fn remove_plan_card_locked(
    bot: &Bot,
    chat: ChatId,
    state: &Arc<TelegramState>,
    session_id: Uuid,
) {
    if let Some(mid) = state.take_plan_card(session_id).await
        && let Err(e) = bot.delete_message(chat, mid).await
    {
        tracing::debug!("Telegram plan card delete failed ({mid:?}): {e}");
    }
}
