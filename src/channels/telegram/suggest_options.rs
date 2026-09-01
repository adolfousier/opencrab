//! Telegram-side rendering for the OPTIONAL `suggest_options` tool (#597).
//!
//! Non-blocking: the agent surfaces
//! `ProgressEvent::SuggestedOptions`, and we post an inline keyboard under the
//! finished response with one button per suggestion. Tapping a button injects
//! that suggestion as the user's next message (a fresh turn) — see the
//! `followup:` arm in the callback dispatcher. Typing your own message is always
//! available and just starts a normal turn; there is no oneshot and no timeout.

use std::sync::Arc;
use std::time::Duration;

use teloxide::payloads::{
    EditMessageReplyMarkupSetters, EditMessageTextSetters, SendMessageSetters,
};
use teloxide::types::{
    ChatId, InlineKeyboardButton, InlineKeyboardMarkup, MessageId, ParseMode, ThreadId,
};
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

/// What a tapped suggestion block is rewritten into (#39).
///
/// The pick record is baked into the body BEFORE any transport arm runs,
/// so no arm can drop it: a merged host — rich or classic — keeps its
/// answer HTML and gains the pick record; a standalone block becomes the
/// pick record. Before this shape the append lived inside the rich arm
/// only, and the classic merged host silently lost the choice (owner
/// report 2026-08-29 23:51Z).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PickRewrite {
    /// Rich merged host: body rides `edit_rich_html`, empty markup strips
    /// the dead buttons.
    RichHost(String),
    /// Classic merged host: body rides `edit_message_text` + empty markup
    /// to strip the dead buttons.
    ClassicHost(String),
    /// Standalone suggestion block: body rides plain `edit_message_text`.
    Standalone(String),
    // NOTE (#55): glued hosts (table-bearing rich answers, keyboard attached
    // via edit_message_reply_markup) never reach pick_rewrite — the tap
    // handler routes them by the `glued` flag BEFORE calling this function,
    // because a tap on a glued host must NEVER rewrite the body.
}

/// The single construction site for a post-tap body (#39).
///
/// `host` is `(html, rich)` of the merged host bubble when the tapped
/// message IS it; merged host → answer HTML + pick record, standalone →
/// pick record alone.
pub(crate) fn pick_rewrite(host: Option<(&str, bool)>, picked: String) -> PickRewrite {
    match host {
        Some((full, rich)) => {
            let body = format!("{full}\n\n{picked}");
            if rich {
                PickRewrite::RichHost(body)
            } else {
                PickRewrite::ClassicHost(body)
            }
        }
        None => PickRewrite::Standalone(picked),
    }
}

/// Button-width calibration, measured 2026-08-25 on Alexey's client
/// (`sendRichMessage` probes, messages 29975 + 29991): a single full-width
/// button fits <=50 chars on one line and wraps by 54. (The original
/// "shared rows wrap at 11+8=19" claim was DISPROVEN by the 2026-08-31
/// probes — see [`SHARED_ROW_MAX_CHARS`] and fork issue #49.)
pub(crate) const MAX_BUTTON_CHARS: usize = 50;
/// Longest label allowed to share one row with its siblings. Recalibrated
/// 2026-08-31 (live probes, fork issue #49): the real constraint is
/// ROW-TOTAL width (~32 chars shared equally across the row, before the
/// body-width-dependent truncation Alexey observed), not per-label chars —
/// 4×8=32 held, 15+18=33 clipped ~2 symbols, and bubble width varies with
/// the message body, so 12 keeps a safety margin under the worst bubble
/// while doubling information per row vs the old 8.
pub(crate) const SHARED_ROW_MAX_CHARS: usize = 12;
/// Tap ergonomics (Alexey, 2026-08-25): numbered buttons never pack more
/// than 4 per row, so every target stays big enough for a finger.
pub(crate) const MAX_NUMBERS_PER_ROW: usize = 4;

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

