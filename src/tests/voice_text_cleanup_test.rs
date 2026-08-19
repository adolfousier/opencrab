//! Tests for voice/text_cleanup.rs.
//!
//! Cleaning runs for every TTS provider, so these must build without the
//! `local-tts` feature; that gate is what hid them from the coverage build.

use crate::channels::voice::text_cleanup::clean_for_tts;

#[test]
fn clean_for_tts_strips_markdown() {
    let input = "**Hello** *world*! Check `this_code` out.";
    let cleaned = clean_for_tts(input);
    assert_eq!(cleaned, "Hello world! Check this_code out.");
}

#[test]
fn clean_for_tts_keeps_code_block_content() {
    let input = "Here is code:\n\n```rust\nfn main() {}\n```\n\nDone.";
    let cleaned = clean_for_tts(input);
    assert!(cleaned.contains("fn main()"));
    assert!(cleaned.contains("Done."));
    assert!(!cleaned.contains("```"));
}

#[test]
fn clean_for_tts_collapses_whitespace() {
    let input = "Hello    world   how   are  you";
    let cleaned = clean_for_tts(input);
    assert_eq!(cleaned, "Hello world how are you");
}

#[test]
fn clean_for_tts_collapses_punctuation() {
    let input = "Wow!!! Really??? Yes...";
    let cleaned = clean_for_tts(input);
    assert_eq!(cleaned, "Wow! Really? Yes.");
}

#[test]
fn clean_for_tts_strips_headers() {
    let input = "## My Header\nSome text";
    let cleaned = clean_for_tts(input);
    assert_eq!(cleaned, "My Header. Some text");
}

#[test]
fn clean_for_tts_strips_bullets() {
    let input = "- First item\n- Second item";
    let cleaned = clean_for_tts(input);
    assert_eq!(cleaned, "First item. Second item");
}
