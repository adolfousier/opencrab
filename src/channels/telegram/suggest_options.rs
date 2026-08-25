//! Telegram-side rendering for the OPTIONAL `suggest_options` tool (#597).
//!
//! Non-blocking: the agent surfaces
//! `ProgressEvent::SuggestedOptions`, and we post an inline keyboard under the
//! finished response with one button per suggestion. Tapping a button injects
//! that suggestion as the user's next message (a fresh turn) — see the
//! `followup:` arm in the callback dispatcher. Typing your own message is always
//! available and just starts a normal turn; there is no oneshot and no timeout.

use std::sync::Arc;

use teloxide::payloads::{EditMessageTextSetters, SendMessageSetters};
use teloxide::types::{ChatId, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode, ThreadId};
use uuid::Uuid;

use super::TelegramState;

/// Callback-data prefix for a tapped follow-up suggestion: `followup:<session>:<idx>`.
pub(crate) const FOLLOWUP_PREFIX: &str = "followup:";

/// Label-length budget above which Telegram folds suggestions into the body

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

/// Button-width calibration, measured 2026-08-25 on Alexey's client
/// (`sendRichMessage` probes, messages 29975 + 29991): a single full-width
/// button fits <=50 chars on one line and wraps by 54; shared rows only
/// survive MICRO labels (Yes/No pairs fit; 11+8=19 chars total wraps).
pub(crate) const MAX_BUTTON_CHARS: usize = 50;
/// Longest label allowed to share one row with its siblings (measured:
/// 3-7 char words sit side by side without wrapping).
const SHARED_ROW_MAX_CHARS: usize = 8;
/// Tap ergonomics (Alexey, 2026-08-25): numbered buttons never pack more
/// than 4 per row, so every target stays big enough for a finger.
const MAX_NUMBERS_PER_ROW: usize = 4;

/// Which shape the suggestion controls take for a given option list.
/// Tiers are measured, not guessed — see [`MAX_BUTTON_CHARS`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SuggestLayout {
    /// Every label short AND few options: all buttons share ONE row.
    SharedRow,
    /// Every label fits a full-width button: one button per row.
    Column,
    /// Some label too long even full-width: texts fold into the message
    /// body as a numbered list, buttons collapse to bare numbers packed
    /// [`MAX_NUMBERS_PER_ROW`] per row.
    NumberedProse,
}

