//! Telegram-side rendering for the OPTIONAL `suggest_followups` tool (#597).
//!
//! Unlike `follow_up_question` this is non-blocking: the agent surfaces
//! `ProgressEvent::SuggestedFollowups`, and we post an inline keyboard under the
//! finished response with one button per suggestion. Tapping a button injects
//! that suggestion as the user's next message (a fresh turn) — see the
//! `followup:` arm in the callback dispatcher. Typing your own message is always
//! available and just starts a normal turn; there is no oneshot and no timeout.

use std::sync::Arc;

use teloxide::payloads::SendMessageSetters;
use teloxide::types::{ChatId, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode, ThreadId};
use uuid::Uuid;

use super::TelegramState;

/// Callback-data prefix for a tapped follow-up suggestion: `followup:<session>:<idx>`.
pub(crate) const FOLLOWUP_PREFIX: &str = "followup:";

/// What the suggestion block becomes once one of its options is tapped.
///
/// Replaces the prompt and its keyboard in place. The Bot API has no
/// send-as-user, so posting the choice as a new message renders a
/// user-chosen continuation under the bot's name, avatar and badge. A `>`
/// quote does not change that: the bubble is still labelled as the bot
/// (#844). Editing the block reads as a selected control instead.
pub(crate) fn picked_block(text: &str) -> String {
    format!("\u{25b6}\u{fe0f} {text}")
}

/// Last-resort record when the suggestion block cannot be edited, because it
/// is too old or no longer accessible. Worse attribution than editing, but
/// losing the record of what was chosen is worse still.
pub(crate) fn echo_fallback(text: &str) -> String {
    format!("> \u{25b6}\u{fe0f} {text}")
}

/// Post the follow-up suggestion buttons under the response and stash the option
/// list on state so the tap handler can resolve `idx -> text`. No-op on empty.
pub(crate) async fn render_suggestions(
    bot: &teloxide::Bot,
    state: &Arc<TelegramState>,
    session_id: Uuid,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    options: Vec<String>,
) {
    use teloxide::prelude::Requester;

    if options.is_empty() {
        return;
    }

    // One button per suggestion, single column so long labels stay readable.
    // The absolute index is encoded in the callback data; the text itself can
    // exceed Telegram's 64-byte callback-data limit, so we never put it there.
    let rows: Vec<Vec<InlineKeyboardButton>> = options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let label = if opt.chars().count() > 60 {
                let mut s: String = opt.chars().take(57).collect();
                s.push_str("...");
                s
            } else {
                opt.clone()
            };
            vec![InlineKeyboardButton::callback(
                label,
                format!("{FOLLOWUP_PREFIX}{session_id}:{i}"),
            )]
        })
        .collect();

    state.set_pending_followups(session_id, options).await;

    let keyboard = InlineKeyboardMarkup::new(rows);
    let mut req = bot
        .send_message(chat_id, "\u{1f4a1} Suggested next:")
        .reply_markup(keyboard);
    req = req.parse_mode(ParseMode::Html);
    if let Some(tid) = thread_id {
        req = req.message_thread_id(tid);
    }
    if let Err(e) = req.await {
        tracing::warn!("Telegram suggest_followups: send failed: {e}");
        // The buttons never landed — drop the stash so a stale entry can't
        // swallow an unrelated future tap.
        state.clear_pending_followups(session_id).await;
    }
}
