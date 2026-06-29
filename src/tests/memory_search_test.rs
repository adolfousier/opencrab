use crate::memory::search::*;

#[test]
fn test_sanitize_fts_query() {
    assert_eq!(sanitize_fts_query("hello world"), "\"hello\" \"world\"");
    assert_eq!(sanitize_fts_query(""), "");
    assert_eq!(sanitize_fts_query("auth\"bug"), "\"authbug\"");
}

#[test]
fn test_extract_snippet() {
    let body = "# Today\nFixed the authentication bug in login flow. Also refactored database.";
    let snippet = extract_snippet(body, "\"authentication\"", 60);
    assert!(snippet.contains("authentication"));
}

#[test]
fn test_extract_snippet_no_match() {
    let body = "Some content without the search term";
    let snippet = extract_snippet(body, "\"nonexistent\"", 60);
    assert!(snippet.contains("Some content"));
}
