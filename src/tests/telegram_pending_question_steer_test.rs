//! Tests for answering a pending `follow_up_question` with a mid-turn TEXT
//! message instead of a button tap (#500). The tool blocks inside `rx.await`,
//! so a text queued between rounds never reaches it; the reverse
//! session → question map lets the handler fire the oneshot with the text.

use crate::channels::telegram::TelegramState;
use tokio::sync::oneshot;
use uuid::Uuid;

#[tokio::test]
async fn text_answers_pending_question() {
    let state = TelegramState::new();
    let session = Uuid::new_v4();
    let (tx, rx) = oneshot::channel::<String>();
    state
        .register_pending_question(
            "q-1".to_string(),
            session,
            tx,
            vec!["yes".to_string(), "no".to_string()],
        )
        .await;

    // The user typed free-form text instead of tapping a button.
    let resolved = state
        .resolve_pending_question_with_text(session, "cancel it".to_string())
        .await;
    assert!(resolved, "a live pending question must resolve from text");

    // The blocked tool receives the raw text verbatim as its answer.
    assert_eq!(rx.await.unwrap(), "cancel it");

    // Reverse map is cleared: a second text finds nothing pending.
    assert!(
        !state
            .resolve_pending_question_with_text(session, "again".to_string())
            .await,
        "reverse map must be cleared after the first resolve"
    );
}

#[tokio::test]
async fn text_without_pending_question_falls_through() {
    let state = TelegramState::new();
    let session = Uuid::new_v4();
    assert!(
        !state
            .resolve_pending_question_with_text(session, "hello".to_string())
            .await,
        "no pending question → false so the handler queues normally"
    );
}

#[tokio::test]
async fn button_resolve_clears_reverse_map() {
    let state = TelegramState::new();
    let session = Uuid::new_v4();
    let (tx, rx) = oneshot::channel::<String>();
    state
        .register_pending_question(
            "q-2".to_string(),
            session,
            tx,
            vec!["red".to_string(), "green".to_string()],
        )
        .await;

    // Button-click path resolves by option index.
    assert_eq!(
        state.resolve_pending_question("q-2", 1).await.as_deref(),
        Some("green")
    );
    assert_eq!(rx.await.unwrap(), "green");

    // A text arriving after the click must NOT re-resolve the same question.
    assert!(
        !state
            .resolve_pending_question_with_text(session, "blue".to_string())
            .await,
        "button resolve must also clear the reverse session → question entry"
    );
}

#[tokio::test]
async fn closed_receiver_falls_through() {
    let state = TelegramState::new();
    let session = Uuid::new_v4();
    let (tx, rx) = oneshot::channel::<String>();
    state
        .register_pending_question("q-3".to_string(), session, tx, vec!["a".to_string()])
        .await;

    // Tool gave up (timeout) and dropped its receiver.
    drop(rx);
    assert!(
        !state
            .resolve_pending_question_with_text(session, "late".to_string())
            .await,
        "a closed receiver → false so the text is queued instead of lost"
    );
}

#[tokio::test]
async fn clear_pending_question_is_idempotent() {
    let state = TelegramState::new();
    let session = Uuid::new_v4();
    let (tx, _rx) = oneshot::channel::<String>();
    state
        .register_pending_question("q-4".to_string(), session, tx, vec!["x".to_string()])
        .await;

    state.clear_pending_question("q-4", session).await;
    // Second clear is a no-op, not a panic.
    state.clear_pending_question("q-4", session).await;

    assert!(
        !state
            .resolve_pending_question_with_text(session, "y".to_string())
            .await,
        "cleared question must not resolve"
    );
}
