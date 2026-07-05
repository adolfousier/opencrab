//! Offline probe for #354: does `serde_json::from_value::<Update>` parse a
//! NORMAL text-message update correctly? When the raw-JSON listener was
//! wired in, the entire Telegram intake went silent — updates were consumed
//! but nothing dispatched. If this test fails (Error kind), the from_value
//! path itself is the culprit and any raw listener must re-serialize to a
//! string before the typed parse.

use teloxide::types::{Update, UpdateKind};

fn minimal_text_update() -> serde_json::Value {
    serde_json::json!({
        "update_id": 1,
        "message": {
            "message_id": 10,
            "date": 1750000000,
            "chat": {"id": 123, "type": "private", "first_name": "A"},
            "from": {"id": 123, "is_bot": false, "first_name": "A"},
            "text": "hello"
        }
    })
}

// PROVEN GOTCHA (#354 outage root cause): teloxide's Update deserializer
// only works from STRING input. from_value turns EVERY update — even a
// plain text message — into UpdateKind::Error, which no dispatch branch
// matches. A raw-JSON listener wired through from_value therefore consumed
// all updates and dispatched none: the whole Telegram intake went silent.
// Any raw listener MUST re-serialize and go through from_str.
#[test]
fn from_value_yields_error_kind_never_use_it() {
    let u: Update = serde_json::from_value(minimal_text_update()).expect("deserializes leniently");
    assert!(
        matches!(u.kind, UpdateKind::Error(_)),
        "if this ever starts parsing correctly, the raw listener can be simplified"
    );
}

#[test]
fn from_str_parses_plain_text_update() {
    let s = minimal_text_update().to_string();
    let u: Update = serde_json::from_str(&s).expect("must deserialize");
    assert!(matches!(u.kind, UpdateKind::Message(_)));
}