pub(crate) fn pick_layout(options: &[String]) -> SuggestLayout {
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
pub(crate) fn folded_list_html(options: &[String]) -> String {
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

/// Rich-surface variant of [`folded_list_html`]: the rich (sendRichMessage)
/// HTML input collapses raw newlines, so each numbered line rides its own
/// `<p>` block or the options list renders as one long line (#1226).
/// Classic ParseMode::Html preserves raw newlines and keeps using
/// [`folded_list_html`].
pub(crate) fn folded_list_html_p(options: &[String]) -> String {
    options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            format!(
                "<p>{}. {}</p>",
                i + 1,
                super::markdown::format_inline(&super::markdown::escape_html(opt))
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

/// The suggestion controls as native rich-button rows (Bot API 10.3
/// #59: cut every `<tg-button-row>…</tg-button-row>` span out of a rich
/// bubble body — the inverse of [`suggestion_rows_rich_html`]. Used by the
/// host-aware stale-shell strip: the #597 clear killed the stash, but the
/// buttons keep rendering inside the body until the body is rewritten clean.
pub(crate) fn strip_button_rows(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<tg-button-row>") {
        match rest[start..].find("</tg-button-row>") {
            Some(rel) => {
                let end = start + rel + "</tg-button-row>".len();
                out.push_str(&rest[..start]);
                rest = &rest[end..];
            }
            None => break, // unterminated span: leave the remainder untouched
        }
    }
    out.push_str(rest);
    out.trim_end().to_string()
}

/// #59 (DRY): the empty reply-markup used to strip dead keyboards — one
/// construction site instead of one per strip arm.
pub(crate) fn empty_keyboard() -> teloxide::types::InlineKeyboardMarkup {
    teloxide::types::InlineKeyboardMarkup::new(
        Vec::<Vec<teloxide::types::InlineKeyboardButton>>::new(),
    )
}

/// `<tg-button-row>`), laid out per the measured ladder. Primary style
/// throughout — picked over app-default after Alexey compared both live.
/// Callback payloads stay `followup:<session>:<idx>`, so taps route through
/// the existing callback dispatcher unchanged regardless of surface.
pub(crate) fn suggestion_rows_rich_html(options: &[String], token: &str) -> String {
    let btn = |i: usize, label: &str| {
        format!(
            "<tg-button type=\"callback_data\" data=\"{FOLLOWUP_PREFIX}{token}:{i}\" \
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

#[allow(clippy::too_many_arguments)] // #31: trailer rides the existing arg set
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
    // #31: the post-halt sign-off run reclaimed from the flow (after the
    // suggest_options Tool entry). Rich merge: embedded as a paragraph AFTER
    // the in-body button rows (one message, never removed). Every other
    // shape: its own bubble after placement — content, not chrome, so it
    // ships even when the buttons die.
    trailer: Option<String>,
) {
    if options.is_empty() {
        // Stash cleared between delivery and render (mid-turn tap, newer
        // turn) — the trailer still ships (#31); there is nothing to
        // register and no keyboard to place.
        if let Some(t) = &trailer {
            send_trailer_bubble(bot, chat_id, thread_id, t).await;
        }
        return;
    }

    // Per-keyboard identity (#1217): register BEFORE building buttons so the
    // opaque token rides in every callback payload; taps resolve against this
    // exact set even when a newer turn registers its own keyboard meanwhile.
    let token = state
        .register_pending_followups(session_id, options.clone())
        .await;

    // Layout tiers are measured, not guessed (see MAX_BUTTON_CHARS): short
    // labels share one row, medium labels get a full-width row each, and
    // anything longer folds into the body as a numbered list with compact
    // number buttons (<=4 per row). The absolute index is encoded in the
    // callback data; the option text itself can exceed Telegram's 64-byte
    // callback-data limit, so we never put it there.
    let layout = pick_layout(&options);
    let text_btn = |i: usize, opt: &str| {
        InlineKeyboardButton::callback(opt.to_string(), format!("{FOLLOWUP_PREFIX}{token}:{i}"))
    };
    let num_btn = |i: usize| {
        InlineKeyboardButton::callback((i + 1).to_string(), format!("{FOLLOWUP_PREFIX}{token}:{i}"))
    };
    let rows: Vec<Vec<InlineKeyboardButton>> = match layout {
        SuggestLayout::SharedRow => vec![
            options
                .iter()
                .enumerate()
                .map(|(i, opt)| text_btn(i, opt))
                .collect(),
        ],
        SuggestLayout::Column => options
            .iter()
            .enumerate()
            .map(|(i, opt)| vec![text_btn(i, opt)])
            .collect(),
        SuggestLayout::NumberedProse => {
            let all: Vec<InlineKeyboardButton> = (0..options.len()).map(num_btn).collect();
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
    // message body. Both placement payloads are built ONCE — before the first
    // attempt — so a Retry-After deferral (#30) re-sends byte-identical
    // content instead of re-deriving it.
    let merge_payload: Option<MergePayload> = merge_host.map(|host| {
        let mid = host.message_id;
        // #55 glue tier: table-bearing rich answers capture body=None — a
        // body-rewriting merge would flatten the table (#679), so the
        // keyboard rides `edit_message_reply_markup` instead and the body
        // is never touched.
        let Some(body) = host.body else {
            return MergePayload {
                message_id: mid,
                new_html: String::new(),
                rich: false,
                glue: true,
            };
        };
        // Base body + surface: classic bubbles keep their exact delivered
        // HTML; rich bubbles re-render from the captured markdown.
        let (mut new_html, rich) = match body {
            super::state::BubbleBody::Html(html) => (html, false),
            super::state::BubbleBody::Markdown(md) => (super::rich::markdown_to_html_p(&md), true),
        };
        if layout == SuggestLayout::NumberedProse {
            if rich {
                // Rich HTML collapses raw newlines (#1226): the numbered list
                // rides <p>-wrapped lines so it keeps its shape; classic hosts
                // preserve the raw newline join via folded_list_html.
                new_html.push_str(&folded_list_html_p(&options));
            } else {
                new_html.push('\n');
                new_html.push_str(folded_list_html(&options).trim_start());
            }
        }
        if rich {
            new_html.push('\n');
            new_html.push_str(&suggestion_rows_rich_html(&options, &token));
            // #31: the sign-off paragraph rides AFTER the button rows — one
            // message carries answer + controls + trailer, in that order.
            if let Some(t) = &trailer {
                new_html.push('\n');
                new_html.push_str(&super::rich::markdown_to_html_p(t));
            }
        }
        MergePayload {
            message_id: mid,
            new_html,
            rich,
            glue: false,
        }
    });

    // Standalone fallback (no merge candidate, no glue target, or the edit
    // lost a race / grew too old): the header sentence is still gone per
    // #tg-suggest-merge — prose mode shows just the numbered list, button
    // modes need SOME text for the Bot API to accept the message. #55: the
    // bare lamp is retired — "Pick one:" says the same with words.
    let standalone_body = if layout == SuggestLayout::NumberedProse {
        folded_list_html(&options).trim_start().to_string()
    } else {
        String::from("Pick one:")
    };

    let option_count = options.len();
    match place_once(
        bot,
        state,
        chat_id,
        thread_id,
        &token,
        option_count,
        &keyboard,
        merge_payload.as_ref(),
        &standalone_body,
    )
    .await
    {
        Ok(()) => {
            // #31: send the trailer bubble only if it wasn't already embedded
            // in a rich merge. The followup host is attached only on merge
            // success, so peek_followup_host tells us if the merge landed.
            // If it did and was rich, the trailer is already in the HTML.
            let embedded = state
                .peek_followup_host(&token)
                .await
                .map(|h| h.rich && trailer.is_some())
                .unwrap_or(false);
            if !embedded && let Some(t) = &trailer {
                send_trailer_bubble(bot, chat_id, thread_id, t).await;
            }
        }
        Err(PlaceErr::Fatal(e)) => {
            tracing::warn!("Telegram suggest_options: send failed: {e}");
            // The buttons never landed — drop the stash so a stale entry can't
            // swallow an unrelated future tap.
            state.drop_pending_followup(&token).await;
        }
        Err(PlaceErr::RetryAfter(wait)) => {
            // #30: a 429 here used to drop the stash at once — but BOTH arms
            // die inside the same flood window (the standalone send followed
            // the merge edit by 22ms into the same 41s ban), and the buttons
            // were lost forever. Keep the stash instead and re-place after
            // the TRUE Retry-After, budget-capped.
            tracing::warn!(
                "Telegram suggest_options: placement hit Retry-After {}s (token {token}) — \
                 stash kept, deferring",
                wait.as_secs()
            );
            let bot = bot.clone();
            let state = state.clone();
            let token = token.clone();
            let keyboard = keyboard.clone();
            let trailer = trailer.clone();
            tokio::spawn(async move {
                let mut wait = wait;
                for attempt in 1..=MAX_DEFERRED_PLACEMENT_ATTEMPTS {
                    use rand::Rng;
                    // Jitter so several placements deferred by the same ban
                    // don't all re-hit Telegram on the same second.
                    let jitter = Duration::from_millis(rand::rng().random_range(0..=2000));
                    tokio::time::sleep(wait + jitter).await;
                    match place_once(
                        &bot,
                        &state,
                        chat_id,
                        thread_id,
                        &token,
                        option_count,
                        &keyboard,
                        merge_payload.as_ref(),
                        &standalone_body,
                    )
                    .await
                    {
                        Ok(()) => {
                            tracing::info!(
                                "Telegram suggest_options: deferred placement {attempt}/\
                                 {MAX_DEFERRED_PLACEMENT_ATTEMPTS} landed (token {token})"
                            );
                            // #31: send the trailer bubble only if it wasn't
                            // already embedded in a rich merge.
                            let embedded = state
                                .peek_followup_host(&token)
                                .await
                                .map(|h| h.rich && trailer.is_some())
                                .unwrap_or(false);
                            if !embedded && let Some(t) = &trailer {
                                send_trailer_bubble(&bot, chat_id, thread_id, t).await;
                            }
                            return;
                        }
                        Err(PlaceErr::Fatal(e)) => {
                            tracing::warn!(
                                "Telegram suggest_options: deferred placement {attempt} \
                                 failed permanently: {e}"
                            );
                            state.drop_pending_followup(&token).await;
                            return;
                        }
                        Err(PlaceErr::RetryAfter(w)) => {
                            tracing::warn!(
                                "Telegram suggest_options: deferred placement {attempt} hit \
                                 Retry-After {}s again (token {token})",
                                w.as_secs()
                            );
                            wait = w;
                        }
                    }
                }
                tracing::warn!(
                    "Telegram suggest_options: placement budget spent after \
                     {MAX_DEFERRED_PLACEMENT_ATTEMPTS} deferred attempts (token {token}) — dropping"
                );
                state.drop_pending_followup(&token).await;
            });
        }
    }
}

/// Pre-built merge-edit payload (#30): computed ONCE per suggestion block so
/// a deferred re-placement after a Retry-After re-sends byte-identical
/// content. `rich` picks the wire: rich bubbles edit via the rich API with
/// in-body button rows, classic bubbles via editMessageText + reply_markup.
#[derive(Clone)]
struct MergePayload {
    message_id: MessageId,
    new_html: String,
    rich: bool,
    /// #55 glue tier: attach via `edit_message_reply_markup` only — the
    /// host body is not merge-safe (table) and must never be re-sent.
    glue: bool,
}

/// Placement error class (#30): decides whether the stash survives the
/// failure.
enum PlaceErr {
    /// Telegram answered 429 with a Retry-After — the placement may succeed
    /// once the window passes, so the stash MUST survive the wait.
    RetryAfter(Duration),
    /// Anything else: retrying cannot fix it; the stash drops as before.
    Fatal(String),
}

/// Deferred placement attempts after a Retry-After, on top of the inline
/// first pass (#30). Two deferrals cap the chase at roughly two flood
/// windows while comfortably covering the 31–42s windows observed in the
/// #30 ledger.
const MAX_DEFERRED_PLACEMENT_ATTEMPTS: u32 = 2;

/// Wait used when only the rich arm's stringified "(429)" survives — see
/// [`classify_rich_err`].
const RICH_429_FALLBACK_WAIT_SECS: u64 = 30;

fn classify_request_err(e: teloxide::RequestError) -> PlaceErr {
    match e {
        teloxide::RequestError::RetryAfter(secs) => PlaceErr::RetryAfter(secs.duration()),
        other => PlaceErr::Fatal(other.to_string()),
    }
}

/// The rich arm buries Telegram's exact retry_after inside its own internal
/// retry loop (`post_rich`) and surfaces only an anyhow string, so
/// classification keys off the status marker. The wait is a middle-of-the-
/// road default: the rich path already slept out the true value
/// RICH_MAX_RETRIES times before bailing, and the observed flood windows
/// run 31–42s (#30 ledger).
fn classify_rich_err(e: &str) -> PlaceErr {
    if e.contains("(429)") {
        PlaceErr::RetryAfter(Duration::from_secs(RICH_429_FALLBACK_WAIT_SECS))
    } else {
        PlaceErr::Fatal(e.to_string())
    }
}

/// One placement pass (#30): merge onto the answer bubble when a payload
/// exists, standalone otherwise. RetryAfter-class errors bubble up so the
/// caller can defer with the stash intact; anything else is Fatal.
#[allow(clippy::too_many_arguments)]
async fn place_once(
    bot: &teloxide::Bot,
    state: &Arc<TelegramState>,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    token: &str,
    option_count: usize,
    keyboard: &InlineKeyboardMarkup,
    merge: Option<&MergePayload>,
    standalone_body: &str,
) -> Result<(), PlaceErr> {
    use teloxide::prelude::Requester;

    if let Some(mp) = merge {
        let mid = mp.message_id;
        let outcome: Result<(), PlaceErr> = if mp.glue {
            // #55 glue tier: attach the keyboard without touching the body.
            bot.edit_message_reply_markup(chat_id, mid)
                .reply_markup(keyboard.clone())
                .await
                .map(|_| ())
                .map_err(classify_request_err)
        } else if mp.rich {
            super::rich::api::edit_rich_html(
                bot.api_url().as_str(),
                bot.token(),
                chat_id.0,
                mid.0,
                &mp.new_html,
                None,
                "turn",
                "-",
            )
            .await
            .map_err(|e| classify_rich_err(&e.to_string()))
        } else {
            bot.edit_message_text(chat_id, mid, &mp.new_html)
                .parse_mode(ParseMode::Html)
                .reply_markup(keyboard.clone())
                .await
                .map(|_| ())
                .map_err(classify_request_err)
        };
        match outcome {
            Ok(()) => {
                // Name the arm, the host message and the token so any tap can
                // be mapped back to its panel from logs alone.
                tracing::info!(
                    "Telegram suggest_options: keyboard merged onto msg {mid} \
                     ({} host, token {token}, {option_count} options)",
                    if mp.glue {
                        "glue"
                    } else if mp.rich {
                        "rich"
                    } else {
                        "classic"
                    }
                );
                state
                    .attach_followup_host(
                        token,
                        super::state::MergedHost {
                            message_id: mid,
                            html: mp.new_html.clone(),
                            rich: mp.rich,
                            glued: mp.glue,
                        },
                    )
                    .await;
                return Ok(());
            }
            Err(PlaceErr::RetryAfter(wait)) => return Err(PlaceErr::RetryAfter(wait)),
            Err(PlaceErr::Fatal(e)) => {
                tracing::warn!(
                    "Telegram suggest_options: merge onto msg {mid} failed ({e}) — standalone fallback"
                );
            }
        }
    }

    let mut req = bot
        .send_message(chat_id, standalone_body)
        .reply_markup(keyboard.clone());
    req = req.parse_mode(ParseMode::Html);
    if let Some(tid) = thread_id {
        req = req.message_thread_id(tid);
    }
    match req.await {
        Ok(msg) => {
            tracing::info!(
                "Telegram suggest_options: standalone block msg {} \
                 (token {token}, {option_count} options)",
                msg.id
            );
            Ok(())
        }
        Err(e) => Err(classify_request_err(e)),
    }
}

/// The #31 sign-off trailer as its own bubble: Markdown rendered with the
/// same HTML wire as every other telegram bubble, thread-routed, with a
/// plain-text retry when the parse-mode send is rejected — a malformed
/// markdown construct must degrade the sign-off, never discard it
/// (keep-never-discard is the whole point of #31).
async fn send_trailer_bubble(
    bot: &teloxide::Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    trailer: &str,
) {
    use teloxide::prelude::Requester;

    let html = super::markdown::markdown_to_telegram_html(trailer);
    let mut req = bot.send_message(chat_id, html).parse_mode(ParseMode::Html);
    if let Some(tid) = thread_id {
        req = req.message_thread_id(tid);
    }
    match req.await {
        Ok(msg) => {
            tracing::info!("Telegram: #31 trailer bubble delivered as msg {}", msg.id);
        }
        Err(e) => {
            tracing::warn!("Telegram: #31 trailer bubble HTML send failed ({e}) — retrying plain");
            let mut plain = bot.send_message(chat_id, trailer);
            if let Some(tid) = thread_id {
                plain = plain.message_thread_id(tid);
            }
            match plain.await {
                Ok(msg) => {
                    tracing::info!(
                        "Telegram: #31 trailer bubble delivered plain as msg {}",
                        msg.id
                    );
                }
                Err(e2) => {
                    tracing::warn!("Telegram: #31 trailer bubble dropped after plain retry: {e2}");
                }
            }
        }
    }
}