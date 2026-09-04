//! Telegram resume: sender label and short session id formatting.

use crate::channels::telegram::TelegramState;
use crate::channels::telegram::resume::*;
use uuid::Uuid;

/// Owner ruling 2026-08-28: a session sitting in a DM with the bot must
/// be labelled with the BOT's username, never the reader's own name —
/// the reader IS the chat's human side, so handing it back as the
/// sender is useless.
#[tokio::test]
async fn dm_session_labels_the_bot_not_the_reader() {
    let state = TelegramState::new();
    state.set_bot_username("test_bot".to_owned()).await;
    let sender = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
    state.register_session_chat(sender, 12345, None).await;
    let bot = teloxide::Bot::new("42:TEST");
    assert_eq!(
        sender_label(&state, &bot, sender, -100_999).await,
        "test_bot"
    );
}

/// Empty get_me cache (shouldn't happen post-boot): the DM arm must
/// degrade to the short session id, never to a getChat lookup that
/// would return the reader's own profile.
#[tokio::test]
async fn dm_session_without_cached_bot_username_falls_back_to_short_id() {
    let state = TelegramState::new();
    let sender = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
    state.register_session_chat(sender, 12345, None).await;
    let bot = teloxide::Bot::new("42:TEST");
    assert_eq!(
        sender_label(&state, &bot, sender, -100_999).await,
        short_session_id(sender)
    );
}
