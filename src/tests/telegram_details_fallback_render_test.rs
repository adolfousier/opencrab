//! A `<details>` collapse that fails its native rich send must still render
//! through the rich AST on the HTML fallback.
//!
//! `should_send_native_rich` routes a collapse block to `sendRichMessage`
//! because `has_rich_structure` matches the `<details>` opener. When that
//! send fails on a transport error the ladder retries through
//! `markdown_to_telegram_html`, which picks its renderer with
//! `prefers_rich_render`. That predicate used to cover only tables and task
//! lists, so a collapse fell to the line-based ladder, which escapes unknown
//! tags — the `<details>`, `<summary>` and `<sub>` markup surfaced as visible
//! text in the delivered message instead of collapsing.

use crate::channels::telegram::markdown::markdown_to_telegram_html;
use crate::channels::telegram::rich::detect::{
    contains_details, has_rich_structure, is_details_open, prefers_rich_render,
};

/// The background-task result card shape emitted by `resume.rs`: a block-form
/// opener, a `<sub>` summary and a fenced body.
fn result_card() -> String {
    "<details>\n<summary><sub>✅ `build` 🕒 1s</sub></summary>\n\n\
     ```\ntest result: FAILED. 0 passed; 5 failed\n```\n\n</details>"
        .to_string()
}

#[test]
fn details_opener_matches_bare_and_attributed_forms() {
    assert!(is_details_open("<details>"));
    assert!(is_details_open("<details open>"));
    assert!(is_details_open("<details><summary>inline</summary>"));
    assert!(!is_details_open("</details>"));
    assert!(!is_details_open("details about the failure"));
}

#[test]
fn contains_details_finds_an_indented_opener() {
    assert!(contains_details(&result_card()));
    assert!(contains_details("intro\n  <details open>\nbody"));
    assert!(!contains_details("no collapse here\njust prose"));
}

/// The two predicates must agree on collapse blocks: whichever one sends it
/// rich, the other has to render it rich on the way back down.
#[test]
fn rich_gate_and_fallback_renderer_agree_on_details() {
    let card = result_card();
    assert!(
        has_rich_structure(&card),
        "collapse must qualify for the native rich send"
    );
    assert!(
        prefers_rich_render(&card),
        "the HTML fallback must render the same collapse through the rich AST"
    );
}

/// A collapse with no table and no task list is the exact payload that used
/// to leak. The rendered HTML must carry no literal collapse markup.
#[test]
fn details_fallback_emits_no_literal_collapse_markup() {
    let html = markdown_to_telegram_html(&result_card());

    for leaked in [
        "&lt;details&gt;",
        "&lt;summary&gt;",
        "&lt;sub&gt;",
        "&lt;/details&gt;",
    ] {
        assert!(
            !html.contains(leaked),
            "escaped collapse markup leaked into the delivered message: {leaked} in {html}"
        );
    }
    // The rich renderer's downgrade keeps the summary visible as a heading.
    assert!(
        html.contains('▸'),
        "collapse summary should survive as a flat header: {html}"
    );
    assert!(
        html.contains("build"),
        "summary text should survive the downgrade: {html}"
    );
}

/// Prose without any block structure keeps the line-based ladder.
#[test]
fn plain_prose_still_uses_the_line_renderer() {
    assert!(!prefers_rich_render("just a sentence with **bold** in it"));
}

/// An unmatched `<sub>` is not markup we emitted, so it must stay visible
/// text rather than being silently swallowed by the downgrade.
#[test]
fn lone_sub_opener_in_prose_is_still_escaped() {
    let html = markdown_to_telegram_html(
        "<details>\n<summary>x</summary>\n\nuse <sub> here\n\n</details>",
    );
    assert!(
        html.contains("&lt;sub&gt;"),
        "an unmatched tag is prose, not markup: {html}"
    );
}
