//! Plan-card tracking survives a restart (#809 defect 1).
//!
//! Which message carries a session's card lived only in a process-local map.
//! A restart wiped it, and from then on the card could neither be edited (no
//! tracked id) nor removed (the take returned nothing). The next turn posted a
//! SECOND card below the stale one, so the chat accumulated duplicates while
//! the old checklist stayed frozen on whatever state it died at.
//!
//! Fixtures are synthetic and carry no user identifiers.

use crate::db::Database;
use crate::db::repository::{PlanCard, PlanCardRepository};

async fn repo() -> PlanCardRepository {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    PlanCardRepository::new(db.pool().clone())
}

fn card(session: &str, message_id: i64, signature: &str) -> PlanCard {
    PlanCard {
        session_id: session.to_string(),
        chat_id: -1001234567890,
        thread_id: Some(42),
        message_id,
        signature: signature.to_string(),
    }
}

#[tokio::test]
async fn a_tracked_card_is_recovered() {
    // The restart case: a later process must find the same message.
    let repo = repo().await;
    repo.set(card("session-a", 555, "sig-1")).await.unwrap();

    let found = repo
        .get("session-a")
        .await
        .unwrap()
        .expect("card must persist");
    assert_eq!(found.message_id, 555);
    assert_eq!(found.chat_id, -1001234567890);
    assert_eq!(found.thread_id, Some(42));
}

#[tokio::test]
async fn the_signature_survives_too() {
    // The signature is what suppresses no-op edits. Losing it on restart meant
    // the first refresh always re-edited, spending an API call to write
    // identical content.
    let repo = repo().await;
    repo.set(card("session-a", 555, "sig-1")).await.unwrap();
    assert_eq!(
        repo.get("session-a").await.unwrap().unwrap().signature,
        "sig-1"
    );
}

#[tokio::test]
async fn re_tracking_updates_rather_than_duplicating() {
    // One card per session. A second row would reintroduce the duplicate this
    // fixes, just in the database instead of the chat.
    let repo = repo().await;
    repo.set(card("session-a", 555, "sig-1")).await.unwrap();
    repo.set(card("session-a", 999, "sig-2")).await.unwrap();

    let found = repo.get("session-a").await.unwrap().unwrap();
    assert_eq!(found.message_id, 999);
    assert_eq!(found.signature, "sig-2");
}

#[tokio::test]
async fn clearing_stops_tracking() {
    let repo = repo().await;
    repo.set(card("session-a", 555, "sig-1")).await.unwrap();
    repo.delete("session-a").await.unwrap();
    assert!(repo.get("session-a").await.unwrap().is_none());
}

#[tokio::test]
async fn sessions_do_not_share_a_card() {
    // Group topics run several sessions at once; one clearing its card must
    // not untrack another's.
    let repo = repo().await;
    repo.set(card("session-a", 111, "a")).await.unwrap();
    repo.set(card("session-b", 222, "b")).await.unwrap();
    repo.delete("session-a").await.unwrap();

    assert!(repo.get("session-a").await.unwrap().is_none());
    assert_eq!(
        repo.get("session-b").await.unwrap().unwrap().message_id,
        222
    );
}

#[tokio::test]
async fn a_dm_card_has_no_thread() {
    // thread_id is nullable: direct chats have no topic.
    let repo = repo().await;
    let mut dm = card("session-dm", 777, "sig");
    dm.thread_id = None;
    repo.set(dm).await.unwrap();
    assert_eq!(
        repo.get("session-dm").await.unwrap().unwrap().thread_id,
        None
    );
}

#[tokio::test]
async fn an_untracked_session_yields_nothing() {
    assert!(repo().await.get("never-seen").await.unwrap().is_none());
}
