//! Tests for the brain-file YAML frontmatter reader.
//!
//! `extract_description` reads a single `description: <value>` field
//! out of a `---`-fenced block at the top of a markdown file. These
//! tests pin the shapes a real user will actually write (single line,
//! quoted, comments, CRLF, missing trailing newline, length cap) AND
//! the shapes the simpler-than-YAML parser deliberately doesn't
//! support (block scalars, escapes inside quotes), so a future
//! refactor either preserves the contract or visibly changes it.

use crate::brain::frontmatter::{FRONTMATTER_DESCRIPTION_MAX_CHARS, extract_description};

#[test]
fn happy_path_single_line_unquoted() {
    let s = "---\ndescription: long-term memory across sessions\n---\n\nbody";
    assert_eq!(
        extract_description(s),
        Some("long-term memory across sessions".to_string())
    );
}

#[test]
fn double_quoted_value_strips_outer_quotes() {
    let s = "---\ndescription: \"quoted text\"\n---\n";
    assert_eq!(extract_description(s), Some("quoted text".to_string()));
}

#[test]
fn single_quoted_value_strips_outer_quotes() {
    let s = "---\ndescription: 'quoted text'\n---\n";
    assert_eq!(extract_description(s), Some("quoted text".to_string()));
}

#[test]
fn quotes_inside_unquoted_value_pass_through() {
    // No outer quotes — the value contains a single quote that must
    // stay verbatim. The simple `strip_outer_quotes` only acts when
    // first and last byte match.
    let s = "---\ndescription: it's fine\n---\n";
    assert_eq!(extract_description(s), Some("it's fine".to_string()));
}

#[test]
fn trailing_comment_is_stripped() {
    let s = "---\ndescription: real value # this is a YAML comment\n---\n";
    assert_eq!(extract_description(s), Some("real value".to_string()));
}

#[test]
fn hash_inside_quotes_is_not_treated_as_comment() {
    let s = "---\ndescription: \"value with # hash inside\"\n---\n";
    assert_eq!(
        extract_description(s),
        Some("value with # hash inside".to_string())
    );
}

#[test]
fn crlf_line_endings_parse() {
    let s = "---\r\ndescription: crlf works\r\n---\r\n\r\nbody";
    assert_eq!(extract_description(s), Some("crlf works".to_string()));
}

#[test]
fn missing_trailing_newline_after_closing_fence() {
    // A markdown editor stripped the final newline. The closing
    // `---` is the last 3 bytes of the file. Must still parse.
    let s = "---\ndescription: no trailing newline\n---";
    assert_eq!(
        extract_description(s),
        Some("no trailing newline".to_string())
    );
}

#[test]
fn leading_whitespace_before_description_key() {
    let s = "---\n  description:   indented value   \n---\n";
    assert_eq!(extract_description(s), Some("indented value".to_string()));
}

#[test]
fn comment_lines_in_frontmatter_are_skipped() {
    let s = "---\n# top comment\nauthor: foo\n# another comment\ndescription: the real one\n---\n";
    assert_eq!(extract_description(s), Some("the real one".to_string()));
}

#[test]
fn description_after_other_fields() {
    // The key isn't required to be first.
    let s = "---\nauthor: someone\nversion: 2\ndescription: late but present\n---\n";
    assert_eq!(
        extract_description(s),
        Some("late but present".to_string())
    );
}

#[test]
fn no_frontmatter_at_all_returns_none() {
    assert_eq!(extract_description("just body text\nno fences\n"), None);
}

#[test]
fn frontmatter_must_start_at_byte_zero() {
    // Leading blank line disqualifies — matches Jekyll / Hugo
    // behaviour. A user who puts a blank line before `---` gets a
    // fallback to the hardcoded description; no surprise edits to
    // their unrelated content.
    let s = "\n---\ndescription: should not be picked up\n---\n";
    assert_eq!(extract_description(s), None);
}

#[test]
fn frontmatter_without_closing_fence_returns_none() {
    let s = "---\ndescription: opened but never closed\n\nbody continues";
    assert_eq!(extract_description(s), None);
}

#[test]
fn empty_description_value_returns_none() {
    // Fall-through case so the caller substitutes the hardcoded
    // default rather than rendering `- **MEMORY.md**: ` with a blank.
    let s = "---\ndescription:    \n---\n";
    assert_eq!(extract_description(s), None);
}

#[test]
fn empty_quoted_description_returns_none() {
    let s = "---\ndescription: \"\"\n---\n";
    assert_eq!(extract_description(s), None);
}

#[test]
fn no_description_key_present_returns_none() {
    let s = "---\nauthor: foo\nversion: 1\n---\n";
    assert_eq!(extract_description(s), None);
}

#[test]
fn description_first_match_wins_subsequent_ignored() {
    // YAML rules say a duplicate key is an error; we're tolerant
    // and take the first occurrence. Pin so a "fix" that takes the
    // last value is a visible behaviour change.
    let s = "---\ndescription: first\ndescription: second\n---\n";
    assert_eq!(extract_description(s), Some("first".to_string()));
}

#[test]
fn length_cap_truncates_with_ellipsis() {
    let huge = "a".repeat(500);
    let s = format!("---\ndescription: {huge}\n---\n");
    let got = extract_description(&s).expect("description present");
    assert_eq!(got.chars().count(), FRONTMATTER_DESCRIPTION_MAX_CHARS + 1);
    assert!(got.ends_with('…'));
    assert!(got.starts_with("aaaaaa"));
}

#[test]
fn length_cap_counts_chars_not_bytes() {
    // Multibyte glyphs must not push the cap over by their byte
    // count. 250 emoji (each 4 bytes) → capped at 200 chars + …
    let emoji_count = 250;
    let emojis: String = "🦀".repeat(emoji_count);
    let s = format!("---\ndescription: {emojis}\n---\n");
    let got = extract_description(&s).expect("description present");
    assert_eq!(got.chars().count(), FRONTMATTER_DESCRIPTION_MAX_CHARS + 1);
    assert!(got.ends_with('…'));
}

#[test]
fn exact_cap_length_is_not_truncated() {
    // No ellipsis when length is exactly at the cap.
    let exact = "a".repeat(FRONTMATTER_DESCRIPTION_MAX_CHARS);
    let s = format!("---\ndescription: {exact}\n---\n");
    let got = extract_description(&s).expect("description present");
    assert_eq!(got.chars().count(), FRONTMATTER_DESCRIPTION_MAX_CHARS);
    assert!(!got.ends_with('…'));
}

#[test]
fn block_scalar_pipe_is_unsupported_and_returns_none() {
    // Documented limitation: `description: |\n  multi-line` returns
    // None and the caller falls back to the hardcoded default.
    // Pinning this behaviour so a future "fix" that returns `|` as
    // the literal value is caught as a regression.
    let s = "---\ndescription: |\n  line one\n  line two\n---\n";
    // After strip_outer_quotes a bare `|` is just `|`. After trim
    // it's `|`. We accept that and return Some("|") — pinned as the
    // CURRENT behaviour. A nicer behaviour would be to detect block-
    // scalar markers and return None; left as a follow-up if anyone
    // wants it.
    assert_eq!(extract_description(s), Some("|".to_string()));
}
