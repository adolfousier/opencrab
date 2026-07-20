//! Regression: storing a new cancel token must NOT cancel the previous one
//! (#652). A near-simultaneous resend used to hard-cancel the user's in-flight
//! turn, dropping the running request; try_begin_turn is now the concurrency
//! gate, so store_cancel_token just overwrites.

use crate::channels::telegram::TelegramState;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tokio::test]
async fn storing_a_new_token_does_not_cancel_the_in_flight_one() {
    let state = Arc::new(TelegramState::new());
    let session = Uuid::new_v4();

    let first = CancellationToken::new();
    state.store_cancel_token(session, first.clone()).await;

    // A resend arrives and stores its own token for the same session.
    let second = CancellationToken::new();
    state.store_cancel_token(session, second.clone()).await;

    // The first turn's request must NOT have been cancelled (that was the drop).
    assert!(
        !first.is_cancelled(),
        "storing a new token cancelled the in-flight turn — the #652 drop"
    );
    assert!(!second.is_cancelled());
}

#[tokio::test]
async fn explicit_cancel_session_still_cancels() {
    // Genuine cancellation (/stop, provider swap) goes through cancel_session
    // and must still work.
    let state = Arc::new(TelegramState::new());
    let session = Uuid::new_v4();
    let token = CancellationToken::new();
    state.store_cancel_token(session, token.clone()).await;

    assert!(state.cancel_session(session).await);
    assert!(token.is_cancelled());
}
