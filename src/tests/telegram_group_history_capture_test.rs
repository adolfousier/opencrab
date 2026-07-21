//! Regression for #685: every group message must persist to channel history,
//! even from non-allowlisted senders and other bots the bot never responds to.
//!
//! Root cause was that a group message from a bot (or a non-allowlisted user)
//! was dropped BEFORE capture, so a message the user later replied to (a peer
//! bot's post) was never stored and could not be recovered. The capture now
//! runs before the drop, building a history record for any group message that
//! carries text.

use crate::channels::telegram::handler::build_group_history_record;
use teloxide::types::{Message, Update, UpdateKind};

fn extract_message(json: &str) -> Message {
    // teloxide's Update deserializer only works from STRING input (#354).
    let u: Update = serde_json::from_str(json).expect("parse update");
    match u.kind {
        UpdateKind::Message(m) => m,
        other => panic!("expected a message update, got {other:?}"),
    }
}

#[test]
fn bot_group_message_is_captured_for_history() {
    // A group message from a non-allowlisted BOT must still be recorded, so a
    // later reply to it can be recovered by message id.
    let json = serde_json::json!({
        "update_id": 1,
        "message": {
            "message_id": 42,
            "date": 1750000000,
            "chat": {"id": -1001234567i64, "type": "supergroup", "title": "Team Room"},
            "from": {"id": 999888, "is_bot": true, "first_name": "Crabs"},
            "text": "Quick data point on phantom tool calls"
        }
    })
    .to_string();
    let msg = extract_message(&json);
    let user = msg.from.as_ref().expect("sender present");
    assert!(user.is_bot, "sender is a bot in this fixture");

    let rec = build_group_history_record(&msg, user).expect("a record must be built");
    assert_eq!(rec.channel, "telegram");
    assert_eq!(rec.content, "Quick data point on phantom tool calls");
    assert_eq!(rec.sender_id, "999888");
    assert_eq!(rec.sender_name, "Crabs");
    assert_eq!(rec.platform_message_id.as_deref(), Some("42"));
    assert_eq!(rec.channel_chat_id, "-1001234567");
}

#[test]
fn caption_only_message_is_captured() {
    // A media message with a caption still yields a record (caption is content).
    let json = serde_json::json!({
        "update_id": 2,
        "message": {
            "message_id": 43,
            "date": 1750000000,
            "chat": {"id": -100i64, "type": "supergroup", "title": "g"},
            "from": {"id": 5, "is_bot": false, "first_name": "Adi"},
            "photo": [{"file_id": "x", "file_unique_id": "y", "width": 1, "height": 1}],
            "caption": "look at this"
        }
    })
    .to_string();
    let msg = extract_message(&json);
    let user = msg.from.as_ref().expect("sender present");
    let rec = build_group_history_record(&msg, user).expect("record from caption");
    assert_eq!(rec.content, "look at this");
    assert_eq!(rec.sender_name, "Adi");
}

#[test]
fn empty_message_yields_no_record() {
    // A message with no text/caption (e.g. a rich-only bot message) has nothing
    // useful to store, so capture is skipped rather than writing an empty row.
    let json = serde_json::json!({
        "update_id": 3,
        "message": {
            "message_id": 44,
            "date": 1750000000,
            "chat": {"id": -100i64, "type": "supergroup", "title": "g"},
            "from": {"id": 5, "is_bot": true, "first_name": "Crabs"}
        }
    })
    .to_string();
    let msg = extract_message(&json);
    let user = msg.from.as_ref().expect("sender present");
    assert!(
        build_group_history_record(&msg, user).is_none(),
        "an empty message must not create a history row"
    );
}
