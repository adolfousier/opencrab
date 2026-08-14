//! Tests for `telegram::rich_decode::decode_rich_content` (#359).
//!
//! Rich Bot API content types must decode into readable text for the agent
//! instead of a raw JSON dump. Payload shapes mirror the Bot API objects
//! (field names verified against teloxide-core 0.13's serde definitions).
//! Anything unrecognized must return `None` so the caller's raw-JSON path
//! stays the safety net.

use crate::channels::telegram::rich_decode::decode_rich_content;
use serde_json::json;

#[test]
fn checklist_renders_title_and_task_marks() {
    let raw = json!({
        "message_id": 10,
        "checklist": {
            "title": "Release steps",
            "tasks": [
                {"id": 1, "text": "run tests", "completed_by_user": {"id": 7, "first_name": "A"}},
                {"id": 2, "text": "tag build", "completion_date": 1750000000},
                {"id": 3, "text": "announce"}
            ]
        }
    });
    let out = decode_rich_content(&raw).expect("checklist decodes");
    assert!(out.starts_with("Checklist: Release steps"));
    assert!(out.contains("[x] run tests"));
    assert!(out.contains("[x] tag build"));
    assert!(out.contains("[ ] announce"));
}

#[test]
fn poll_renders_question_options_and_votes() {
    let raw = json!({
        "poll": {
            "id": "77",
            "question": "Deploy tonight?",
            "is_anonymous": true,
            "is_closed": false,
            "total_voter_count": 5,
            "options": [
                {"text": "yes", "voter_count": 3},
                {"text": "no", "voter_count": 2}
            ]
        }
    });
    let out = decode_rich_content(&raw).expect("poll decodes");
    assert!(out.starts_with("Poll (anonymous): Deploy tonight?"));
    assert!(out.contains("- yes (3 votes)"));
    assert!(out.contains("- no (2 votes)"));
    assert!(out.contains("Total voters: 5"));
}

#[test]
fn closed_poll_is_marked_closed() {
    let raw = json!({
        "poll": {
            "question": "Done?",
            "is_closed": true,
            "options": []
        }
    });
    let out = decode_rich_content(&raw).expect("poll decodes");
    assert!(out.contains("[closed]"));
}

#[test]
fn paid_media_summarizes_items_and_star_count() {
    let raw = json!({
        "paid_media": {
            "star_count": 50,
            "paid_media": [
                {"type": "photo"},
                {"type": "video"}
            ]
        }
    });
    let out = decode_rich_content(&raw).expect("paid media decodes");
    assert!(out.contains("2 item(s)"));
    assert!(out.contains("photo, video"));
    assert!(out.contains("50-star"));
}

#[test]
fn caption_is_appended_to_decoded_content() {
    let raw = json!({
        "paid_media": {"star_count": 1, "paid_media": [{"type": "preview"}]},
        "caption": "exclusive shot"
    });
    let out = decode_rich_content(&raw).expect("paid media decodes");
    assert!(out.contains("Caption: exclusive shot"));
}

#[test]
fn giveaway_renders_prize_and_winner_count() {
    let raw = json!({
        "giveaway": {
            "chats": [],
            "winners_selection_date": 1751000000,
            "winner_count": 3,
            "prize_description": "a mug",
            "prize_star_count": 300
        }
    });
    let out = decode_rich_content(&raw).expect("giveaway decodes");
    assert!(out.starts_with("Giveaway: 3 winner(s)"));
    assert!(out.contains("Prize: a mug"));
    assert!(out.contains("Prize pool: 300 stars"));
    assert!(out.contains("2025-06-27"));
}

#[test]
fn giveaway_winners_lists_names() {
    let raw = json!({
        "giveaway_winners": {
            "winner_count": 2,
            "winners": [
                {"id": 1, "first_name": "Sam", "username": "sam_w"},
                {"id": 2, "first_name": "Kim"}
            ]
        }
    });
    let out = decode_rich_content(&raw).expect("winners decode");
    assert!(out.contains("2 winner(s)"));
    assert!(out.contains("Sam (@sam_w)"));
    assert!(out.contains("Kim"));
}

#[test]
fn giveaway_completed_mentions_unclaimed() {
    let raw = json!({
        "giveaway_completed": {"winner_count": 4, "unclaimed_prize_count": 1}
    });
    let out = decode_rich_content(&raw).expect("completed decodes");
    assert!(out.contains("4 winner(s)"));
    assert!(out.contains("1 prize(s) unclaimed"));
}

#[test]
fn gift_renders_star_value_and_text() {
    // Message-level key is `gift` holding a GiftInfo, whose inner `gift`
    // is the Gift object with the star count.
    let raw = json!({
        "gift": {
            "gift": {"id": "g1", "star_count": 100},
            "text": "congrats"
        }
    });
    let out = decode_rich_content(&raw).expect("gift decodes");
    assert!(out.contains("100 stars"));
    assert!(out.contains("congrats"));
}

