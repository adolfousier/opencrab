//! Reasoning blocks are removed before delivery, not just their markers (#903).
//!
//! `strip_llm_artifacts` handled `<!-- reasoning -->...<!-- /reasoning -->` by
//! delegating to `strip_html_comments`. Those are two SEPARATE self-closed
//! comments, so the markers were deleted and the prose between them survived as
//! ordinary message text. The agent's own deliberation shipped to Telegram and
//! the TUI as if it were the answer.
//!
//! Worse than untidy: leaked narration is written in the past tense about work
//! done ("Commit landed", "The work is shipped and tracked"), so it fed
//! claim-shaped text into the phantom detectors' input.
//!
//! `hoist_reasoning_blocks` already did this correctly but was only ever called
//! on the context-rebuild path, never on delivery.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::utils::sanitize::strip_llm_artifacts;

const LEAKY: &str = "<!-- reasoning -->\nI should re-read the issue first in case \
anything changed since I opened it.\n<!-- /reasoning -->\n\nIssue closed with a remark.";

#[test]
fn the_reasoning_prose_does_not_survive() {
    let out = strip_llm_artifacts(LEAKY);
    assert!(
        !out.contains("I should re-read the issue"),
        "reasoning leaked into delivered text: {out}"
    );
}

#[test]
fn the_answer_survives() {
    // Removing the block must not take the answer with it.
    let out = strip_llm_artifacts(LEAKY);
    assert!(
        out.contains("Issue closed with a remark."),
        "answer lost: {out}"
    );
}

#[test]
fn the_markers_are_gone_too() {
    let out = strip_llm_artifacts(LEAKY);
    assert!(
        !out.contains("<!-- reasoning -->"),
        "marker survived: {out}"
    );
    assert!(
        !out.contains("<!-- /reasoning -->"),
        "marker survived: {out}"
    );
}

#[test]
fn several_blocks_are_all_removed() {
    // A turn can interleave reasoning with answer text repeatedly.
    let text = "<!-- reasoning -->first thought<!-- /reasoning -->\nAnswer one.\n\
                <!-- reasoning -->second thought<!-- /reasoning -->\nAnswer two.";
    let out = strip_llm_artifacts(text);
    assert!(!out.contains("first thought"), "{out}");
    assert!(!out.contains("second thought"), "{out}");
    assert!(out.contains("Answer one."), "{out}");
    assert!(out.contains("Answer two."), "{out}");
}

#[test]
fn ordinary_html_comments_are_still_stripped() {
    // The generic sweep must keep working; this only runs ahead of it.
    let out = strip_llm_artifacts("Answer.<!-- an ordinary comment -->");
    assert!(!out.contains("ordinary comment"), "{out}");
    assert!(out.contains("Answer."), "{out}");
}

#[test]
fn text_with_no_reasoning_is_untouched() {
    let plain = "Just a normal answer with no markers at all.";
    assert_eq!(strip_llm_artifacts(plain), plain);
}

#[test]
fn an_unclosed_marker_does_not_eat_the_answer() {
    // A truncated stream can leave an opening marker with no partner. The
    // block regex will not match, so the generic sweep removes the stray
    // comment and the text after it must remain.
    let out = strip_llm_artifacts("<!-- reasoning -->\nthinking\n\nThe answer.");
    assert!(
        out.contains("The answer."),
        "answer lost to a stray marker: {out}"
    );
}
