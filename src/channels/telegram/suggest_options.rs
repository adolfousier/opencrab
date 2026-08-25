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

/// Fold-in is a FALLBACK (#1178 D3): full-text buttons primary, but if ANY
/// option exceeds 30 chars the texts fold into the host message body as a
/// rich numbered list and the buttons collapse to one row of numbers (D4) —
/// a column of long labels is unreadable and a row of them overflows.
/// The stash always holds the ORIGINAL options either way, so taps
/// resolve verbatim text, never bare digits.
fn should_fold(options: &[String]) -> bool {
    options.iter().any(|o| o.chars().count() > 30)
}

/// The folded option list as rich HTML. REUSES the canonical inline
/// primitives from `super::markdown` — `escape_html` → `format_inline`,
/// the exact pair the outbound renderer's default line branch applies —
/// instead of a private formatter. Options are independent ONE-line texts,
/// so they deliberately skip document-level interpretation (a stray `|`
/// must not turn the list into a table); inline markup (`code`, bold) and
/// HTML escaping behave identically to every other Telegram surface.
/// No "Suggested next" header — the list rides directly under the answer
/// text in the same bubble (#tg-suggest-merge), so the label would only
/// duplicate what the buttons already say.
fn folded_list_html(options: &[String]) -> String {
    options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            format!(
                "{}. {}",
                i + 1,
                super::markdown::format_inline(&super::markdown::escape_html(opt))
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) async fn render_suggestions(
    bot: &teloxide::Bot,
    state: &Arc<TelegramState>,
    session_id: Uuid,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    options: Vec<String>,
    // Merge candidate captured by deliver_final_response: the bubble the final
    // response landed in (id + exact HTML). Some = attach the keyboard to THAT
    // bubble — one message instead of two, no "Suggested next" header. None or
    // failed edit = standalone fallback below.
    merge_host: Option<(teloxide::types::MessageId, String)>,
) {
    use teloxide::prelude::Requester;

    if options.is_empty() {
        return;
    }

    let fold = should_fold(&options);

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

    let keyboard = InlineKeyboardMarkup::new(rows);

    // Primary path: MERGE onto the answer bubble (#tg-suggest-merge). In fold
    // mode the rich numbered list is appended under the answer text; in full
    // mode the buttons carry everything and no text is added at all.
    let mut placed = false;
    if let Some((mid, ref html)) = merge_host {
        let mut new_html = html.clone();
        if fold {
            new_html.push_str("\n");
            new_html.push_str(folded_list_html(&options).trim_start());
        }
        match bot
            .edit_message_text(chat_id, mid, &new_html)
            .parse_mode(ParseMode::Html)
            .reply_markup(keyboard.clone())
            .await
        {
            Ok(_) => {
                placed = true;
                state
                    .set_pending_followups(
                        session_id,
                        options.clone(),
                        Some(super::state::MergedHost {
                            message_id: mid,
                            html: new_html,
                        }),
                    )
                    .await;
            }
            Err(e) => {
                tracing::warn!(
                    "Telegram suggest_options: merge onto msg {mid} failed ({e}) — standalone fallback"
                );
            }
        }
    }

    // Fallback (no merge candidate, or the edit lost a race / grew too old):
    // standalone block. The header sentence is still gone per #tg-suggest-merge
    // — folded mode shows just the rich list, full mode needs SOME text for the
    // Bot API to accept the message, so it degrades to the bare 💡 marker.
    if !placed {
        let body = if fold {
            folded_list_html(&options).trim_start().to_string()
        } else {
            String::from("\u{1f4a1}")
        };
        state
            .set_pending_followups(session_id, options.clone(), None)
            .await;
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
}

#[cfg(test)]
mod fold_tests {
    use super::*;

    #[test]
    fn no_fold_when_all_options_short() {
        let opts = vec!["Ship it".to_string(), "Hold".to_string()];
        assert!(!should_fold(&opts));
    }

    #[test]
    fn folded_list_is_rich_html_without_header() {
        let opts = vec!["Ship it".to_string(), "Review & merge".to_string()];
        let body = folded_list_html(&opts);
        // Header is gone (#tg-suggest-merge) — the list rides under the answer.
        assert!(!body.contains("Suggested next"));
        // Canonical renderer output: numbered lines, HTML-escaped text
        // (& → &amp; proves the shared pipeline escaped, not a private one).
        assert!(body.contains("1. Ship it"));
        assert!(body.contains("2. Review &amp; merge"));
    }

    #[test]
    fn folds_when_any_option_exceeds_30_chars() {
        let short = "Ship it".to_string();
        let long =
            "this is a very long option that definitely exceeds thirty characters".to_string();
        assert!(long.chars().count() > 30);
        let opts = vec![short.clone(), long.clone()];
        assert!(should_fold(&opts));
        let body = folded_list_html(&opts);
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
