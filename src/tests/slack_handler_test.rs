//! Tests for Slack handler `split_message`.

use crate::channels::slack::handler::split_message;

#[test]
fn split_short_message() {
    let chunks = split_message("hello", 3000);
    assert_eq!(chunks, vec!["hello"]);
}

#[test]
fn split_long_message() {
    let text = "a\n".repeat(2000);
    let chunks = split_message(&text, 3000);
    assert!(chunks.len() >= 2);
    for chunk in &chunks {
        assert!(chunk.len() <= 3000);
    }
    let joined: String = chunks.into_iter().collect();
    assert_eq!(joined, text);
}
