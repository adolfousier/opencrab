//! A replayed turn sees the follow-up the user queued before the restart
//! (#1401).
//!
//! The interrupted Slack turn had a mid-turn user message injected seconds
//! before the restart, persisted as a user row and followed by the empty
//! assistant placeholder the killed turn left behind. Replay rebuilds the
//! context from the database, so the follow-up must survive that rebuild
//! and the empty placeholder must not reach the provider.

use uuid::Uuid;

use crate::brain::agent::context::AgentContext;
use crate::brain::provider::{ContentBlock, Role};
use crate::db::models::Message as DbMessage;

fn row(session: Uuid, role: &str, content: &str, seq: i32) -> DbMessage {
    DbMessage::new(session, role.to_string(), content.to_string(), seq)
}

#[test]
fn queued_follow_up_survives_the_rebuild_and_the_empty_placeholder_does_not() {
    let session = Uuid::new_v4();
    let rows = vec![
        row(session, "user", "card 394 is still not fixed", 1),
        row(session, "assistant", "Checking the v2 docs now.", 2),
        row(
            session,
            "user",
            "dev server has the latest code by the way",
            3,
        ),
        // The killed turn's assistant row: created at turn start, never filled.
        row(session, "assistant", "", 4),
    ];
    let context = AgentContext::from_db_messages(session, rows, 100_000);
    let messages = &context.messages;
    assert_eq!(messages.len(), 3, "the empty placeholder must be dropped");
    let last = messages.last().expect("three messages");
    assert_eq!(last.role, Role::User);
    assert!(
        matches!(&last.content[..], [ContentBlock::Text { text }] if text == "dev server has the latest code by the way"),
        "the queued follow-up must be the last thing the replayed model reads: {:?}",
        last.content
    );
    assert!(
        messages.iter().all(|m| m
            .content
            .iter()
            .all(|b| !matches!(b, ContentBlock::Text { text } if text.is_empty()))),
        "no empty text block may reach the provider"
    );
}
