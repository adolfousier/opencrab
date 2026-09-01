//! A tapped follow-up is recorded on its own block, not spoken by the bot (#844).
//!
//! Tapping a suggestion posted a new message containing the chosen text. The
//! Bot API has no send-as-user, so that bubble carries the bot's name, avatar
//! and badge, and a continuation the user chose reads as the bot saying it.
//!
//! #787 tried to fix this by prefixing the echo with a `>` quote. Quote
//! formatting sits inside a bubble that is still labelled as the bot, so the
//! attribution did not change. The block is now edited in place instead, which
//! reads as a selected control and drops the keyboard at the moment the rest of
//! the options stop working.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::channels::telegram::suggest_options::{
    PickRewrite, echo_fallback, mark_picked_button, pick_rewrite, picked_block,
    suggestion_rows_rich_html,
};

const CHOICE: &str = "Update the SKILL.md with the new callback routing";

#[test]
fn the_edited_block_is_not_quoted() {
    // A leading `>` is what #787 added. It renders as a quote inside a bot
    // bubble, which is the appearance being fixed.
    let block = picked_block(CHOICE, None);
    assert!(
        !block.starts_with('>'),
        "the in-place record must not be quoted: {block}"
    );
}

#[test]
fn the_edited_block_marks_the_choice_and_keeps_the_text() {
    let block = picked_block(CHOICE, None);
    assert!(block.starts_with('\u{25b6}'), "must lead with the marker");
    assert!(block.contains(CHOICE), "the chosen text must survive");
}

#[test]
fn the_fallback_stays_quoted() {
    // Only reached when the block cannot be edited. Quoting is the weaker
    // presentation, which is why it is the fallback and not the default.
    let echo = echo_fallback(CHOICE, None);
    assert!(
        echo.starts_with("> "),
        "fallback must remain a quote: {echo}"
    );
    assert!(echo.contains(CHOICE));
}

#[test]
fn the_two_shapes_differ() {
    // If these ever converge, the fix has been undone: the whole point is
    // that the in-place record does not look like the old echo.
    assert_ne!(picked_block(CHOICE, None), echo_fallback(CHOICE, None));
}

#[test]
fn text_is_passed_through_untouched() {
    // The suggestion may contain markdown, backticks or angle brackets. This
    // layer must not mangle them; escaping is md_to_html's job at the call
    // site, and doing it twice would double-escape.
    for text in [
        "Run `cargo clippy` and report",
        "Compare <old> against <new>",
        "Fix the **bold** claim",
        "",
    ] {
        assert!(
            picked_block(text, None).ends_with(text),
            "text was altered: {text}"
        );
    }
}

// ── Attribution (#893) ──────────────────────────────────────────────────────

#[test]
fn the_chooser_is_named_when_known() {
    // In a group, an unattributed line says nothing about who acted.
    let block = picked_block(CHOICE, Some("Daniel"));
    assert!(block.contains("Daniel"), "chooser missing: {block}");
    assert!(block.contains(CHOICE), "the choice must survive: {block}");
}

#[test]
fn an_unknown_chooser_falls_back_to_the_plain_record() {
    // Identity is not always available; the pick must still be recorded.
    for chooser in [None, Some(""), Some("   ")] {
        let block = picked_block(CHOICE, chooser);
        assert!(block.contains(CHOICE), "the choice was lost: {block}");
        assert!(
            !block.contains("—"),
            "an empty name left a dangling separator"
        );
    }
}

#[test]
fn the_fallback_echo_names_the_chooser_too() {
    let echo = echo_fallback(CHOICE, Some("Daniel"));
    assert!(echo.starts_with("> "), "fallback must stay quoted: {echo}");
    assert!(echo.contains("Daniel"));
    assert!(echo.contains(CHOICE));
}

// ── Post-tap rewrite body (#39) ─────────────────────────────────────────────

#[test]
fn classic_host_body_keeps_answer_and_pick() {
    // The exact regression from the owner report 2026-08-29 23:51Z: the
    // classic merged host edited the answer HTML alone and the pick
    // record vanished. The body must carry BOTH, answer first.
    let rewrite = pick_rewrite(
        Some(("<b>the answer</b>", false)),
        picked_block(CHOICE, None),
        0,
    );
    let PickRewrite::ClassicHost(body) = rewrite else {
        panic!("a classic host must stay classic: {rewrite:?}")
    };
    assert!(
        body.starts_with("<b>the answer</b>"),
        "answer html first: {body}"
    );
    assert!(body.contains(CHOICE), "pick record survives: {body}");
    assert!(body.contains('\u{25b6}'), "pick marker survives: {body}");
}

