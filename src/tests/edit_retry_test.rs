//! Regression tests for the shared Retry-After edit handler (#68).
//!
//! Incident 2026-09-01 18:17:26Z: a suggestion-button tap hit a Telegram 429
//! ("Retry after 12s") on the pick-record edit, and the immediate quoted-echo
//! fallback fired 105ms later INTO the same flood window and died too — the
//! tap was consumed with no visible effect. The fix defers ONE identical
//! retry past the server-instructed window; the fallback only runs on
//! exhaustion.
//!
//! Tests: (1) the string classifier used by the tap leg, whose arms fold
//! typed errors into strings before returning; (2) the typed classifier;
//! (3) the full wire behavior — attempt 1 draws a 429 with retry_after, the
//! deferred task re-fires after the window and lands.

use std::time::Duration;

use crate::channels::telegram::edit_retry::{self, EditErr};
use crate::channels::telegram::send::best_effort_note;
use teloxide::prelude::*;

const FOUR29_BODY: &str = r#"{"ok":false,"error_code":429,"description":"Too Many Requests: retry after 1","parameters":{"retry_after":1}}"#;

#[test]
fn classify_str_parses_teloxide_retry_after_marker() {
    // teloxide renders RequestError::RetryAfter as "Retry after <n>s" — the
    // exact surface the 18:17:26Z incident warn shows.
    match edit_retry::classify_str("Retry after 12s") {
        EditErr::RetryAfter(wait) => assert_eq!(wait, Duration::from_secs(12)),
        EditErr::Fatal(e) => panic!("expected RetryAfter, got Fatal: {e}"),
    }
}

#[test]
fn classify_str_falls_back_on_rich_429_marker() {
    // The rich arm buries "(429)" in its anyhow string (#30 precedent).
    match edit_retry::classify_str("rich render failed: (429) too many requests") {
        EditErr::RetryAfter(wait) => assert_eq!(wait, Duration::from_secs(30)),
        EditErr::Fatal(e) => panic!("expected RetryAfter, got Fatal: {e}"),
    }
}

#[test]
fn classify_str_fatal_errors_stay_fatal() {
    match edit_retry::classify_str("chat not found") {
        EditErr::RetryAfter(wait) => panic!("expected Fatal, got RetryAfter {wait:?}"),
        EditErr::Fatal(e) => assert_eq!(e, "chat not found"),
    }
}

#[tokio::test]
async fn classify_reads_typed_teloxide_retry_after() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/botTESTTOKEN/EditMessageText")
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_header("retry-after", "1")
        .with_body(FOUR29_BODY)
        .expect(1)
        .create_async()
        .await;
    let bot = Bot::with_client("TESTTOKEN", reqwest::Client::builder().build().unwrap())
        .set_api_url(server.url().parse().unwrap());

    let err = bot
        .edit_message_text(
            teloxide::types::ChatId(1),
            teloxide::types::MessageId(7),
            "x",
        )
        .await
        .expect_err("expected the 429 to surface as an error");

    match edit_retry::classify(&err) {
        EditErr::RetryAfter(wait) => assert_eq!(wait, Duration::from_secs(1)),
        EditErr::Fatal(e) => panic!("expected RetryAfter from typed 429, got Fatal: {e}"),
    }
    mock.assert_async().await;
}

#[tokio::test]
async fn best_effort_note_defers_one_retry_past_the_429_window() {
    let mut server = mockito::Server::new_async().await;
    // Attempt 1: 429 with retry_after=1 — the server-instructed window.
    let first = server
        .mock("POST", "/botTESTTOKEN/SendMessage")
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_header("retry-after", "1")
        .with_body(FOUR29_BODY)
        .expect(1)
        .create_async()
        .await;
    // Deferred retry, byte-identical request: 200 OK.
    let second = server
        .mock("POST", "/botTESTTOKEN/SendMessage")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            // Full Message, not just an id — teloxide decodes sendMessage's
            // result into `Message` (#28 lesson); a bare id fails decode and
            // the retry would read as failed even though the wire hit 200.
            r#"{"ok":true,"result":{"message_id":42,"date":1756166400,"chat":{"id":1,"first_name":"t","type":"private"},"text":"hello"}}"#,
        )
        .expect(1)
        .create_async()
        .await;
    let bot = Bot::with_client("TESTTOKEN", reqwest::Client::builder().build().unwrap())
        .set_api_url(server.url().parse().unwrap());

    best_effort_note(
        &bot,
        teloxide::types::ChatId(1),
        None,
        "hello",
        None,
        "test",
        "edit-retry",
        "wire test",
    )
    .await;

    // Attempt 1 fired synchronously and drew the 429.
    first.assert_async().await;
    // The deferred retry lands after the 1s server-instructed window. The
    // 1s wait is server-instructed and bounded — not a flake-sleep lottery.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    second.assert_async().await;
}
