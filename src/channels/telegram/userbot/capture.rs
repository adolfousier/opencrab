//! Pure receive-plane mapping and gating.

use grammers_client::message::Message as GrMessage;
use grammers_client::peer::Peer;
use sha2::{Digest as _, Sha256};

use crate::db::models::ChannelMessage;

/// Exact numeric match against the configured chat set. Empty means dry mode.
pub(crate) fn chat_allowed(allowed: &[String], chat_id: i64) -> bool {
    let id = chat_id.to_string();
    allowed.iter().any(|entry| entry.trim() == id)
}

/// Stable row id for one MTProto-captured Telegram message. The deterministic
/// id makes update redelivery idempotent without another state structure.
pub(crate) fn message_row_id(chat_id: i64, message_id: i32) -> uuid::Uuid {
    let digest = Sha256::digest(format!("telegram-userbot:{chat_id}:{message_id}"));
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // RFC 9562 variant + version 8 (application-defined SHA-256 payload).
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}

/// Convert only passive, representable text. No network calls and no synthetic
/// Bot API message: the MTProto object remains data until it reaches storage.
pub(crate) fn to_channel_message(message: &GrMessage) -> Option<ChannelMessage> {
    let content = message.text().trim();
    if content.is_empty() || message.outgoing() || message.via_bot_id().is_some() {
        return None;
    }

    let chat_id = message.peer_id().bot_api_dialog_id()?;
    let sender_id = message.sender_id().and_then(|id| id.bot_api_dialog_id());
    let sender = message.sender();
    if matches!(sender, Some(Peer::User(user)) if user.is_bot()) {
        return None;
    }

    // Broadcast posts often carry no human sender. Preserve them under the
    // channel identity instead of silently dropping the core use case.
    let sender_id = sender_id.unwrap_or(chat_id);
    let sender_name = sender
        .and_then(Peer::name)
        .or_else(|| message.peer().and_then(Peer::name))
        .unwrap_or("unknown")
        .to_owned();
    let chat_name = message.peer().and_then(Peer::name).map(str::to_owned);
    let mut row = ChannelMessage::new(
        "telegram-userbot".to_owned(),
        chat_id.to_string(),
        chat_name,
        sender_id.to_string(),
        sender_name,
        content.to_owned(),
        "text".to_owned(),
        Some(message.id().to_string()),
    );
    row.id = message_row_id(chat_id, message.id());
    row.created_at = message.date();
    Some(row)
}