#[test]
fn unique_gift_renders_name_and_number() {
    let raw = json!({
        "unique_gift": {
            "gift": {"base_name": "Astral Shard", "name": "AstralShard-777", "number": 777},
            "origin": "transfer"
        }
    });
    let out = decode_rich_content(&raw).expect("unique gift decodes");
    assert!(out.contains("Astral Shard #777"));
}

#[test]
fn story_names_the_poster() {
    let raw = json!({
        "story": {
            "id": 9,
            "chat": {"id": 5, "title": "Some Channel"}
        }
    });
    let out = decode_rich_content(&raw).expect("story decodes");
    assert!(out.contains("\"Some Channel\""));
}

#[test]
fn users_shared_lists_labels_with_id_fallback() {
    let raw = json!({
        "users_shared": {
            "request_id": 1,
            "users": [
                {"user_id": 42},
                {"user_id": 43, "first_name": "Ana", "username": "ana"}
            ]
        }
    });
    let out = decode_rich_content(&raw).expect("users shared decodes");
    assert!(out.contains("user id 42"));
    assert!(out.contains("Ana (@ana)"));
}

#[test]
fn chat_shared_uses_title_and_username() {
    let raw = json!({
        "chat_shared": {"request_id": 2, "chat_id": -100123, "title": "Ops", "username": "ops_grp"}
    });
    let out = decode_rich_content(&raw).expect("chat shared decodes");
    assert!(out.contains("Shared chat: Ops (@ops_grp)"));
}

#[test]
fn chat_shared_falls_back_to_id() {
    let raw = json!({
        "chat_shared": {"request_id": 2, "chat_id": -100123}
    });
    let out = decode_rich_content(&raw).expect("chat shared decodes");
    assert!(out.contains("chat id -100123"));
}

#[test]
fn unrecognized_content_returns_none_for_raw_fallback() {
    // A future Bot API type nobody has decoded yet: the caller must get None
    // and fall back to the raw-JSON dump, never a half-empty rendering.
    let raw = json!({
        "message_id": 1,
        "some_future_content": {"payload": true}
    });
    assert!(decode_rich_content(&raw).is_none());
}

#[test]
fn plain_text_message_returns_none() {
    let raw = json!({"message_id": 1, "text": "hello"});
    assert!(decode_rich_content(&raw).is_none());
}

#[test]
fn malformed_known_key_returns_none_not_panic() {
    // Content key present but the inner shape is garbage: decoder must bail
    // to None (raw fallback), not panic or render nonsense.
    let raw = json!({"checklist": {"tasks": "not-an-array"}});
    assert!(decode_rich_content(&raw).is_none());
    let raw = json!({"poll": 7});
    assert!(decode_rich_content(&raw).is_none());
    let raw = json!({"unique_gift": {"gift": {}}});
    assert!(decode_rich_content(&raw).is_none());
}

// ── rich_message (sendRichMessage, Bot API 10.1) — #686 ──────────────────────

#[test]
fn rich_message_html_decodes_to_plain_text() {
    // Peer OpenCrabs bots post via sendRichMessage `{html}`; teloxide leaves
    // text() empty, so this must recover readable content, tags stripped and
    // entities decoded.
    let raw = json!({
        "message_id": 10,
        "rich_message": { "html": "<b>Quick data point</b> on phantom &amp; tool calls" }
    });
    let out = decode_rich_content(&raw).expect("rich_message html decodes");
    assert_eq!(out, "Quick data point on phantom & tool calls");
}

#[test]
fn rich_message_blocks_decodes_to_text() {
    // The blocks input mode (our render_json AST): one line per top-level block,
    // inline text (incl. nested bold) concatenated within a block.
    let raw = json!({
        "message_id": 11,
        "rich_message": { "blocks": [
            { "type": "heading", "level": 2, "content": [{"type": "text", "text": "Release"}] },
            { "type": "paragraph", "content": [
                {"type": "text", "text": "Body "},
                {"type": "bold", "content": [{"type": "text", "text": "here"}]}
            ]}
        ]}
    });
    let out = decode_rich_content(&raw).expect("rich_message blocks decode");
    // #1058: headings now render with markdown level markers.
    assert_eq!(out, "## Release\nBody here");
}

#[test]
fn rich_message_with_caption_appends_caption() {
    let raw = json!({
        "message_id": 12,
        "caption": "see thread",
        "rich_message": { "html": "the data" }
    });
    let out = decode_rich_content(&raw).expect("decodes");
    assert!(out.contains("the data") && out.contains("Caption: see thread"));
}

#[test]
fn empty_rich_message_falls_through_to_none() {
    // No html and no blocks (or empty ones) -> None, so the caller's raw-JSON
    // safety net still applies rather than an empty string.
    assert!(decode_rich_content(&json!({"rich_message": {}})).is_none());
    assert!(decode_rich_content(&json!({"rich_message": {"blocks": []}})).is_none());
}
