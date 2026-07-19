//! Persistent per-session plan card (#580): a single Telegram message that
//! shows the plan title + checklist and the Approve/Discard keyboard, edited in
//! place across the creation/execution/completion turns instead of re-rendered
//! inside each per-turn flow block. Tracked cross-turn on [`TelegramState`], so
//! there is exactly one card at a time rather than one checklist per turn.

use super::TelegramState;
use super::flow_chrome::{PlanKb, load_plan_prose, load_plan_sections};
use super::handler::escape_html;
use super::send::message_in_thread;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{ParseMode, ThreadId};
use uuid::Uuid;

/// Render the card body, or `None` when the session has no plan content (no
/// title and no checklist) — the caller removes the card in that case.
pub(crate) fn render_plan_card_html(
    title: Option<&str>,
    checklist: Option<&[String]>,
    prose: Option<&str>,
) -> Option<String> {
    let mut out = String::new();
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        out.push_str(&format!("<b>📋 {}</b>", escape_html(t)));
    }
    // Fold the design prose into the card (#621) when there is no checklist
    // yet (Editing, before the seed): the card is the single surface carrying
    // title + prose + keyboard, so the reply stays clean and nothing splits
    // across messages. Once tasks exist (Active) the checklist is the live
    // view and the prose drops off the card.
    if checklist.is_none()
        && let Some(p) = prose.map(str::trim).filter(|p| !p.is_empty())
    {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("<blockquote expandable>");
        out.push_str(&escape_html(p));
        out.push_str("</blockquote>");
    }
    if let Some(rows) = checklist {
        for row in rows {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&escape_html(row));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Create or update the session's plan card to reflect the live plan state,
/// carrying `plan_kb`. Removes the card when the plan is gone.
pub(crate) async fn refresh_plan_card(
    bot: &Bot,
    chat: ChatId,
    thread_id: Option<ThreadId>,
    state: &Arc<TelegramState>,
    session_id: Uuid,
    plan_kb: PlanKb,
) {
    let (title, checklist) = load_plan_sections(session_id).await;
    // Load the design prose for the card when there are no tasks yet (Editing).
    // The flow message already loads this via load_plan_prose; here we flatten
    // the per-heading sections into a single preview string for the card's
    // expandable blockquote. The full .md stays accessible via /show-plan.
    let prose = if checklist.is_none() {
        load_plan_prose(session_id).await.map(|sections| {
            let mut out = String::new();
            for sec in &sections {
                if let Some(ref h) = sec.heading {
                    if !out.is_empty() {
                        out.push_str("\n\n");
                    }
                    out.push_str(h);
                }
                if !sec.body.trim().is_empty() {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&sec.body);
                }
            }
            crate::utils::truncate_str(&out, 2800).to_string()
        })
    } else {
        None
    };
    let Some(html) =
        render_plan_card_html(title.as_deref(), checklist.as_deref(), prose.as_deref())
    else {
        remove_plan_card(bot, chat, state, session_id).await;
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
                state.set_plan_card(session_id, mid, signature).await;
                return;
            }
            Err(e) => {
                let es = e.to_string();
                if es.contains("message is not modified") {
                    state.set_plan_card(session_id, mid, signature).await;
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
        Ok(m) => state.set_plan_card(session_id, m.id, signature).await,
        Err(e) => tracing::warn!("Telegram plan card create failed: {e}"),
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
    if let Some(mid) = state.take_plan_card(session_id).await
        && let Err(e) = bot.delete_message(chat, mid).await
    {
        tracing::debug!("Telegram plan card delete failed ({mid:?}): {e}");
    }
}
