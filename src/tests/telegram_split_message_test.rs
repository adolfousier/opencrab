//! Regression (#950): a client-split long message reaches the agent whole.
//!
//! Telegram's client breaks a message too long to send into several. The
//! pieces carry no grouping id — unlike an album, nothing marks fragment two as
//! a continuation — so the first one started a turn on its own and the agent
//! answered half a sentence and reported the message as cut off. The remainder
//! arrived afterwards as a separate mid-turn injection.
//!
//! Fragments are now held briefly and joined. Length is the only available
//! signal, so these tests pin both halves of that trade: a near-limit message
//! waits, and an ordinary one never does.

use crate::channels::telegram::TelegramState;
use crate::channels::telegram::handler::is_split_candidate;

/// Telegram's limit, in UTF-16 code units.
const LIMIT: usize = 4096;

fn text_of(len: usize) -> String {
    "a".repeat(len)
}

#[test]
fn an_ordinary_message_is_never_held() {
    // The cost of getting this wrong is a delay on every single message, so it
    // matters more than the split case itself.
    for len in [0usize, 1, 50, 500, 2000, 3900] {
        assert!(
            !is_split_candidate(&text_of(len)),
            "a {len}-char message must dispatch immediately"
        );
    }
}

#[test]
fn a_message_at_the_send_limit_is_held() {
    assert!(is_split_candidate(&text_of(LIMIT)));
}

#[test]
fn a_fragment_just_short_of_the_limit_is_held() {
    // Clients break at a whitespace boundary, so a fragment lands a little
    // under the ceiling rather than exactly on it. Missing these would defeat
    // the whole fix, since a real fragment is never exactly 4096.
    assert!(is_split_candidate(&text_of(LIMIT - 1)));
    assert!(is_split_candidate(&text_of(LIMIT - 100)));
}

#[test]
fn the_threshold_is_measured_in_utf16_like_telegram_counts_it() {
    // Telegram's limit is UTF-16 code units. Counting bytes would hold a short
    // emoji message, counting chars would let a long one straight through.
    let emoji = "🦀".repeat(LIMIT / 2); // 2 UTF-16 units each → exactly at the limit
    assert_eq!(emoji.chars().count(), LIMIT / 2, "half as many chars");
    assert!(
        is_split_candidate(&emoji),
        "a message at the limit in UTF-16 must be held even though it is half the char count"
    );

    // The same char count in ASCII is nowhere near the limit and must not wait.
    assert!(!is_split_candidate(&text_of(LIMIT / 2)));
}

#[tokio::test]
async fn fragments_are_returned_in_arrival_order() {
    // Order is the message. Reversing it would be worse than truncating it.
    let state = TelegramState::new();
    for part in ["first", "second", "third"] {
        state.buffer_text(-100, 42, part.to_string()).await;
    }
    assert_eq!(
        state.drain_text_buffer(-100, 42).await,
        vec!["first", "second", "third"]
    );
}

#[tokio::test]
async fn draining_empties_the_buffer() {
    // A leftover fragment would prepend itself to the user's next message.
    let state = TelegramState::new();
    state.buffer_text(-100, 42, "one".into()).await;
    assert_eq!(state.drain_text_buffer(-100, 42).await, vec!["one"]);
    assert!(
        state.drain_text_buffer(-100, 42).await.is_empty(),
        "the second drain must find nothing"
    );
}

#[tokio::test]
async fn two_senders_in_one_chat_do_not_merge() {
    // Two people pasting long text at once must not have their messages joined.
    let state = TelegramState::new();
    state.buffer_text(-100, 1, "from-one".into()).await;
    state.buffer_text(-100, 2, "from-two".into()).await;
    assert_eq!(state.drain_text_buffer(-100, 1).await, vec!["from-one"]);
    assert_eq!(state.drain_text_buffer(-100, 2).await, vec!["from-two"]);
}

#[tokio::test]
async fn one_sender_in_two_chats_does_not_merge() {
    let state = TelegramState::new();
    state.buffer_text(-100, 42, "in-a".into()).await;
    state.buffer_text(-200, 42, "in-b".into()).await;
    assert_eq!(state.drain_text_buffer(-100, 42).await, vec!["in-a"]);
    assert_eq!(state.drain_text_buffer(-200, 42).await, vec!["in-b"]);
}

#[tokio::test]
async fn a_new_fragment_cancels_the_previous_wait() {
    // The token handed to the earlier fragment must be cancelled, so that
    // fragment bows out and the later one dispatches the whole buffer. Without
    // this both would fire and the message would be answered twice.
    let state = TelegramState::new();
    let first = state.reset_text_debounce(-100, 42).await;
    assert!(!first.is_cancelled());

    let second = state.reset_text_debounce(-100, 42).await;
    assert!(first.is_cancelled(), "the earlier fragment must stand down");
    assert!(!second.is_cancelled(), "the latest one owns the buffer");
}

#[tokio::test]
async fn a_cancelled_wait_reports_that_it_did_not_expire() {
    // false means "another fragment took over", which is what tells the
    // handler to return without dispatching.
    let state = TelegramState::new();
    let token = state.reset_text_debounce(-100, 42).await;
    token.cancel();
    assert!(!state.wait_text_debounce(token).await);
}

#[tokio::test]
async fn debounce_tokens_are_scoped_per_sender() {
    // A second sender's message must not cancel the first sender's wait.
    let state = TelegramState::new();
    let a = state.reset_text_debounce(-100, 1).await;
    let _b = state.reset_text_debounce(-100, 2).await;
    assert!(
        !a.is_cancelled(),
        "another sender's fragment must not cut this one's wait short"
    );
}
