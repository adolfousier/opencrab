//! Lean JSON rendering for gramers read results.
//!
//! The watch loop's `convert.rs` is teloxide-shaped; the tool plane
//! speaks the maintainer's MCP envelope instead (`messages`/`chats`
//! arrays of flat objects). Ids render as strings via `Display`
//! (Bot API dialog id form) so JSON consumers never lose precision
//! on 64-bit channel ids.

use grammers_client::message::Message;
use grammers_client::peer::{Dialog, Peer};
use serde_json::{Value, json};

pub(crate) fn message(m: &Message) -> Value {
    json!({
        "id": m.id(),
        "chat_id": m.peer_id().to_string(),
        "sender_id": m.sender_id().map(|p| p.to_string()),
        "date": m.date().to_rfc3339(),
        "text": m.text(),
        "reply_to": m.reply_to_message_id(),
    })
}

pub(crate) fn peer(p: &Peer) -> Value {
    let kind = match p {
        Peer::User(_) => "user",
        Peer::Group(_) => "group",
        Peer::Channel(_) => "channel",
    };
    json!({
        "id": p.id().to_string(),
        "kind": kind,
        "name": p.name(),
        "username": p.username(),
    })
}

pub(crate) fn dialog(d: &Dialog) -> Value {
    peer(d.peer())
}
