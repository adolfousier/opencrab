//! Persistent per-session plan card (#580): a single Telegram message that
//! shows the plan title + checklist and the Approve/Discard keyboard, edited in
//! place across the creation/execution/completion turns instead of re-rendered
//! inside each per-turn flow block. Tracked cross-turn on [`TelegramState`], so
//! there is exactly one card at a time rather than one checklist per turn.

use super::TelegramState;
use super::flow_chrome::{
    GoalSection, PlanKb, ProseSection, load_goal_section, load_plan_prose, load_plan_sections,
    prose_body_lines,
};
use super::handler::escape_html;
use super::send::message_in_thread;
use crate::brain::agent::AgentService;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{ParseMode, ThreadId};
use uuid::Uuid;

/// Telegram's per-message character limit. The card must fit in one message
/// because it carries the Approve/Discard keyboard.
const TG_MSG_LIMIT: usize = 4096;

/// Goal text budget (chars) on one card. The goal renders as a collapsed
/// expandable (ADR 0005 Decision 12), so the cap only trims the expanded
/// body, never the visible chrome.
const GOAL_TEXT_CAP: usize = 600;

/// Render the card body, or `None` when the session has no plan content (no
/// title and no checklist) — the caller removes the card in that case.
pub(crate) fn render_plan_card_html(
    title: Option<&str>,
    checklist: Option<&[String]>,
    prose: Option<&[ProseSection]>,
    goal: Option<&GoalSection>,
) -> Option<String> {
    let mut out = String::new();
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        out.push_str(&format!("📋 <b>{}</b>", escape_html(t)));
    }
    // Fold the design prose into the card (#621) using the same per-heading
    // expandable format as the flow chrome (ADR 0005 Decision 3): every
    // section is its own collapsed <blockquote expandable> with a bold
    // heading, so the card stays compact and each section expands on tap.
    // Locked order: title, prose expandables, checklist rows. No per-section
    // budget — each section renders fully (matching chrome_classic); the
    // 4096 hard-cap at the end drops whole sections from the tail if needed.
    if let Some(sections) = prose.filter(|s| !s.is_empty()) {
        for sec in sections {
            let full = prose_body_lines(&sec.body).join("\n");
            if !out.is_empty() {
                out.push('\n');
            }
            match &sec.heading {
                Some(h) => out.push_str(&format!(
                    "<blockquote expandable><b>{}</b>\n{full}</blockquote>",
                    escape_html(h),
                )),
                None => out.push_str(&full),
            }
        }
    }
    if let Some(rows) = checklist {
        for row in rows {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&escape_html(row));
        }
    }
    // Goal section last in the locked order (ADR 0005 Decision 3: title,
    // prose expandables, checklist rows, goal), rendered as its own collapsed
    // <blockquote expandable> with the Decision 10 prefix — the card is
    // always a settled render, so a completed goal shows ✅ and an active
    // one 🎯. A blank line separates it from prose/checklist above, exactly
    // like FlowSections::chrome_classic.
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
            let truncated: String = text.chars().take(GOAL_TEXT_CAP).collect();
            out.push_str(&format!(
                "<blockquote expandable>{} {}</blockquote>",
                g.prefix(true),
                escape_html(&truncated)
            ));
        }
    }
    // Hard 4096-char cap: if the card still exceeds Telegram's limit after
    // full rendering, drop whole sections from the end until it fits.  Split
    // by `<blockquote` boundaries so we never cut mid-tag.  The full prose
    // is always available via /show-plan.
    if out.chars().count() > TG_MSG_LIMIT {
        // Split into segments: each <blockquote expandable>...<blockquote expandable>...</blockquote>
        // block is one segment, plus the lines before/after blockquotes.
        let mut segments: Vec<String> = Vec::new();
        let mut current = String::new();
        for line in out.split('\n') {
            if line.starts_with("<blockquote expandable>") && !current.is_empty() {
                segments.push(current);
                current = String::new();
            }
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
            if line.ends_with("</blockquote>") {
                segments.push(current);
                current = String::new();
            }
        }
        if !current.is_empty() {
            segments.push(current);
        }
        // Drop segments from the end until the card fits
        while segments.len() > 1
            && segments.iter().map(|s| s.chars().count() + 1).sum::<usize>() > TG_MSG_LIMIT + 1
        {
            segments.pop();
        }
        out = segments.join("\n");
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(heading: &str, body: &str) -> ProseSection {
        ProseSection {
            heading: Some(heading.to_string()),
            body: body.to_string(),
        }
    }

    #[test]
    fn card_never_exceeds_4096_chars() {
        // Build a plan with massive prose that would definitely exceed 4096
        // if the hard cap didn't drop whole sections from the tail.
        let big_body = "x".repeat(10_000);
        let sections = vec![
            section("Section A", &big_body),
            section("Section B", &big_body),
            section("Section C", &big_body),
        ];
        let checklist: Vec<String> = (0..50).map(|i| format!("☐ Task {i}")).collect();
        let goal = GoalSection {
            text: "y".repeat(5000),
            completed: false,
        };
        let html = render_plan_card_html(
            Some("Big Plan Title"),
            Some(&checklist),
            Some(&sections),
            Some(&goal),
        )
        .unwrap();
        assert!(
            html.chars().count() <= TG_MSG_LIMIT,
            "card exceeded 4096 chars: {}",
            html.chars().count()
        );
    }

    #[test]
    fn prose_truncation_never_breaks_html_tags() {
        // Prose with HTML-producing content (code fences become <code> tags).
        // Each section renders fully — HTML is always well-formed.
        let body = "some text\n```\nlet x = 1;\nlet y = 2;\nlet z = x + y;\n```\nmore text";
        let sections = vec![section("Code Section", body)];
        let html =
            render_plan_card_html(Some("Title"), None, Some(&sections), None).unwrap();
        // Every <code> must have a matching </code>
        let opens = html.matches("<code>").count();
        let closes = html.matches("</code>").count();
        assert_eq!(opens, closes, "mismatched <code> tags: {html}");
        // Every <blockquote> must have a matching </blockquote>
        let bq_opens = html.matches("<blockquote").count();
        let bq_closes = html.matches("</blockquote>").count();
        assert_eq!(bq_opens, bq_closes, "mismatched <blockquote> tags: {html}");
    }

    #[test]
    fn small_prose_fully_preserved() {
        // Prose that fits within 4096 is NOT truncated at all — the card
        // shows full content matching chrome_classic behaviour.
        let body = "Hello world. This is a short prose section.";
        let sections = vec![section("Context", body)];
        let html =
            render_plan_card_html(Some("Title"), None, Some(&sections), None).unwrap();
        assert!(html.contains("Hello world"), "prose was lost: {html}");
        assert!(html.contains("<blockquote expandable>"), "no blockquote: {html}");
    }

    #[test]
    fn huge_sections_dropped_from_tail() {
        // When total exceeds 4096, whole <blockquote expandable> sections
        // are dropped from the end. The first section is kept.
        let body_a = "A content here.";
        let body_b = "x".repeat(5000); // forces over 4096
        let sections = vec![section("First", body_a), section("Second", &body_b)];
        let html =
            render_plan_card_html(Some("Title"), None, Some(&sections), None).unwrap();
        assert!(
            html.chars().count() <= TG_MSG_LIMIT,
            "card exceeded 4096 chars: {}",
            html.chars().count()
        );
        assert!(html.contains("First"), "first section was dropped");
        // Second section should be dropped to fit
        assert!(!html.contains("Second"), "second section should have been dropped");
    }

    #[test]
    fn goal_text_truncated_by_chars_not_bytes() {
        // Multibyte goal text should be truncated by character count.
        let goal_text = "🎯".repeat(1000); // 3000 bytes but 1000 chars
        let goal = GoalSection {
            text: goal_text,
            completed: false,
        };
        let html = render_plan_card_html(Some("Title"), None, None, Some(&goal)).unwrap();
        assert!(
            html.chars().count() <= TG_MSG_LIMIT,
            "card exceeded 4096 chars: {}",
            html.chars().count()
        );
        // Goal should be wrapped in blockquote
        assert!(html.contains("<blockquote expandable>"));
        assert!(html.contains("</blockquote>"));
    }

    #[test]
    fn empty_sections_produce_none() {
        assert!(render_plan_card_html(None, None, None, None).is_none());
    }

    #[test]
    fn title_only_card() {
        let html = render_plan_card_html(Some("My Plan"), None, None, None).unwrap();
        assert!(html.contains("📋 <b>My Plan</b>"));
    }

    #[test]
    fn checklist_rows_preserved() {
        let rows = vec![
            "☑ Done task".to_string(),
            "☐ Todo task".to_string(),
        ];
        let html = render_plan_card_html(None, Some(&rows), None, None).unwrap();
        assert!(html.contains("☑ Done task"));
        assert!(html.contains("☐ Todo task"));
    }

    #[test]
    fn prose_with_cyrillic_no_panic() {
        // Regression: byte-based truncation on multibyte Cyrillic panics.
        // Now renders fully (no pre-truncation), with the 4096 cap
        // dropping whole sections if needed.
        let body = "А".repeat(5000);
        let sections = vec![section("Кириллица", &body)];
        let html =
            render_plan_card_html(Some("План"), None, Some(&sections), None).unwrap();
        assert!(
            html.chars().count() <= TG_MSG_LIMIT,
            "card exceeded 4096 chars: {}",
            html.chars().count()
        );
    }

    #[test]
    fn cap_never_breaks_blockquote_tags() {
        // When the 4096 cap drops sections, the remaining HTML must still
        // have matched open/close tags.
        let sections = vec![
            section("A", &"a".repeat(2000)),
            section("B", &"b".repeat(2000)),
            section("C", &"c".repeat(2000)),
        ];
        let html =
            render_plan_card_html(Some("Title"), None, Some(&sections), None).unwrap();
        assert!(
            html.chars().count() <= TG_MSG_LIMIT,
            "card exceeded 4096: {}",
            html.chars().count()
        );
        let bq_opens = html.matches("<blockquote").count();
        let bq_closes = html.matches("</blockquote>").count();
        assert_eq!(bq_opens, bq_closes, "mismatched blockquote tags in capped card");
    }
}

