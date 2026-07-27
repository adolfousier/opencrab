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

use crate::channels::telegram::suggest_followups::{echo_fallback, picked_block};

const CHOICE: &str = "Update the SKILL.md with the new callback routing";

#[test]
fn the_edited_block_is_not_quoted() {
    // A leading `>` is what #787 added. It renders as a quote inside a bot
    // bubble, which is the appearance being fixed.
    let block = picked_block(CHOICE);
    assert!(
        !block.starts_with('>'),
        "the in-place record must not be quoted: {block}"
    );
}

#[test]
fn the_edited_block_marks_the_choice_and_keeps_the_text() {
    let block = picked_block(CHOICE);
    assert!(block.starts_with('\u{25b6}'), "must lead with the marker");
    assert!(block.contains(CHOICE), "the chosen text must survive");
}

#[test]
fn the_fallback_stays_quoted() {
    // Only reached when the block cannot be edited. Quoting is the weaker
    // presentation, which is why it is the fallback and not the default.
    let echo = echo_fallback(CHOICE);
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
    assert_ne!(picked_block(CHOICE), echo_fallback(CHOICE));
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
            picked_block(text).ends_with(text),
            "text was altered: {text}"
        );
    }
}