fn pick_layout(options: &[String]) -> SuggestLayout {
    let width = |o: &String| o.chars().count();
    if options.len() <= MAX_NUMBERS_PER_ROW
        && options.iter().all(|o| width(o) <= SHARED_ROW_MAX_CHARS)
    {
        SuggestLayout::SharedRow
    } else if options.iter().all(|o| width(o) <= MAX_BUTTON_CHARS) {
        SuggestLayout::Column
    } else {
        SuggestLayout::NumberedProse
    }
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

/// The suggestion controls as native rich-button rows (Bot API 10.3
/// `<tg-button-row>`), laid out per the measured ladder. Primary style
/// throughout — picked over app-default after Alexey compared both live.
/// Callback payloads stay `followup:<session>:<idx>`, so taps route through
/// the existing callback dispatcher unchanged regardless of surface.
fn suggestion_rows_rich_html(options: &[String], session_id: Uuid) -> String {
    let btn = |i: usize, label: &str| {
        format!(
            "<tg-button type=\"callback_data\" data=\"{FOLLOWUP_PREFIX}{session_id}:{i}\" \
             style=\"primary\">{}</tg-button>",
            super::markdown::escape_html(label)
        )
    };
    match pick_layout(options) {
        SuggestLayout::SharedRow => format!(
            "<tg-button-row>{}</tg-button-row>",
            options
                .iter()
                .enumerate()
                .map(|(i, opt)| btn(i, opt))
                .collect::<String>()
        ),
        SuggestLayout::Column => options
            .iter()
            .enumerate()
            .map(|(i, opt)| format!("<tg-button-row>{}</tg-button-row>", btn(i, opt)))
            .collect::<Vec<_>>()
            .join("\n"),
        SuggestLayout::NumberedProse => (0..options.len())
            .map(|i| btn(i, &(i + 1).to_string()))
            .collect::<Vec<_>>()
            .chunks(MAX_NUMBERS_PER_ROW)
            .map(|c| format!("<tg-button-row>{}</tg-button-row>", c.concat()))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub(crate) async fn render_suggestions(
    bot: &teloxide::Bot,
    state: &Arc<TelegramState>,
    session_id: Uuid,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    options: Vec<String>,
    // Merge candidate captured by deliver_final_response: the bubble the final
    // response landed in, whatever surface sent it (classic HTML, or table-free
    // rich markdown). Some = attach the controls to THAT bubble — one message
    // instead of two, no "Suggested next" header. None or failed edit =
    // standalone fallback below.
    merge_host: Option<super::state::MergeBubble>,
) {
    use teloxide::prelude::Requester;

    if options.is_empty() {
        return;
    }

    // Layout tiers are measured, not guessed (see MAX_BUTTON_CHARS): short
    // labels share one row, medium labels get a full-width row each, and
    // anything longer folds into the body as a numbered list with compact
    // number buttons (<=4 per row). The absolute index is encoded in the
    // callback data; the option text itself can exceed Telegram's 64-byte
    // callback-data limit, so we never put it there.
    let layout = pick_layout(&options);
    let text_btn = |i: usize, opt: &str| {
        InlineKeyboardButton::callback(
            opt.to_string(),
            format!("{FOLLOWUP_PREFIX}{session_id}:{i}"),
        )
    };
    let num_btn = |i: usize| {
        InlineKeyboardButton::callback(
            (i + 1).to_string(),
            format!("{FOLLOWUP_PREFIX}{session_id}:{i}"),
        )
    };
    let rows: Vec<Vec<InlineKeyboardButton>> = match layout {
        SuggestLayout::SharedRow => vec![options
            .iter()
            .enumerate()
            .map(|(i, opt)| text_btn(i, opt))
            .collect()],
        SuggestLayout::Column => options
            .iter()
            .enumerate()
            .map(|(i, opt)| vec![text_btn(i, opt)])
            .collect(),
        SuggestLayout::NumberedProse => {
            let all: Vec<InlineKeyboardButton> =
                (0..options.len()).map(num_btn).collect();
            all.chunks(MAX_NUMBERS_PER_ROW)
                .map(|c| c.to_vec())
                .collect()
        }
    };

    let keyboard = InlineKeyboardMarkup::new(rows);

    // Primary path: MERGE onto the answer bubble (#tg-suggest-merge). Prose
    // mode appends the numbered list under the answer text; button modes add
    // nothing on the classic surface (the buttons carry everything). Rich
    // bubbles additionally get native <tg-button-row> controls INSIDE the
    // message body.
    let mut placed = false;
    if let Some(host) = merge_host {
        let mid = host.message_id;
        // Base body + surface: classic bubbles keep their exact delivered
        // HTML; rich bubbles re-render from the captured markdown. Table-
        // bearing answers never reach this arm as Markdown — capture skips
        // them because rich HTML input flattens tables (#679).
        let (mut new_html, rich) = match host.body {
            super::state::BubbleBody::Html(html) => (html, false),
            super::state::BubbleBody::Markdown(md) => {
                (super::rich::markdown_to_html(&md), true)
            }
        };
        if layout == SuggestLayout::NumberedProse {
            new_html.push_str("\n");
            new_html.push_str(folded_list_html(&options).trim_start());
        }
        if rich {
            new_html.push('\n');
            new_html.push_str(&suggestion_rows_rich_html(&options, session_id));
        }
        let outcome: Result<(), String> = if rich {
            super::rich::api::edit_rich_html(
                bot.api_url().as_str(),
                bot.token(),
                chat_id.0,
                mid.0,
                &new_html,
                None,
                "turn",
                "-",
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
        } else {
            bot.edit_message_text(chat_id, mid, &new_html)
                .parse_mode(ParseMode::Html)
                .reply_markup(keyboard.clone())
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        };
        match outcome {
            Ok(()) => {
                placed = true;
                state
                    .set_pending_followups(
                        session_id,
                        options.clone(),
                        Some(super::state::MergedHost {
                            message_id: mid,
                            html: new_html,
                            rich,
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
    // — prose mode shows just the numbered list, button modes need SOME text
    // for the Bot API to accept the message, so they degrade to the bare 💡.
    if !placed {
        let body = if layout == SuggestLayout::NumberedProse {
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
mod layout_tests {
    use super::*;

    #[test]
    fn short_few_options_share_one_row() {
        let opts = vec!["Yes".to_string(), "No".to_string(), "Skip".to_string()];
        assert_eq!(pick_layout(&opts), SuggestLayout::SharedRow);
    }

    #[test]
    fn five_short_options_do_not_share_a_row() {
        // >4 tap targets in one row = buttons too small for a finger
        // (Alexey, 2026-08-25). They drop to the Column tier instead.
        let opts: Vec<String> = ["alpha", "beta", "gamma", "delta", "eps"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(opts.iter().all(|o| o.chars().count() <= SHARED_ROW_MAX_CHARS));
        assert_eq!(pick_layout(&opts), SuggestLayout::Column);
    }

    #[test]
    fn nine_char_label_kills_shared_row() {
        let opts = vec!["Yes".to_string(), "x".repeat(9)];
        assert_eq!(pick_layout(&opts), SuggestLayout::Column);
    }

    #[test]
    fn boundary_exactly_50_stays_column() {
        // Measured: 50ch fits one line (probe P2), so 50 stays a button.
        let opts = vec!["x".repeat(50)];
        assert_eq!(pick_layout(&opts), SuggestLayout::Column);
    }

    #[test]
    fn over_50_folds_to_numbered_prose() {
        // Measured: 54ch wraps (probe P3); cap is exclusive at 50.
        let opts = vec!["Ship it".to_string(), "x".repeat(51)];
        assert_eq!(pick_layout(&opts), SuggestLayout::NumberedProse);
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
}
