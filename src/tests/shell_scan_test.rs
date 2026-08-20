//! The single quote-aware pass shared by the label builder and the
//! long-command classifier.

use crate::utils::shell_scan::{blank_quoted, scan};

#[test]
fn every_character_survives_the_scan() {
    let cmd = "echo \"a; b\" && ls 'x y'";
    let rebuilt: String = scan(cmd).into_iter().map(|c| c.ch).collect();
    assert_eq!(rebuilt, cmd, "a scan must be losslessly reassemblable");
}

#[test]
fn quoted_spans_are_literal_and_their_marks_are_not() {
    let scanned = scan("a 'b' c");
    let quoted: String = scanned.iter().filter(|c| c.literal).map(|c| c.ch).collect();
    assert_eq!(quoted, "b");
    assert_eq!(scanned.iter().filter(|c| c.quote_mark).count(), 2);
}

#[test]
fn separators_inside_quotes_are_data() {
    let scanned = scan("echo \"a; b\"");
    let semicolon = scanned.iter().find(|c| c.ch == ';').expect("kept");
    assert!(semicolon.literal, "a quoted ; must not read as a separator");
}

#[test]
fn escapes_are_data_inside_and_outside_double_quotes() {
    let outside = scan("echo a\\;b");
    assert!(outside.iter().find(|c| c.ch == ';').expect("kept").literal);
    // A backslash-escaped quote does not open a span, so the tail stays syntax.
    let escaped = scan("echo \\\"x\\\" ; ls");
    let semicolon = escaped.iter().find(|c| c.ch == ';').expect("kept");
    assert!(
        !semicolon.literal,
        "escaped quotes must not swallow the rest"
    );
}

#[test]
fn a_backslash_inside_single_quotes_is_ordinary() {
    // POSIX: single quotes protect everything, backslash included.
    let scanned = scan("echo 'a\\' ; ls");
    let semicolon = scanned.iter().find(|c| c.ch == ';').expect("kept");
    assert!(!semicolon.literal, "the span closed at the second quote");
}

#[test]
fn blanking_keeps_positions_outside_quotes() {
    assert_eq!(blank_quoted("a 'bcd' e"), "a       e");
    assert_eq!(blank_quoted("plain"), "plain");
}

#[test]
fn an_unterminated_quote_swallows_the_tail() {
    // The conservative reading: everything after an unclosed quote is data.
    let blanked = blank_quoted("echo 'x; ls");
    assert_eq!(blanked.trim_end(), "echo");
    assert_eq!(blanked.chars().count(), "echo 'x; ls".chars().count());
}
