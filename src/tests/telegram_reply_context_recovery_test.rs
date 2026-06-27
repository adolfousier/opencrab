//! Regression test for issue #234: Reply context lost for bot rich messages.
//!
//! When a user replies to a bot's rich message (photo/video/document) in a
//! Telegram group *without* highlighting text, Bot API 10.1 delivers:
//!   - `full_text` = "" (no text()/caption() on the replied-to rich message)
//!   - `msg.quote()` = None (Telegram clients don't support quoting rich messages)
//!
//! Before #225, recovery was gated on `Some(quote)` so these replies produced
//! `format_reply_context("") → None`, losing all reply context.  Since #225,
//! the handler recovers `full_text` from `channel_messages` for bot replies,
//! so `format_reply_context(recovered_text, "") → Some(...)`.
//!
//! This test pins the pure `format_reply_context` function to prove:
//! 1. Recovered bot message text + no quote → context IS present (#225 fix)
//! 2. Both empty (pre-#225 state) → context IS absent (the old bug)
//! 3. Empty full + non-empty quote → context IS present (normal user quote)

use crate::channels::telegram::handler::format_reply_context;

/// After the #225 recovery path populates `full_text` from `channel_messages`,
/// the function must return context even when `quote_text` is empty.
#[test]
fn recovered_bot_rich_message_with_no_quote_produces_context() {
    let ctx = format_reply_context("assistant", "Here is your photo!", "");
    assert!(
        ctx.is_some(),
        "format_reply_context must return Some when reply_full_text is populated by recovery path"
    );
    let text = ctx.unwrap();
    assert!(
        text.contains("assistant"),
        "context must include the sender name"
    );
    assert!(
        text.contains("Here is your photo!"),
        "context must include the recovered message text"
    );
    assert!(
        !text.contains("highlighted"),
        "context must NOT mention highlighting since quote_text is empty"
    );
}

/// Before #225: both sides empty → no context at all.  This is the exact
/// scenario the reporter described.  Pin it so a future refactor can't
/// accidentally reintroduce it.
#[test]
fn empty_full_and_empty_quote_returns_none() {
    let ctx = format_reply_context("assistant", "", "");
    assert!(
        ctx.is_none(),
        "format_reply_context must return None when both sides are empty (the pre-#225 bug)"
    );
}

/// Normal user quote (non-empty quote, no full text recovery) still works.
#[test]
fn quote_without_full_text_produces_context() {
    let ctx = format_reply_context("John", "", "check this part");
    assert!(
        ctx.is_some(),
        "format_reply_context must return Some when quote_text is present"
    );
    let text = ctx.unwrap();
    assert!(
        text.contains("check this part"),
        "context must include the quoted text"
    );
}

/// Both non-empty with distinct values → shows both full and quote.
#[test]
fn both_full_and_quote_distinct_shows_both() {
    let ctx = format_reply_context("assistant", "Full bot message here.", "highlighted bit");
    assert!(ctx.is_some());
    let text = ctx.unwrap();
    assert!(text.contains("Full bot message here."));
    assert!(text.contains("highlighted bit"));
    assert!(text.contains("highlighted"), "must label the quote");
}

/// Empty full_text but non-empty quote with bot sender → still works.
/// This is the case where the user highlighted part of the bot's message
/// but the rich-message recovery didn't fire (e.g. the message had caption).
#[test]
fn bot_quote_without_recovery_still_works() {
    let ctx = format_reply_context("assistant", "", "look at this");
    assert!(
        ctx.is_some(),
        "bot quote without recovery must still produce context"
    );
}

// ── DM recovery (#234, the case #225 missed) ────────────────────────────────
//
// #225 only recovered bot text from `channel_messages`, which is gated on
// `!is_dm` — bot DMs are never stored there. So a DM reply to a rich bot
// message had NO recovery source and lost all context. The fix recovers the
// last assistant message from the session `messages` table via
// `MessageRepository::get_last_assistant_message`. These tests pin that.

use crate::db::models::Message;
use crate::db::{Database, MessageRepository};
use crate::services::{ServiceContext, SessionService};

/// The DM recovery source: the last assistant message in the session is
/// returned even when a later user row exists, and even though the incoming
/// reply isn't persisted yet at recovery time.
#[tokio::test]
async fn dm_recovery_returns_last_assistant_message() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let context = ServiceContext::new(db.pool().clone());
    let session = SessionService::new(context)
        .create_session(Some("DM".to_string()))
        .await
        .unwrap();

    let repo = MessageRepository::new(db.pool().clone());
    repo.create(&Message::new(
        session.id,
        "user".into(),
        "earlier question".into(),
        1,
    ))
    .await
    .unwrap();
    repo.create(&Message::new(
        session.id,
        "assistant".into(),
        "Here is your rich photo report.".into(),
        2,
    ))
    .await
    .unwrap();

    let recovered = repo
        .get_last_assistant_message(session.id)
        .await
        .unwrap()
        .expect("must find the assistant message for DM reply recovery");
    assert_eq!(recovered.content, "Here is your rich photo report.");

    // The recovered text flows into format_reply_context with an empty quote,
    // exactly as the DM rich-reply path does — context must be present.
    let ctx = format_reply_context("assistant", &recovered.content, "");
    assert!(ctx.is_some());
    assert!(ctx.unwrap().contains("Here is your rich photo report."));
}

/// `role = 'assistant'` filter: a trailing user row must NOT shadow the
/// assistant message the user is replying to.
#[tokio::test]
async fn dm_recovery_skips_trailing_user_message() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let context = ServiceContext::new(db.pool().clone());
    let session = SessionService::new(context)
        .create_session(Some("DM".to_string()))
        .await
        .unwrap();

    let repo = MessageRepository::new(db.pool().clone());
    repo.create(&Message::new(
        session.id,
        "assistant".into(),
        "bot reply text".into(),
        1,
    ))
    .await
    .unwrap();
    repo.create(&Message::new(
        session.id,
        "user".into(),
        "a later user line".into(),
        2,
    ))
    .await
    .unwrap();

    let recovered = repo
        .get_last_assistant_message(session.id)
        .await
        .unwrap()
        .expect("must return the assistant row, not the trailing user row");
    assert_eq!(recovered.content, "bot reply text");
}

/// Empty session (no assistant message yet) → None, and the handler then
/// leaves `full_text` empty (no false recovery).
#[tokio::test]
async fn dm_recovery_returns_none_when_no_assistant_message() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let context = ServiceContext::new(db.pool().clone());
    let session = SessionService::new(context)
        .create_session(Some("DM".to_string()))
        .await
        .unwrap();

    let repo = MessageRepository::new(db.pool().clone());
    repo.create(&Message::new(
        session.id,
        "user".into(),
        "only a user line".into(),
        1,
    ))
    .await
    .unwrap();

    let recovered = repo.get_last_assistant_message(session.id).await.unwrap();
    assert!(recovered.is_none());
}
