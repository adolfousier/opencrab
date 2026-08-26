//! Background-task and sub-agent pushes land in the topic that owns the
//! session, not whichever topic spoke last (#1200).
//!
//! Sessions have been per-topic since #215 and `register_session_chat` records
//! the topic, but the resume path asked `latest_thread_id_for_chat`, which
//! answers for the whole chat. In a forum, any traffic in another topic while
//! a detached command ran redirected the result there. Both push types share
//! one callback, so both were affected.

use uuid::Uuid;

use crate::channels::telegram::TelegramState;

#[tokio::test]
async fn test_session_topic_is_recorded_and_resolves_per_session() {
    let state = TelegramState::new();
    let chat = -1003936827469i64;
    let in_topic = Uuid::new_v4();
    let in_general = Uuid::new_v4();

    state
        .register_session_chat(in_topic, chat, Some(30045))
        .await;
    state.register_session_chat(in_general, chat, None).await;

    assert_eq!(
        state.session_topic(in_topic).await,
        Some(30045),
        "#1200: the owning topic is what the push must target"
    );
    assert_eq!(
        state.session_topic(in_general).await,
        None,
        "General / non-forum stays None so the chat-wide fallback applies"
    );
}

#[tokio::test]
async fn test_two_topics_in_one_chat_keep_separate_destinations() {
    // The exact shape of the bug: two live sessions in one forum. Resolving
    // by chat collapses them onto one destination; resolving by session does
    // not.
    let state = TelegramState::new();
    let chat = -1003936827469i64;
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();

    state.register_session_chat(a, chat, Some(101)).await;
    state.register_session_chat(b, chat, Some(202)).await;

    assert_eq!(state.session_topic(a).await, Some(101));
    assert_eq!(
        state.session_topic(b).await,
        Some(202),
        "#1200: the later registration must not capture the earlier session"
    );
    // And the reverse lookup still distinguishes them.
    assert_eq!(state.chat_session(chat, Some(101)).await, Some(a));
    assert_eq!(state.chat_session(chat, Some(202)).await, Some(b));
}

#[tokio::test]
async fn test_unknown_session_has_no_topic_so_the_fallback_applies() {
    let state = TelegramState::new();
    assert_eq!(
        state.session_topic(Uuid::new_v4()).await,
        None,
        "#1200: an unbound session must fall through to the chat-wide lookup, \
         which is today's behaviour rather than a new failure"
    );
}
