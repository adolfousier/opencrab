//! Tests for Discord handler `split_message`.

use crate::channels::discord::handler::split_message;

#[test]
fn split_short_message() {
    let chunks = split_message("hello", 2000);
    assert_eq!(chunks, vec!["hello"]);
}

#[test]
fn split_long_message() {
    let text = "a\n".repeat(1500);
    let chunks = split_message(&text, 2000);
    assert!(chunks.len() >= 2);
    for chunk in &chunks {
        assert!(chunk.len() <= 2000);
    }
    let joined: String = chunks.into_iter().collect();
    assert_eq!(joined, text);
}
