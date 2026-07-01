//! Tests for `utils::image::extract_react_marker` — the `<<react:emoji>>`
//! reaction directive extractor.
//!
//! The extractor is strict on purpose: only payloads that look like a real
//! emoji are treated as directives, and occurrences inside backtick code
//! spans are never extracted. A word payload (`<<react:emoji>>` written in
//! prose while discussing the feature) once fired a bogus REACTION_INVALID
//! Telegram call and mutated the final text, breaking exact-match dedup
//! against the already-sent intermediate — both copies landed in the chat.

use crate::utils::extract_react_marker;

// ── valid directives ─────────────────────────────────────────────────────

#[test]
fn react_bare_directive() {
    let (text, emoji) = extract_react_marker("<<react:👍>>");
    assert_eq!(text, "");
    assert_eq!(emoji.as_deref(), Some("👍"));
}

#[test]
fn react_directive_with_text() {
    let (text, emoji) = extract_react_marker("Sure thing! <<react:✅>>");
    assert_eq!(text, "Sure thing!");
    assert_eq!(emoji.as_deref(), Some("✅"));
}

#[test]
fn react_multiple_directives_uses_first() {
    let (text, emoji) = extract_react_marker("<<react:👍>> and <<react:❤️>>");
    assert_eq!(text, "and");
    assert_eq!(emoji.as_deref(), Some("👍"));
}

#[test]
fn react_whitespace_trimmed() {
    let (text, emoji) = extract_react_marker("  <<react:🔥>>  ");
    assert_eq!(text, "");
    assert_eq!(emoji.as_deref(), Some("🔥"));
}

#[test]
fn react_with_surrounding_newlines() {
    let (text, emoji) = extract_react_marker("\n\n<<react:🔥>>\n\n");
    assert_eq!(text, "");
    assert_eq!(emoji.as_deref(), Some("🔥"));
}

#[test]
fn react_embedded_in_middle() {
    let (text, emoji) = extract_react_marker("Hello <<react:👋>> world");
    assert_eq!(text, "Hello  world");
    assert_eq!(emoji.as_deref(), Some("👋"));
}

#[test]
fn react_only_react_with_no_extra_text() {
    // The common reaction-only case: LLM outputs just the directive
    let (text, emoji) = extract_react_marker("<<react:✅>>");
    assert!(text.trim().is_empty());
    assert_eq!(emoji.as_deref(), Some("✅"));
}

#[test]
fn react_compound_emoji_accepted() {
    // Skin-tone modifier (👍🏽) and VS-16 sequences (❤️) are multi-char but
    // still valid reaction payloads.
    let (text, emoji) = extract_react_marker("<<react:👍🏽>>");
    assert_eq!(text, "");
    assert_eq!(emoji.as_deref(), Some("👍🏽"));
}

// ── no directive ─────────────────────────────────────────────────────────

#[test]
fn react_no_directive() {
    let (text, emoji) = extract_react_marker("Just a normal message.");
    assert_eq!(text, "Just a normal message.");
    assert!(emoji.is_none());
}

#[test]
fn react_malformed_no_closing() {
    // Missing >> — left untouched
    let (text, emoji) = extract_react_marker("<<react:👍");
    assert_eq!(text, "<<react:👍");
    assert!(emoji.is_none());
}

// ── prose mentions must NOT extract ──────────────────────────────────────

#[test]
fn react_word_payload_stays_in_prose() {
    // "emoji" is a placeholder word, not an emoji — the marker is prose
    // (e.g. the agent discussing this very feature) and must survive intact
    // so the text matches the already-sent intermediate for dedup.
    let (text, emoji) = extract_react_marker("the leak where <<react:emoji>> showed as raw text");
    assert_eq!(text, "the leak where <<react:emoji>> showed as raw text");
    assert!(emoji.is_none());
}

#[test]
fn react_hello_payload_not_extracted() {
    let (text, emoji) = extract_react_marker("<<react:hello>>");
    assert_eq!(text, "<<react:hello>>");
    assert!(emoji.is_none());
}

#[test]
fn react_empty_payload_not_extracted() {
    let (text, emoji) = extract_react_marker("<<react:>>");
    assert_eq!(text, "<<react:>>");
    assert!(emoji.is_none());
}

#[test]
fn react_marker_in_code_span_untouched() {
    // Even a REAL emoji payload is not a directive inside backticks — it's
    // quoted documentation.
    let (text, emoji) = extract_react_marker("use `<<react:👍>>` to react");
    assert_eq!(text, "use `<<react:👍>>` to react");
    assert!(emoji.is_none());
}

#[test]
fn react_directive_after_closed_code_span_still_extracts() {
    let (text, emoji) = extract_react_marker("see `code` here <<react:👍>>");
    assert_eq!(text, "see `code` here");
    assert_eq!(emoji.as_deref(), Some("👍"));
}

#[test]
fn react_long_payload_rejected() {
    // Over the 8-char cap — not a plausible single reaction emoji.
    let (text, emoji) = extract_react_marker("<<react:🔥🔥🔥🔥🔥🔥🔥🔥🔥>>");
    assert_eq!(text, "<<react:🔥🔥🔥🔥🔥🔥🔥🔥🔥>>");
    assert!(emoji.is_none());
}
