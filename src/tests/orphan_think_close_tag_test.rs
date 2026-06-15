//! Pin the orphan-close-tag handling in `filter_think_tags` and
//! `strip_think_blocks` — added 2026-06-15 after the user reported
//! that the Mimo model (Xiaomi) occasionally skips the opening
//! `<think>` tag and outputs only the closing `</think>`. Without
//! this fix, the thinking content leaks as visible display text.
//!
//! The streaming filter (`filter_think_tags`) now detects orphan
//! close tags when `inside_think` is false and routes preceding
//! text to the reasoning buffer. The non-streaming stripper
//! (`strip_think_blocks`) does the same for batch processing.

use crate::brain::provider::custom_openai_compatible::{
    filter_think_tags, strip_think_blocks,
};

// ─── Streaming path: filter_think_tags ────────────────────────────────

#[test]
fn orphan_close_think_streaming() {
    // Model skipped the opening `<think>` tag and only emitted the closer.
    let mut inside = false;
    let mut close_idx = 0;
    let mut consumed = 0;
    let mut carry = String::new();

    let (display, reasoning) = filter_think_tags(
        "let me think about this...\n</think>Here is the answer.",
        &mut inside,
        &mut close_idx,
        &mut consumed,
        &mut carry,
    );

    assert_eq!(display, "Here is the answer.");
    assert!(
        reasoning.contains("let me think about this"),
        "orphan thinking must go to reasoning, got: {reasoning:?}"
    );
}

#[test]
fn orphan_close_think_streaming_with_trailing_display() {
    let mut inside = false;
    let mut close_idx = 0;
    let mut consumed = 0;
    let mut carry = String::new();

    let (display, reasoning) = filter_think_tags(
        "reasoning text\n</think>display text",
        &mut inside,
        &mut close_idx,
        &mut consumed,
        &mut carry,
    );

    assert_eq!(display, "display text");
    assert!(reasoning.contains("reasoning text"));
}

#[test]
fn properly_opened_think_still_works() {
    // Normal case: open tag present — must still work as before.
    let mut inside = false;
    let mut close_idx = 0;
    let mut consumed = 0;
    let mut carry = String::new();

    let (display, reasoning) = filter_think_tags(
        "Hello.<think>Some reasoning.</think>World.",
        &mut inside,
        &mut close_idx,
        &mut consumed,
        &mut carry,
    );

    assert_eq!(display, "Hello.World.");
    assert!(
        reasoning.contains("Some reasoning"),
        "properly-tagged reasoning must be captured: {reasoning:?}"
    );
}

#[test]
fn no_close_tag_means_all_display() {
    // No close tag at all — everything is display text.
    let mut inside = false;
    let mut close_idx = 0;
    let mut consumed = 0;
    let mut carry = String::new();

    let (display, reasoning) = filter_think_tags(
        "Just regular text, no tags.",
        &mut inside,
        &mut close_idx,
        &mut consumed,
        &mut carry,
    );

    assert_eq!(display, "Just regular text, no tags.");
    assert!(reasoning.is_empty());
}

#[test]
fn orphan_close_empty_before() {
    // Close tag at the very start — nothing before it to capture.
    let mut inside = false;
    let mut close_idx = 0;
    let mut consumed = 0;
    let mut carry = String::new();

    let (display, reasoning) = filter_think_tags(
        "<think>Actual response.",
        &mut inside,
        &mut close_idx,
        &mut consumed,
        &mut carry,
    );

    assert_eq!(display, "Actual response.");
    assert!(reasoning.is_empty());
}

// ─── Non-streaming path: strip_think_blocks ───────────────────────────

#[test]
fn orphan_close_think_batch() {
    let input = "some leaked reasoning\n</think>actual response";
    let result = strip_think_blocks(input);
    assert_eq!(result, "actual response");
}

#[test]
fn orphan_close_think_batch_no_display_after() {
    let input = "leaked reasoning only\n</think>";
    let result = strip_think_blocks(input);
    assert_eq!(result, "");
}

#[test]
fn properly_opened_think_batch_still_works() {
    let input = "Hello.<think>reasoning.</think>World.";
    let result = strip_think_blocks(input);
    assert_eq!(result, "Hello.World.");
}

#[test]
fn no_think_tags_batch() {
    let input = "Just normal text.";
    let result = strip_think_blocks(input);
    assert_eq!(result, "Just normal text.");
}

// ─── HTML comments (not affected by orphan detection) ─────────────────

#[test]
fn html_comment_close_not_treated_as_orphan() {
    // `-->` is index 2 in STRIP_CLOSE_TAGS and should NOT trigger
    // orphan detection (too generic for prose).
    let mut inside = false;
    let mut close_idx = 0;
    let mut consumed = 0;
    let mut carry = String::new();

    let (display, reasoning) = filter_think_tags(
        "Some text --> more text",
        &mut inside,
        &mut close_idx,
        &mut consumed,
        &mut carry,
    );

    assert_eq!(display, "Some text --> more text");
    assert!(reasoning.is_empty(), "--> alone must NOT trigger orphan routing");
}
