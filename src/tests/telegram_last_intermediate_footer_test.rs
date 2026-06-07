//! Tests for `telegram::handler::build_last_intermediate_with_footer`.
//!
//! Regression (2026-06-06): on Telegram the ctx/tok-s footer NEVER showed
//! on any turn. Cause: tool-using turns deliver all their text as
//! intermediate messages, so the final-response text dedups to empty
//! (`html.len=0`) and the footer — computed correctly — was dropped per
//! commit 7a0ca1c9 ("if there's no final text to attach it to, skip it").
//! Since nearly every turn uses a tool, the footer was always dropped.
//!
//! The fix appends the footer to the LAST intermediate message by editing
//! it in place — inline, never a standalone footer bubble. This helper
//! builds the edited body: it reconstructs the last chunk exactly as it
//! was originally sent, appends the footer, and refuses (returns None)
//! when the result wouldn't fit Telegram's 4096-char cap.

use crate::channels::telegram::handler::build_last_intermediate_with_footer;

const FOOTER: &str = "ctx: 57K/200K 29% | 530 tok/s";

#[test]
fn appends_footer_to_short_intermediate() {
    let body = build_last_intermediate_with_footer("Here is the status: all green.", FOOTER)
        .expect("short intermediate + footer must fit");
    assert!(
        body.contains("all green"),
        "the original intermediate content must be preserved: {body}"
    );
    assert!(
        body.ends_with(FOOTER),
        "the footer must be appended at the very end: {body}"
    );
    assert!(
        body.contains("\n\n"),
        "footer must be separated from content by a blank line: {body}"
    );
}

#[test]
fn empty_footer_returns_none() {
    // No footer to append (e.g. tok/s suppressed AND ctx somehow empty) —
    // nothing to do.
    assert_eq!(
        build_last_intermediate_with_footer("some content", ""),
        None
    );
}

#[test]
fn empty_intermediate_returns_none() {
    // No intermediate message to attach to — caller falls back to just
    // removing the placeholder.
    assert_eq!(build_last_intermediate_with_footer("", FOOTER), None);
}

#[test]
fn ctx_only_footer_no_tok_s_still_appends() {
    // When tok/s is suppressed (burst-delivery guard), the footer is
    // ctx-only. It must still be appended so the user sees the budget.
    let footer = "ctx: 57K/200K 29%";
    let body = build_last_intermediate_with_footer("done", footer).expect("ctx-only must fit");
    assert!(body.ends_with(footer), "{body}");
}

#[test]
fn result_within_telegram_cap_is_returned() {
    // A ~3500-char intermediate plus a short footer stays under 4096.
    let big = "x".repeat(3500);
    let body = build_last_intermediate_with_footer(&big, FOOTER)
        .expect("3500 + footer must fit under 4096");
    assert!(body.chars().count() <= 4096);
    assert!(body.ends_with(FOOTER));
}

#[test]
fn oversized_last_chunk_refuses_rather_than_truncate() {
    // If the last chunk is already near the 4096 cap, appending the footer
    // would overflow. We must return None (skip the footer) rather than
    // truncate real content to make room for metadata.
    //
    // split_message caps chunks at 4096, so the last chunk can be up to
    // 4096 chars; footer + separator pushes it over. A 4090-char body
    // leaves no room for a 28-char footer + 2-char separator.
    let near_cap = "y".repeat(4090);
    assert_eq!(
        build_last_intermediate_with_footer(&near_cap, FOOTER),
        None,
        "must refuse to append when it would exceed the 4096 cap"
    );
}

#[test]
fn multichunk_intermediate_appends_to_last_chunk_only() {
    // An intermediate longer than 4096 is split into multiple chunks when
    // sent. The footer must attach to the LAST chunk (the last message the
    // user sees), so the returned body must NOT contain the start of the
    // content — only the tail chunk + footer.
    let head = "A".repeat(4096);
    let tail = "ZZZZ tail content";
    let combined = format!("{head}{tail}");
    let body = build_last_intermediate_with_footer(&combined, FOOTER)
        .expect("last chunk + footer must fit");
    assert!(body.ends_with(FOOTER), "{body}");
    assert!(
        body.contains("tail content"),
        "must carry the last chunk's tail content: {body}"
    );
    assert!(
        body.chars().count() <= 4096,
        "the edited last-chunk message must stay within the cap"
    );
}
