//! Tests for `send_retrying_rate_limit` (#297): command replies must wait
//! out Telegram's RetryAfter (429) and retry instead of dropping. A per-chat
//! rate limit (typically a streaming turn editing its placeholder in the
//! same chat) may DELAY a programmatic reply, never lose it.

use crate::channels::telegram::handler::send_retrying_rate_limit;
use std::cell::Cell;
use teloxide::types::Seconds;

fn rate_limited<T>() -> Result<T, teloxide::RequestError> {
    // 0 seconds so tests do not sleep for real.
    Err(teloxide::RequestError::RetryAfter(Seconds::from_seconds(0)))
}

#[tokio::test]
async fn waits_out_rate_limits_then_delivers() {
    let calls = Cell::new(0u32);
    let out = send_retrying_rate_limit("test send", || {
        let n = calls.get() + 1;
        calls.set(n);
        async move { if n <= 2 { rate_limited() } else { Ok(42u32) } }
    })
    .await;
    assert_eq!(out.unwrap(), 42);
    assert_eq!(calls.get(), 3, "two rate-limited attempts, then success");
}

#[tokio::test]
async fn exhausted_retries_propagate_the_error() {
    let calls = Cell::new(0u32);
    let out: Result<(), _> = send_retrying_rate_limit("test send", || {
        calls.set(calls.get() + 1);
        async { rate_limited() }
    })
    .await;
    assert!(matches!(out, Err(teloxide::RequestError::RetryAfter(_))));
    // 1 initial attempt + 3 retries.
    assert_eq!(calls.get(), 4);
}

#[tokio::test]
async fn non_rate_limit_error_propagates_immediately() {
    let calls = Cell::new(0u32);
    let out: Result<(), _> = send_retrying_rate_limit("test send", || {
        calls.set(calls.get() + 1);
        async { Err(teloxide::RequestError::Api(teloxide::ApiError::BotBlocked)) }
    })
    .await;
    assert!(matches!(
        out,
        Err(teloxide::RequestError::Api(teloxide::ApiError::BotBlocked))
    ));
    assert_eq!(calls.get(), 1, "no retries for non-429 errors");
}

#[tokio::test]
async fn first_try_success_sends_once() {
    let calls = Cell::new(0u32);
    let out = send_retrying_rate_limit("test send", || {
        calls.set(calls.get() + 1);
        async { Ok::<_, teloxide::RequestError>("ok") }
    })
    .await;
    assert_eq!(out.unwrap(), "ok");
    assert_eq!(calls.get(), 1);
}
