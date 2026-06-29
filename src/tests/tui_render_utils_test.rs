use super::*;
use ratatui::text::Line;

// ── char_boundary_at_width ──────────────────────────────────────

#[test]
fn test_char_boundary_ascii() {
    assert_eq!(char_boundary_at_width("hello", 3), 3);
    assert_eq!(char_boundary_at_width("hello", 5), 5);
    assert_eq!(char_boundary_at_width("hello", 10), 5); // past end
}

#[test]
fn test_char_boundary_multibyte() {
    // █ (U+2588) is 3 bytes, 1 display column
    let s = "ab█cd";
    // display widths: a=1, b=1, █=1, c=1, d=1 → total 5
    // byte positions: a=0, b=1, █=2..5, c=5, d=6
    assert_eq!(char_boundary_at_width(s, 2), 2); // after 'b'
    assert_eq!(char_boundary_at_width(s, 3), 5); // after '█'
    assert_eq!(char_boundary_at_width(s, 4), 6); // after 'c'
}

#[test]
fn test_char_boundary_wide_chars() {
    // CJK character '中' is 3 bytes, 2 display columns
    let s = "a中b";
    // display widths: a=1, 中=2, b=1 → total 4
    // byte positions: a=0, 中=1..4, b=4
    assert_eq!(char_boundary_at_width(s, 1), 1); // after 'a'
    assert_eq!(char_boundary_at_width(s, 2), 1); // '中' won't fit in 1 remaining col
    assert_eq!(char_boundary_at_width(s, 3), 4); // after '中'
}

#[test]
fn test_char_boundary_empty() {
    assert_eq!(char_boundary_at_width("", 5), 0);
    assert_eq!(char_boundary_at_width("hello", 0), 0);
}

// ── wrap_line_with_padding ──────────────────────────────────────

#[test]
fn test_wrap_ascii_fits() {
    let line = Line::from("short line");
    let result = wrap_line_with_padding(line, 80, "  ");
    assert_eq!(result.len(), 1);
}

#[test]
fn test_wrap_ascii_wraps() {
    let line = Line::from("this is a longer line that should wrap");
    let result = wrap_line_with_padding(line, 20, "  ");
    assert!(
        result.len() > 1,
        "expected wrapping, got {} lines",
        result.len()
    );
}

#[test]
fn test_wrap_multibyte_no_panic() {
    // This is the exact scenario that caused the original panic
    let text = format!("some text with a block char █ at the end{}", "█");
    let line = Line::from(text);
    // Should not panic
    let result = wrap_line_with_padding(line, 30, "  ");
    assert!(!result.is_empty());
}

#[test]
fn test_wrap_emoji_no_panic() {
    let line = Line::from("🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀");
    let result = wrap_line_with_padding(line, 10, "  ");
    assert!(!result.is_empty());
}

#[test]
fn test_wrap_cjk_no_panic() {
    // CJK chars are 2 display columns each
    let line = Line::from("中文测试字符串需要正确换行处理");
    let result = wrap_line_with_padding(line, 10, "  ");
    assert!(result.len() > 1);
}

#[test]
fn test_wrap_mixed_multibyte_and_spaces() {
    let line = Line::from("hello █ world █ test █ more █ text █ end");
    let result = wrap_line_with_padding(line, 15, "  ");
    assert!(result.len() > 1);
    // Every wrapped line must rejoin to a non-empty string (no span got
    // dropped or split into nothing by the multibyte wrap).
    for l in &result {
        let joined: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(!joined.is_empty());
    }
}

#[test]
fn test_wrap_zero_width() {
    let line = Line::from("test");
    let result = wrap_line_with_padding(line, 0, "  ");
    assert_eq!(result.len(), 1); // zero width returns original
}

#[test]
fn test_wrap_cursor_char() {
    // Simulates the input buffer with cursor: the exact crash scenario
    let mut input =
        "next I just noticed something weird like if I keep on this window it is always super fast"
            .to_string();
    input.push('\u{2588}'); // cursor char █
    let line = Line::from(format!("  {}", input));
    let result = wrap_line_with_padding(line, 170, "  ");
    assert!(!result.is_empty());
}
