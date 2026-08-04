//! Regression (#941): the plan card's checklist must render one row per line
//! on BOTH targets, and every section of a card must break lines the same way.
//!
//! The two targets speak different HTML dialects. Classic `sendMessage` uses
//! Telegram's limited `ParseMode::Html`, where a bare `\n` IS the line break.
//! `sendRichMessage` renders real HTML, where a `\n` is ordinary whitespace and
//! collapses — so a bare newline there silently joins lines together.
//!
//! The bug: prose was converted to `<p>` for the rich target while the title,
//! checklist rows and goal separator were left on bare `\n`. Half the card had
//! been converted, so the checklist arrived as one run-on paragraph. These
//! tests pin the property that made that possible — sections choosing their own
//! convention — rather than only the one symptom.

use crate::channels::telegram::flow_chrome::{GoalSection, ProseSection};
use crate::channels::telegram::plan_card::{render_plan_card_html, render_plan_card_rich_html};

const TITLE: &str = "Merge PR #123 (schema heal) + fix on top + tests";

fn rows() -> Vec<String> {
    vec![
        "☑ First task".to_string(),
        "☑ Second task".to_string(),
        "☐ Third task".to_string(),
    ]
}

/// A block-level break in the rich dialect: anything that ends one line and
/// starts the next when the HTML is actually rendered. A bare `\n` is not one.
fn rich_has_block_break_between(html: &str, first: &str, second: &str) -> bool {
    let Some(a) = html.find(first) else {
        return false;
    };
    let Some(b) = html.find(second) else {
        return false;
    };
    if b <= a {
        return false;
    }
    let between = &html[a + first.len()..b];
    between.contains("</p>") || between.contains("<br") || between.contains("</details>")
}

#[test]
fn rich_checklist_rows_are_separated_by_block_level_markup() {
    let html = render_plan_card_rich_html(Some(TITLE), Some(&rows()), None, None)
        .expect("a card with a title and checklist must render");

    // The exact failure from the report: consecutive rows joined by nothing a
    // rendering engine treats as a break.
    assert!(
        rich_has_block_break_between(&html, "First task", "Second task"),
        "checklist rows must be separated by block-level markup in the rich \
         dialect, otherwise they collapse into one line. Got:\n{html}"
    );
    assert!(
        rich_has_block_break_between(&html, "Second task", "Third task"),
        "every consecutive pair must break, not just the first. Got:\n{html}"
    );
    assert!(
        rich_has_block_break_between(&html, "</b>", "First task"),
        "the title must break before the first checklist row. Got:\n{html}"
    );
}

#[test]
fn rich_card_never_relies_on_a_bare_newline_to_break_a_line() {
    // The property, not the symptom: in the rich dialect a `\n` between two
    // pieces of visible text does nothing. `\n` inside a tag's body (as in the
    // goal's `<details>`) is cosmetic source formatting, so this checks the
    // seams between sections, which is where a break has to be real.
    let prose = vec![ProseSection {
        heading: Some("Context".to_string()),
        body: "Some prose paragraph.".to_string(),
    }];
    let goal = GoalSection {
        text: "Ship it".to_string(),
        completed: false,
    };
    let html = render_plan_card_rich_html(Some(TITLE), Some(&rows()), Some(&prose), Some(&goal))
        .expect("full card must render");

    assert!(
        rich_has_block_break_between(&html, "</b>", "Some prose paragraph"),
        "title → prose seam must be a real break. Got:\n{html}"
    );
    assert!(
        rich_has_block_break_between(&html, "Some prose paragraph.", "First task"),
        "prose → checklist seam must be a real break. Got:\n{html}"
    );
    assert!(
        rich_has_block_break_between(&html, "Third task", "goal"),
        "checklist → goal seam must be a real break. Got:\n{html}"
    );
}

#[test]
fn rich_card_does_not_wrap_block_level_elements_in_a_paragraph() {
    // `<p>` cannot contain `<details>`; wrapping one in the other produces
    // markup the renderer will re-balance, moving content out of the card.
    let prose = vec![ProseSection {
        heading: Some("Context".to_string()),
        body: "Some prose.".to_string(),
    }];
    let html = render_plan_card_rich_html(Some(TITLE), None, Some(&prose), None)
        .expect("card with prose must render");

    assert!(
        !html.contains("<p><details"),
        "a collapsible must not be wrapped in a paragraph. Got:\n{html}"
    );
    assert!(
        !html.contains("<p><blockquote"),
        "a blockquote must not be wrapped in a paragraph. Got:\n{html}"
    );
}

#[test]
fn classic_card_still_breaks_lines_with_newlines() {
    // The classic dialect has no `<p>`; a newline is the break. Unifying the
    // serializer must not leak the rich convention into this target.
    let html = render_plan_card_html(Some(TITLE), Some(&rows()), None, None)
        .expect("a card with a title and checklist must render");

    assert!(
        html.contains("☑ First task\n☑ Second task\n☐ Third task"),
        "classic rows must stay newline-separated. Got:\n{html}"
    );
    assert!(
        !html.contains("<p>"),
        "classic ParseMode::Html has no <p> tag — emitting one shows it as \
         literal text or gets the message rejected. Got:\n{html}"
    );
}

#[test]
fn classic_card_keeps_the_blank_line_before_the_goal() {
    // Spacing the goal away from the body is a deliberate part of the classic
    // layout; the block model must preserve it exactly.
    let goal = GoalSection {
        text: "Ship it".to_string(),
        completed: false,
    };
    let html = render_plan_card_html(Some(TITLE), Some(&rows()), None, Some(&goal))
        .expect("card with a goal must render");

    assert!(
        html.contains("☐ Third task\n\n<blockquote"),
        "a blank line must separate the checklist from the goal. Got:\n{html}"
    );
}

#[test]
fn classic_card_has_no_gap_before_the_goal_when_there_is_no_body() {
    // With neither checklist nor prose the gap is not added, so a title-only
    // card does not carry a stray blank line.
    let goal = GoalSection {
        text: "Ship it".to_string(),
        completed: false,
    };
    let html = render_plan_card_html(Some(TITLE), None, None, Some(&goal))
        .expect("title + goal must render");

    assert!(
        !html.contains("\n\n"),
        "no body means no gap before the goal. Got:\n{html}"
    );
}

#[test]
fn an_empty_card_still_renders_nothing() {
    // The caller removes the card on `None`; the block model must not turn an
    // empty card into an empty-but-present one.
    assert!(render_plan_card_rich_html(None, None, None, None).is_none());
    assert!(render_plan_card_html(None, None, None, None).is_none());
    assert!(
        render_plan_card_rich_html(Some("   "), None, None, None).is_none(),
        "a whitespace-only title is not content"
    );
}
