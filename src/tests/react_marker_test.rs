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

// ── mangled prefix (models that escape the angle brackets) ───────────────

#[test]
fn react_escaped_single_backslash_prefix() {
    // Some models emit `<\react:` instead of `<<react:`, escaping the second
    // angle bracket. The reaction must still fire.
    let (text, emoji) = extract_react_marker(r"<\react:👍>>");
    assert_eq!(text, "");
    assert_eq!(emoji.as_deref(), Some("👍"));
}

#[test]
fn react_escaped_double_backslash_prefix() {
    let (text, emoji) = extract_react_marker(r"<\\react:👍>>");
    assert_eq!(text, "");
    assert_eq!(emoji.as_deref(), Some("👍"));
}

#[test]
fn react_single_angle_prefix() {
    // A single opening angle also normalizes to the same extraction.
    let (text, emoji) = extract_react_marker("<react:👍>>");
    assert_eq!(text, "");
    assert_eq!(emoji.as_deref(), Some("👍"));
}

#[test]
fn react_escaped_prefix_with_text() {
    let (text, emoji) = extract_react_marker(r"All set <\react:✅>>");
    assert_eq!(text, "All set");
    assert_eq!(emoji.as_deref(), Some("✅"));
}

#[test]
fn react_mangled_prefix_word_payload_stays() {
    // The emoji-validation guard still applies to a mangled prefix: a word
    // payload is not a directive and must survive intact.
    let (text, emoji) = extract_react_marker(r"<\react:emoji>>");
    assert_eq!(text, r"<\react:emoji>>");
    assert!(emoji.is_none());
}

#[test]
fn react_mangled_prefix_in_code_span_untouched() {
    // Code-span protection still applies to a mangled prefix.
    let (text, emoji) = extract_react_marker(r"use `<\react:👍>>` to react");
    assert_eq!(text, r"use `<\react:👍>>` to react");
    assert!(emoji.is_none());
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
    // No terminator at all (`>>`, `</react>`, or `>`) — left untouched.
    let (text, emoji) = extract_react_marker("<<react:👍");
    assert_eq!(text, "<<react:👍");
    assert!(emoji.is_none());
}

// ── tolerant terminator (models that hallucinate the close) ──────────────

#[test]
fn react_xml_close_tag_terminator() {
    // Cursor/Cline muscle memory: the model closes the directive with an
    // XML-style `</react>` instead of `>>`. It must still fire, and the
    // mangled marker must never leak into the chat as raw text.
    let (text, emoji) = extract_react_marker("<<react:👍</react>");
    assert_eq!(text, "");
    assert_eq!(emoji.as_deref(), Some("👍"));
}

#[test]
fn react_xml_close_tag_with_text() {
    let (text, emoji) = extract_react_marker("All set <<react:✅</react>");
    assert_eq!(text, "All set");
    assert_eq!(emoji.as_deref(), Some("✅"));
}

#[test]
fn react_bare_single_bracket_terminator() {
    // A single closing `>` (dropped the second bracket) still closes the
    // directive rather than leaking the marker.
    let (text, emoji) = extract_react_marker("<<react:🔥>");
    assert_eq!(text, "");
    assert_eq!(emoji.as_deref(), Some("🔥"));
}

#[test]
fn react_double_bracket_preferred_over_single() {
    // `>>` and a bare `>` both start at the same offset; the longer `>>`
    // must win so no stray bracket is left in the output.
    let (text, emoji) = extract_react_marker("<<react:👍>> done");
    assert_eq!(text, "done");
    assert_eq!(emoji.as_deref(), Some("👍"));
}

#[test]
fn react_xml_close_word_payload_stays() {
    // The emoji guard still applies to the tolerant terminator: a word
    // payload closed with `</react>` is prose, not a directive.
    let (text, emoji) = extract_react_marker("<<react:emoji</react>");
    assert_eq!(text, "<<react:emoji</react>");
    assert!(emoji.is_none());
}

#[test]
fn react_bare_terminator_in_code_span_untouched() {
    // Code-span protection still applies to the tolerant terminator.
    let (text, emoji) = extract_react_marker("use `<<react:👍>` to react");
    assert_eq!(text, "use `<<react:👍>` to react");
    assert!(emoji.is_none());
}

// ── keyword-less opener (models that drop the `react:` tag) ──────────────

#[test]
fn react_keywordless_double_bracket_fires() {
    // Some models drop the `react:` tag and just double-bracket the emoji.
    let (text, emoji) = extract_react_marker("<<✅>>");
    assert_eq!(text, "");
    assert_eq!(emoji.as_deref(), Some("✅"));
}

#[test]
fn react_keywordless_with_text() {
    let (text, emoji) = extract_react_marker("Done <<🔥>>");
    assert_eq!(text, "Done");
    assert_eq!(emoji.as_deref(), Some("🔥"));
}

#[test]
fn react_keywordless_bare_terminator_fires() {
    // Keyword-less opener + a single dropped bracket on the close.
    let (text, emoji) = extract_react_marker("<<👍>");
    assert_eq!(text, "");
    assert_eq!(emoji.as_deref(), Some("👍"));
}

#[test]
fn react_keywordless_single_bracket_stays_text() {
    // A single-bracket `<✅>` is one char from HTML/emoticon noise — it must
    // NOT be treated as a directive, only the double-bracket form is.
    let (text, emoji) = extract_react_marker("<✅>");
    assert_eq!(text, "<✅>");
    assert!(emoji.is_none());
}

#[test]
fn react_keywordless_word_payload_stays_text() {
    // The emoji guard still applies without the `react:` tag: an ASCII word
    // between double brackets is prose, not a reaction.
    let (text, emoji) = extract_react_marker("<<word>>");
    assert_eq!(text, "<<word>>");
    assert!(emoji.is_none());
}

#[test]
fn react_keywordless_in_code_span_untouched() {
    let (text, emoji) = extract_react_marker("use `<<✅>>` to react");
    assert_eq!(text, "use `<<✅>>` to react");
    assert!(emoji.is_none());
}

#[test]
fn react_keywordless_empty_payload_stays_text() {
    let (text, emoji) = extract_react_marker("<<>>");
    assert_eq!(text, "<<>>");
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
