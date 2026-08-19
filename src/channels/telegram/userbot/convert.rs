//! grammers → Bot API message conversion.
//!
//! The userbot plane feeds the SAME `handler::handle_message` the bot plane
//! uses, so allowlists, session routing, menus, and replies work unchanged.
//! That handler speaks teloxide types, so an inbound grammers message is
//! rendered as minimal Bot-API-shaped JSON and deserialized into
//! `teloxide::types::Message` (same round-trip precedent as the raw-update
//! stash in `raw_updates.rs`, which builds typed Updates from raw JSON).
//!
//! Text messages only, by design: media has no Bot API JSON representation
//! reachable from MTProto structs without a download round-trip. Media
//! messages are skipped; the bot plane still handles those natively.

use grammers_client::message::Message as GrMessage;
use grammers_client::peer::Peer;
use grammers_session::types::PeerKind;
use serde_json::{Value, json};

/// Convert an inbound grammers message into a teloxide message.
///
/// Returns `None` when the message cannot be represented for the bot handler:
/// missing text (media/service), missing sender, or an unresolvable chat id.
pub(crate) fn to_bot_api(msg: &GrMessage) -> Option<teloxide::types::Message> {
    if msg.text().is_empty() {
        return None;
    }
    let chat_id = msg.peer_id().bot_api_dialog_id()?;
    let sender = msg.sender()?;
    let Peer::User(user) = sender else {
        // Channel/anonymous-group senders have no Bot API `from` user; the
        // handler's first gate requires one, so they are not forwardable.
        return None;
    };

    let (chat_type, chat_title) = match msg.peer_id().kind() {
        PeerKind::User => ("private", None),
        PeerKind::Chat => ("group", msg.peer().and_then(Peer::name).map(str::to_owned)),
        PeerKind::Channel => (
            "supergroup",
            msg.peer().and_then(Peer::name).map(str::to_owned),
        ),
    };

    let mut chat = json!({
        "id": chat_id,
        "type": chat_type,
    });
    if let Some(title) = chat_title {
        chat["title"] = Value::String(title);
    }
    if chat_type == "private"
        && let Some(name) = user.first_name()
    {
        chat["first_name"] = Value::String(name.to_owned());
    }

    let value = json!({
        "message_id": msg.id(),
        "date": msg.date().timestamp(),
        "chat": chat,
        "from": {
            "id": user.id().bot_api_dialog_id().unwrap_or_default(),
            "is_bot": false,
            "first_name": user.first_name().unwrap_or("user"),
        },
        "text": msg.text(),
    });

    serde_json::from_value(value).ok()
}