/// Create or update the session's plan card to reflect the live plan state,
/// carrying `plan_kb`. Removes the card when the plan is gone.
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
    // Fold the design prose into the card (#621): the same per-heading
    // sections the flow message renders via chrome_classic (ADR 0005
    // Decision 3), in both Editing and Active states. The card is the
    // single surface carrying title + prose expandables + checklist +
    // goal + keyboard. The full .md stays accessible via /show-plan.
    let prose = load_plan_prose(session_id).await;
    // Goal chrome (ADR 0005 Decision 10) rides the card only once the plan
    // is Active — "never set while the plan is Editing". Covers goals from
    // /goal, goal_manage, and acceptance criteria auto-pushed on task start.
    let goal = if checklist.is_some() {
        load_goal_section(agent, session_id)
            .await
            .map(|(text, completed)| GoalSection { text, completed })
    } else {
        None
    };
    let Some(html) = render_plan_card_html(
        title.as_deref(),
        checklist.as_deref(),
        prose.as_deref(),
        goal.as_ref(),
    ) else {
        // Lock-free variant: this function already holds the per-session card
        // lock, and it is not reentrant. Calling the public remove_plan_card
        // here deadlocked the handler, and since this branch fires whenever a
        // session has no card content — i.e. most sessions — it blocked
        // delivery in every chat.
        remove_plan_card_locked(bot, chat, state, session_id).await;
        return;
    };
    let kb = plan_kb.keyboard();
    // Signature = body + keyboard state, so an unchanged card is skipped
    // entirely (no edit API call) — the per-tick refresh must not storm edits.
    let signature = format!("{html}\u{1}{plan_kb:?}");

    if let Some((mid, last_sig)) = state.plan_card(session_id).await {
        if last_sig == signature {
            return; // nothing changed; skip the edit
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
                let es = e.to_string();
                if es.contains("message is not modified") {
                    state
                        .set_plan_card(session_id, chat, thread_id, mid, signature)
                        .await;
                    return;
                }
                // Throttled, not broken. Keep the tracked id: dropping it would
                // force the next refresh to CREATE, which is the write most
                // likely to be rejected and the one that leaves duplicates
                // behind (#814).
                if let Some(wait) = super::rate_limit::parse_retry_after(&es) {
                    tracing::warn!(
                        "Telegram plan card edit throttled for session {session_id}: {es} — \
                         pausing card writes for {}s",
                        wait.as_secs()
                    );
                    state
                        .suppress_plan_card(session_id, wait + super::rate_limit::RETRY_MARGIN)
                        .await;
                    return;
                }
                // The tracked card is gone / unusable — drop it and recreate.
                tracing::debug!("Telegram plan card edit failed ({mid:?}): {es} — recreating");
                state.take_plan_card(session_id).await;
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
            let es = e.to_string();
            // Do not retry into the same window. Every rejected attempt kept
            // flood control alive, which is why the countdown ticked from 40s
            // to 3s without ever elapsing (#814).
            if let Some(wait) = super::rate_limit::parse_retry_after(&es) {
                tracing::warn!(
                    "Telegram plan card create throttled for session {session_id}: {es} — \
                     pausing card writes for {}s",
                    wait.as_secs()
                );
                state
                    .suppress_plan_card(session_id, wait + super::rate_limit::RETRY_MARGIN)
                    .await;
            } else {
                tracing::warn!("Telegram plan card create failed: {es}");
            }
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
