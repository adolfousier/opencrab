//! Telegram-side rendering for the OPTIONAL `suggest_options` tool (#597).
//!
//! Non-blocking: the agent surfaces
//! `ProgressEvent::SuggestedOptions`, and we post an inline keyboard under the
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
pub(crate) fn picked_block(text: &str, chooser: Option<&str>) -> String {
    match chooser {
        // Name the member who chose it (#893). Without this the record reads as
        // an anonymous line from the bot, which in a group says nothing about
        // who acted. The Bot API cannot post AS a user, but the callback query
        // carries the tapper's identity and it was simply discarded.
        Some(name) if !name.trim().is_empty() => {
            format!("\u{25b6}\u{fe0f} {} \u{2014} {text}", name.trim())
        }
        _ => format!("\u{25b6}\u{fe0f} {text}"),
    }
}

/// Last-resort record when the suggestion block cannot be edited, because it
/// is too old or no longer accessible. Worse attribution than editing, but
/// losing the record of what was chosen is worse still.
pub(crate) fn echo_fallback(text: &str, chooser: Option<&str>) -> String {
    match chooser {
        Some(name) if !name.trim().is_empty() => {
            format!("> \u{25b6}\u{fe0f} {} \u{2014} {text}", name.trim())
        }
        _ => format!("> \u{25b6}\u{fe0f} {text}"),
    }
}

/// Post the follow-up suggestion buttons under the response and stash the option
/// list on state so the tap handler can resolve `idx -> text`. No-op on empty.
/// Fold-in is a FALLBACK (#1178 D3): full-text buttons primary, but if ANY
/// option exceeds 30 chars the texts fold into the message body as a
/// numbered list and the buttons collapse to one row of numbers (D4) —
/// a column of long labels is unreadable and a row of them overflows.
/// The stash always holds the ORIGINAL options either way, so taps
/// resolve verbatim text, never bare digits.
fn should_fold(options: &[String]) -> bool {
    options.iter().any(|o| o.chars().count() > 30)
}

/// The message body: bare header in full-text mode; header plus the numbered
/// option list when folded.
fn build_body(fold: bool, options: &[String]) -> String {
    if !fold {
        return String::from("\u{1f4a1} Suggested next:");
    }
    let mut b = String::from("\u{1f4a1} Suggested next:");
    for (i, opt) in options.iter().enumerate() {
        b.push_str(&format!("\n{}. {opt}", i + 1));
    }
    b
}

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

    // Fold-in is a FALLBACK (#1178 D3): full-text buttons primary, but if ANY
    // option exceeds 30 chars the texts fold into the message body as a
    // numbered list and the buttons collapse to one row of numbers (D4) —
    // a column of long labels is unreadable and a row of them overflows.
    // The stash always holds the ORIGINAL options either way, so taps
    // resolve verbatim text, never bare digits.
    let fold = should_fold(&options);
    let body = build_body(fold, &options);

    // Full-text mode: one button per suggestion in a single column so long
    // labels stay readable. Folded mode: one row of numeric buttons (D4).
    // The absolute index is encoded in the callback data; the option text
    // itself can exceed Telegram's 64-byte callback-data limit, so we never
    // put it there.
    let rows: Vec<Vec<InlineKeyboardButton>> = if fold {
        vec![
            options
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    InlineKeyboardButton::callback(
                        (i + 1).to_string(),
                        format!("{FOLLOWUP_PREFIX}{session_id}:{i}"),
                    )
                })
                .collect(),
        ]
    } else {
        options
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
            .collect()
    };

    state.set_pending_followups(session_id, options).await;

    let keyboard = InlineKeyboardMarkup::new(rows);
    let mut req = bot.send_message(chat_id, body).reply_markup(keyboard);
    req = req.parse_mode(ParseMode::Html);
    if let Some(tid) = thread_id {
        req = req.message_thread_id(tid);
    }
    if let Err(e) = req.await {
        tracing::warn!("Telegram suggest_options: send failed: {e}");
        // The buttons never landed — drop the stash so a stale entry can't
        // swallow an unrelated future tap.
        state.clear_pending_followups(session_id).await;
    }
}

#[cfg(test)]
mod fold_tests {
    use super::*;

    #[test]
    fn no_fold_when_all_options_short() {
        let opts = vec!["Ship it".to_string(), "Hold".to_string()];
        assert!(!should_fold(&opts));
        assert_eq!(build_body(false, &opts), "\u{1f4a1} Suggested next:");
    }

    #[test]
    fn folds_when_any_option_exceeds_30_chars() {
        let short = "Ship it".to_string();
        let long =
            "this is a very long option that definitely exceeds thirty characters".to_string();
        assert!(long.chars().count() > 30);
        let opts = vec![short.clone(), long.clone()];
        assert!(should_fold(&opts));
        let body = build_body(true, &opts);
        assert!(body.starts_with("\u{1f4a1} Suggested next:"));
        assert!(body.contains("1. Ship it"));
        assert!(body.contains(&format!("2. {long}")));
    }

    #[test]
    fn boundary_exactly_30_does_not_fold() {
        // 30 chars exactly: threshold is EXCLUSIVE (>30 folds).
        let exact = "x".repeat(30);
        let opts = vec![exact];
        assert!(!should_fold(&opts));
    }
}
