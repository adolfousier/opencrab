//! Inline-button callbacks resolve the same session the message handler bound
//! (#1248).
//!
//! `/models` taps in a forum group's General topic wrote the provider/model
//! pick into the pre-#1220 base session (`[chat:<id>]`) while every message in
//! the same chat was served by the General session (`[chat:<id>:topic:1]`).
//! The switch looked like it was silently ignored: the picker confirmed a new
//! pair, the next turn still ran the old one, and the DB row that changed
//! belonged to a session serving nobody.
//!
//! Root cause was a two-line drift: ingress composed
//! `normalize_topic(topic_session_id(..), known_forum)`, the callback resolver
//! called `topic_session_id` alone. Both now go through
//! `session_topic_for_event`, and these tests pin them together.

use uuid::Uuid;

use crate::channels::telegram::TelegramState;
use crate::channels::telegram::session_resolve::{
    GENERAL_TOPIC_ID, normalize_topic, session_topic_for_event, topic_session_id,
};

/// The shared helper must equal the ingress composition for every input, or
/// the drift is back.
#[test]
fn session_topic_for_event_matches_ingress_composition() {
    for is_topic_message in [true, false] {
        for thread_id in [None, Some(1), Some(30045)] {
            for known_forum in [true, false] {
                assert_eq!(
                    session_topic_for_event(is_topic_message, thread_id, known_forum),
                    normalize_topic(topic_session_id(is_topic_message, thread_id), known_forum),
                    "drifted at is_topic_message={is_topic_message}, \
                     thread_id={thread_id:?}, known_forum={known_forum}"
                );
            }
        }
    }
}

/// General topic of a KNOWN forum: the callback must resolve the General
/// bucket, not the base one. This is the exact input the `/models` picker sees
/// (General messages carry no thread id and are not flagged topic messages).
#[test]
fn general_topic_callback_resolves_the_general_bucket() {
    assert_eq!(
        session_topic_for_event(false, None, true),
        Some(GENERAL_TOPIC_ID),
        "#1248: in a known forum, no explicit topic IS General"
    );
    // The old callback composition — kept here as the regression witness.
    assert_eq!(
        topic_session_id(false, None),
        None,
        "the pre-fix callback path resolved None and hit the base session"
    );
}

/// Cold start (chat not yet proven to be a forum) keeps the legacy behaviour,
/// so a DM or plain group is untouched by the fix.
#[test]
fn unknown_forum_keeps_base_session() {
    assert_eq!(session_topic_for_event(false, None, false), None);
}

/// A real forum topic resolves to itself whether or not the forum is known.
#[test]
fn real_topic_is_never_rewritten() {
    assert_eq!(
        session_topic_for_event(true, Some(30045), true),
        Some(30045)
    );
    assert_eq!(
        session_topic_for_event(true, Some(30045), false),
        Some(30045)
    );
}

/// End to end over the same map the callback resolver reads: with both a stale
/// base binding and a live General binding registered for one chat, the fixed
/// composition selects the live one and the old composition selects the stale
/// one.
#[tokio::test]
async fn callback_lookup_lands_on_the_session_that_serves_messages() {
    let state = TelegramState::new();
    let chat = -1004407925328i64;
    let live_general = Uuid::new_v4();
    let stale_base = Uuid::new_v4();

    // Ingress in a known forum binds General under Some(GENERAL_TOPIC_ID).
    state
        .register_session_chat(live_general, chat, Some(GENERAL_TOPIC_ID))
        .await;
    // A pre-#1220 row for the same chat still sits under None.
    state.register_session_chat(stale_base, chat, None).await;

    let fixed = session_topic_for_event(false, None, true);
    assert_eq!(
        state.chat_session(chat, fixed).await,
        Some(live_general),
        "#1248: the button must act on the session that answers messages"
    );

    let pre_fix = topic_session_id(false, None);
    assert_eq!(
        state.chat_session(chat, pre_fix).await,
        Some(stale_base),
        "witness: this is the row the picker used to write into"
    );
}
