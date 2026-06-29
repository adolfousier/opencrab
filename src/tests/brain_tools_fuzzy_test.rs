use crate::brain::tools::fuzzy::*;

#[test]
fn seek_exact() {
    let lines = vec!["foo", "bar", "baz"];
    assert_eq!(seek_sequence(&lines, &["bar", "baz"], 0), vec![1]);
}

#[test]
fn seek_rstrip() {
    let lines = vec!["foo ", "bar\t\t"];
    assert_eq!(seek_sequence(&lines, &["foo", "bar"], 0), vec![0]);
}

#[test]
fn seek_trim_both() {
    let lines = vec![" foo ", " bar\t"];
    assert_eq!(seek_sequence(&lines, &["foo", "bar"], 0), vec![0]);
}

#[test]
fn seek_pattern_longer() {
    let lines = vec!["one line"];
    let result = seek_sequence(&lines, &["too", "many", "lines"], 0);
    assert!(result.is_empty());
}

#[test]
fn replace_exact_substring() {
    let content = "alpha\nbeta\ngamma\n";
    let new = fuzzy_replace_once(content, "beta", "BETA").unwrap();
    assert!(new.contains("BETA"));
    assert!(!new.contains("beta"));
}

#[test]
fn replace_ambiguous_exact() {
    let content = "x\nx\n";
    let err = fuzzy_replace_once(content, "x", "y").unwrap_err();
    assert!(err.contains("2 times"));
}

#[test]
fn replace_fuzzy_indent() {
    let content = "def main():\n    message = \"Hi\"\n";
    let old = "    message = \"Hi\"";
    let new = "    message = \"Hello\"";
    let result = fuzzy_replace_once(content, old, new).unwrap();
    assert!(result.contains("Hello"));
}

#[test]
fn replace_smart_quotes() {
    let content = "println!(\"hello\");\n";
    let old = "println!(“hello”);";
    let new = "println!(\"hi\");";
    let result = fuzzy_replace_once(content, old, new).unwrap();
    assert!(result.contains("\"hi\""));
}

#[test]
fn replace_preserves_trailing_newline() {
    let content = "a\nb\nc\n";
    let result = fuzzy_replace_once(content, "b", "B").unwrap();
    assert!(result.ends_with('\n'));
}

#[test]
fn replace_empty_old_errors() {
    let err = fuzzy_replace_once("x", "", "y").unwrap_err();
    assert!(err.contains("must not be empty"));
}

#[test]
fn replace_not_found_errors() {
    let err = fuzzy_replace_once("hello world", "xyz", "abc").unwrap_err();
    assert!(err.contains("not found"));
}
