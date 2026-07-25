//! The queue keeps context and display apart (#765).
//!
//! A synthetic entry carries the full result for the model and a compact tag
//! for the transcript. Collapsing the two when draining published a background
//! task's whole `[System: ...]` block, command and output included, as a user
//! turn in chat.

use crate::brain::agent::QueuedUserMessage;

#[test]
fn an_empty_queue_injects_nothing() {
    assert!(QueuedUserMessage::join(&[]).is_none());
}

#[test]
fn a_system_entry_keeps_its_compact_display() {
    // The exact shape that leaked: the model must still read the full result.
    let queued = vec![QueuedUserMessage::system(
        "[System: the background task you started has finished. Command: cargo test …]".to_string(),
        "🔧 background task finished: cargo test".to_string(),
    )];
    let joined = QueuedUserMessage::join(&queued).expect("one entry joins");
    assert_eq!(
        joined.display_text, "🔧 background task finished: cargo test",
        "the transcript must never receive the scaffolding"
    );
    assert!(
        joined.context_text.contains("Command: cargo test"),
        "the model still needs the full result"
    );
}

#[test]
fn a_typed_message_reads_the_same_on_both_sides() {
    let queued = vec![QueuedUserMessage::plain("do the thing".to_string())];
    let joined = QueuedUserMessage::join(&queued).expect("one entry joins");
    assert_eq!(joined.context_text, "do the thing");
    assert_eq!(joined.display_text, "do the thing");
}

#[test]
fn mixed_entries_join_each_half_independently() {
    // A typed follow-up landing next to a background completion must not drag
    // the completion's context into the visible transcript.
    let queued = vec![
        QueuedUserMessage::plain("also check the logs".to_string()),
        QueuedUserMessage::system(
            "[System: task finished. Output: …50 lines…]".to_string(),
            "🔧 background task finished: cargo clippy".to_string(),
        ),
    ];
    let joined = QueuedUserMessage::join(&queued).expect("joins");
    assert_eq!(
        joined.display_text,
        "also check the logs\n🔧 background task finished: cargo clippy"
    );
    assert_eq!(
        joined.context_text,
        "also check the logs\n[System: task finished. Output: …50 lines…]"
    );
}

#[test]
fn ordering_is_preserved_on_both_halves() {
    let queued = vec![
        QueuedUserMessage::plain("first".to_string()),
        QueuedUserMessage::plain("second".to_string()),
        QueuedUserMessage::plain("third".to_string()),
    ];
    let joined = QueuedUserMessage::join(&queued).expect("joins");
    assert_eq!(joined.display_text, "first\nsecond\nthird");
    assert_eq!(joined.context_text, "first\nsecond\nthird");
}
