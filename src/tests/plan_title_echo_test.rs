//! An echoed plan title is dropped from the response (#837).
//!
//! The `[ACTIVE PLAN REMINDER]` shows the model `📋 Plan: "{title}"` every
//! turn, and the model opens its reply by repeating it — directly under the
//! card that already renders it, so the title appears twice in two messages.
//!
//! The first implementation compared an exact string after trimming only
//! `#`, `*` and `~`. That misses the likeliest echo of all: the reminder's
//! own formatting, carrying the `📋` and the `Plan:` label. These pin the
//! shapes the model actually produces.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::channels::telegram::delivery::strip_echoed_plan_title;

const TITLE: &str = "Business Tinder Help System Upgrade";

#[test]
fn the_reminders_own_shape_is_stripped() {
    // What the model is shown, so what it is most likely to copy. The
    // original trim list handled none of this.
    for line in [
        "📋 Plan: \"Business Tinder Help System Upgrade\"",
        "📋 Business Tinder Help System Upgrade",
        "Plan: Business Tinder Help System Upgrade",
        "\"Business Tinder Help System Upgrade\"",
    ] {
        let text = format!("{line}\n\nHere is what I found.");
        assert_eq!(
            strip_echoed_plan_title(&text, TITLE),
            "Here is what I found.",
            "must strip: {line}"
        );
    }
}

#[test]
fn markdown_headings_and_emphasis_are_stripped() {
    for line in [
        "# Business Tinder Help System Upgrade",
        "## Business Tinder Help System Upgrade",
        "**Business Tinder Help System Upgrade**",
        "__Business Tinder Help System Upgrade__",
        "Business Tinder Help System Upgrade",
    ] {
        let text = format!("{line}\n\nHere is what I found.");
        assert_eq!(
            strip_echoed_plan_title(&text, TITLE),
            "Here is what I found.",
            "must strip: {line}"
        );
    }
}

#[test]
fn a_line_that_merely_mentions_the_title_is_kept() {
    // The whole line must reduce to the title. Stripping a sentence that
    // happens to contain it would delete real content.
    let text = format!("I finished {TITLE} and moved on.\n\nDetails below.");
    assert_eq!(strip_echoed_plan_title(&text, TITLE), text);
}

#[test]
fn an_unrelated_opening_line_is_kept() {
    let text = "Here is the reconciliation you asked for.\n\nFirst item.";
    assert_eq!(strip_echoed_plan_title(text, TITLE), text);
}

#[test]
fn only_the_first_line_is_considered() {
    // A title appearing later is part of the answer, not an echo of the
    // reminder.
    let text = format!("Summary first.\n\n# {TITLE}\n\nThen detail.");
    assert_eq!(strip_echoed_plan_title(&text, TITLE), text);
}

#[test]
fn an_empty_title_strips_nothing() {
    // A plan with no title must never cause the first line to vanish.
    let text = "Here is the answer.";
    assert_eq!(strip_echoed_plan_title(text, ""), text);
    assert_eq!(strip_echoed_plan_title(text, "   "), text);
}

#[test]
fn a_response_that_is_only_the_title_becomes_empty() {
    // Degenerate but real: the model replies with the title and nothing else.
    assert_eq!(
        strip_echoed_plan_title("# Business Tinder Help System Upgrade", TITLE),
        ""
    );
}

#[test]
fn leading_blank_lines_do_not_defeat_it() {
    let text = format!("\n\n📋 {TITLE}\n\nBody.");
    assert_eq!(strip_echoed_plan_title(&text, TITLE), "Body.");
}

#[test]
fn a_different_plan_title_is_not_stripped() {
    // Two sessions, two plans: matching the wrong one would eat a real line.
    let text = "# Some Other Plan\n\nBody.";
    assert_eq!(strip_echoed_plan_title(text, TITLE), text);
}
