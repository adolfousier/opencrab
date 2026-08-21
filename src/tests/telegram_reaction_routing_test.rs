//! A reaction lands on the session that owns the message, or nowhere.
//!
//! Sessions are keyed by `(chat_id, topic_id)`. A reaction update carries no
//! thread id, so the handler looked one up with `topic_id = None` — which can
//! never match a forum topic's session, since that is registered under
//! `Some(topic)`. Every reaction in a topic therefore missed and fell through
//! to a process-global "shared session" with no relationship to the chat: the
//! reaction was injected into whatever session was current and answered there,
//! in front of people with no access to it.

use uuid::Uuid;

use crate::channels::telegram::state::TelegramState;

const CHAT: i64 = -1004428873948;
const TOPIC: i32 = 249;

#[tokio::test]
async fn a_topic_reaction_finds_its_own_session() {
    let state = TelegramState::new();
    let topic_session = Uuid::new_v4();
    state
        .register_session_chat(topic_session, CHAT, Some(TOPIC))
        .await;

    assert_eq!(
        state.chat_session(CHAT, Some(TOPIC)).await,
        Some(topic_session)
    );
}

#[tokio::test]
async fn the_old_none_lookup_could_never_match_a_topic() {
    // Pins the regression itself: this is the lookup the handler used, and it
    // misses every time in a forum, which is what sent the reaction elsewhere.
    let state = TelegramState::new();
    state
        .register_session_chat(Uuid::new_v4(), CHAT, Some(TOPIC))
        .await;

    assert_eq!(
        state.chat_session(CHAT, None).await,
        None,
        "a topic's session is not reachable under the chat's bare key"
    );
}

#[tokio::test]
async fn one_topic_never_answers_for_another() {
    let state = TelegramState::new();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    state.register_session_chat(a, CHAT, Some(TOPIC)).await;
    state.register_session_chat(b, CHAT, Some(TOPIC + 1)).await;

    assert_eq!(state.chat_session(CHAT, Some(TOPIC)).await, Some(a));
    assert_eq!(state.chat_session(CHAT, Some(TOPIC + 1)).await, Some(b));
}

#[tokio::test]
async fn a_direct_chat_still_resolves_under_no_topic() {
    // A non-forum message stores no thread, so the key stays None on both ends.
    let state = TelegramState::new();
    let dm = Uuid::new_v4();
    state.register_session_chat(dm, CHAT, None).await;

    assert_eq!(state.chat_session(CHAT, None).await, Some(dm));
}

#[tokio::test]
async fn an_unmapped_chat_resolves_to_nothing() {
    // What the handler now does with this: drop the reaction. Anything else is
    // a guess, and the guess is what leaked.
    let state = TelegramState::new();
    assert_eq!(state.chat_session(CHAT, Some(TOPIC)).await, None);
}

// ── the shared session may only answer for its own chat ───────────────

#[tokio::test]
async fn the_shared_session_is_usable_for_its_own_chat() {
    // The case the fallback exists for: an owner DM arriving before any
    // message handler registered the chat. The shared session's own chat is
    // this one, so it still resolves.
    let state = TelegramState::new();
    let shared = Uuid::new_v4();
    state.register_session_chat(shared, CHAT, None).await;

    assert_eq!(state.session_chat(shared).await, Some(CHAT));
}

#[tokio::test]
async fn the_shared_session_cannot_answer_for_another_chat() {
    // The reach that put a group's reaction into an unrelated session: a
    // process-global handle belonging to a different chat entirely.
    let state = TelegramState::new();
    let shared = Uuid::new_v4();
    state.register_session_chat(shared, CHAT, None).await;

    assert_ne!(
        state.session_chat(shared).await,
        Some(-100999999999),
        "a session bound to one chat must not vouch for a press in another"
    );
}