#[test]
fn rich_host_body_keeps_answer_and_pick() {
    let rewrite = pick_rewrite(
        Some(("<b>the answer</b>", true)),
        picked_block(CHOICE, None),
        0,
    );
    let PickRewrite::RichHost(body) = rewrite else {
        panic!("a rich host must stay rich: {rewrite:?}")
    };
    assert!(
        body.starts_with("<b>the answer</b>"),
        "answer html first: {body}"
    );
    assert!(body.contains(CHOICE), "pick record survives: {body}");
}

#[test]
fn standalone_body_is_the_pick_record_alone() {
    let record = picked_block(CHOICE, None);
    assert_eq!(
        pick_rewrite(None, record.clone(), 0),
        PickRewrite::Standalone(record)
    );
}

#[test]
fn the_rich_flag_decides_the_transport_not_the_body() {
    // Same host html, same pick — only the rich flag flips, so the two
    // bodies must match byte for byte; only the variant differs.
    let picked = picked_block(CHOICE, None);
    let classic = pick_rewrite(Some(("host", false)), picked.clone(), 0);
    let rich = pick_rewrite(Some(("host", true)), picked, 0);
    fn body_of(r: &PickRewrite) -> &str {
        match r {
            PickRewrite::RichHost(b) | PickRewrite::ClassicHost(b) | PickRewrite::Standalone(b) => {
                b.as_str()
            }
        }
    }
    assert_eq!(body_of(&classic), body_of(&rich));
    assert_ne!(classic, rich, "the variant must flip with the flag");
}
// ── #67 tap-redraw: mark_picked_button ──────────────────────────────────────

fn shared_row_html() -> String {
    // Build via the real renderer so the transform is tested against the
    // production markup shape, not a hand-typed lookalike.
    suggestion_rows_rich_html(&["Approve".to_string(), "Decline".to_string()], "tok")
}

#[test]
fn picked_button_flips_to_success_check_disabled() {
    let marked = mark_picked_button(&shared_row_html(), 0);
    let picked_btn = marked.split("<tg-button ").nth(1).unwrap();
    let picked_span = &picked_btn[..picked_btn.find("</tg-button>").unwrap()];
    assert!(
        picked_span.contains("style=\"success\""),
        "picked flips to success: {picked_span}"
    );
    assert!(
        picked_span.contains(" disabled"),
        "picked disabled: {picked_span}"
    );
    assert!(
        picked_span.ends_with("\u{2713} Approve"),
        "check prefix on the picked label: {picked_span}"
    );
}

#[test]
fn unpicked_buttons_disabled_keep_style_and_label() {
    let marked = mark_picked_button(&shared_row_html(), 0);
    let second = marked.split("<tg-button ").nth(2).unwrap();
    let span = &second[..second.find("</tg-button>").unwrap()];
    assert!(span.contains(" disabled"), "sibling disabled: {span}");
    assert!(
        span.contains("style=\"primary\""),
        "sibling keeps primary: {span}"
    );
    assert!(!span.contains('\u{2713}'), "no check on siblings: {span}");
    assert!(span.ends_with(">Decline"), "label untouched: {span}");
}

#[test]
fn non_followup_markup_passes_through_byte_for_byte() {
    let html = "<p>hi</p><tg-button-row><tg-button type=\"url\" data=\"https://x\">x</tg-button></tg-button-row>";
    assert_eq!(mark_picked_button(html, 0), html);
}

#[test]
fn html_without_buttons_is_identity() {
    let html = "<b>answer</b>\n\n<p>record</p>";
    assert_eq!(mark_picked_button(html, 1), html);
}

#[test]
fn tap_redraw_rich_host_body_has_marked_rows_and_record() {
    // End-to-end through pick_rewrite: rows rewritten to the picked state,
    // record appended, #39 order preserved (answer/rows first, record last).
    let host = format!("<b>the answer</b>\n{}", shared_row_html());
    let rewrite = pick_rewrite(Some((&host, true)), picked_block(CHOICE, None), 1);
    let PickRewrite::RichHost(body) = rewrite else {
        panic!("rich host stays rich")
    };
    assert!(body.contains("style=\"success\""), "picked marked: {body}");
    assert!(body.contains(" disabled"), "buttons disabled: {body}");
    assert!(
        !body.contains("style=\"primary\"\">Approve"),
        "the picked label must be check-prefixed"
    );
    assert!(body.contains(CHOICE), "pick record survives: {body}");
    assert!(
        body.rfind(CHOICE).unwrap() > body.rfind("</tg-button-row>").unwrap(),
        "record rides after the rows (#39)"
    );
}
