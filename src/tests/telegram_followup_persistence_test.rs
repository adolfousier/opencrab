//! Durable pending follow-up stash (#1226 items 3+4, G).
//!
//! Item 3: suggestion keyboards survived a restart as rendered orphans —
//! the token stash was in-memory only. These tests pin the repo roundtrip,
//! boot hydration, and the mirror-on-mutate lifecycle.
//!
//! Item 4: the standalone flood-fallback bubble must carry an expiry marker
//! so operators can tell a dead lamp from a live question.
//!
//! G: a mid-turn tap consumed the stash before the turn guard ran; the fix
//! re-arms the SAME token, pinned here at the state layer (the guard itself
//! needs a live bot, covered by the compile shape + warn log).

use crate::channels::telegram::state::{MergedHost, TelegramState};
use crate::channels::telegram::suggest_options::{SuggestLayout, standalone_fallback_body};
use crate::db::Database;
use crate::db::repository::pending_followup::{
    FollowupHost, PendingFollowup, PendingFollowupRepository,
};
use teloxide::types::MessageId;
use uuid::Uuid;

fn row(token: &str, session: Uuid, options: &[&str]) -> PendingFollowup {
    PendingFollowup {
        token: token.to_string(),
        session_id: session.to_string(),
        options: options.iter().map(|s| s.to_string()).collect(),
        host: None,
    }
}

#[tokio::test]
async fn repo_roundtrip_saves_loads_and_deletes() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingFollowupRepository::new(db.pool().clone());
    let sid = Uuid::new_v4();

    // Arm without a host.
    let entry = row("tok1", sid, &["run tests", "ship it"]);
    repo.save(&entry).await.unwrap();
    assert_eq!(repo.load_all().await.unwrap(), vec![entry.clone()]);

    // Upsert with a merge host: same token, host columns filled.
    let mut hosted = entry.clone();
    hosted.host = Some(FollowupHost {
        message_id: 4242,
        html: "<b>answer</b>".to_string(),
        rich: true,
        markdown: None,
    });
    repo.save(&hosted).await.unwrap();
    assert_eq!(repo.load_all().await.unwrap(), vec![hosted.clone()]);

    // Delete the tapped token.
    repo.delete("tok1").await.unwrap();
    assert!(repo.load_all().await.unwrap().is_empty());
}

#[tokio::test]
async fn repo_delete_session_clears_only_that_session() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingFollowupRepository::new(db.pool().clone());
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    repo.save(&row("t-a1", a, &["one"])).await.unwrap();
    repo.save(&row("t-a2", a, &["two"])).await.unwrap();
    repo.save(&row("t-b1", b, &["three"])).await.unwrap();

    repo.delete_session(&a.to_string()).await.unwrap();
    let left = repo.load_all().await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].token, "t-b1");
}

#[tokio::test]
async fn store_hydration_restores_armed_keyboards() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingFollowupRepository::new(db.pool().clone());
    let sid = Uuid::new_v4();
    // A previous process armed this keyboard (with a merge host) and died.
    let mut prior = row("boot1", sid, &["retry", "abort"]);
    prior.host = Some(FollowupHost {
        message_id: 99,
        html: "<p>answer</p>".to_string(),
        rich: false,
        markdown: None,
    });
    repo.save(&prior).await.unwrap();

    // Fresh process: empty state gets its store set at boot.
    let state = TelegramState::new();
    state.set_followup_store(repo.clone()).await;

    // The keyboard resolves a tap, host and all — no stale-shell strip.
    let (entry, text, _picked_idx) = state.take_pending_followup("boot1", 1).await.unwrap();
    assert_eq!(entry.session_id, sid);
    assert_eq!(text, "abort");
    let host = entry.host.expect("hydrated host survives");
    assert_eq!(host.message_id, MessageId(99));
    assert_eq!(host.html, "<p>answer</p>");
    assert!(!host.rich);
}

#[tokio::test]
async fn register_and_take_mirror_the_store() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingFollowupRepository::new(db.pool().clone());
    let state = TelegramState::new();
    state.set_followup_store(repo.clone()).await;

    let sid = Uuid::new_v4();
    let token = state
        .register_pending_followups(sid, vec!["alpha".to_string(), "beta".to_string()])
        .await;
    assert_eq!(repo.load_all().await.unwrap().len(), 1);

    // Host attach upserts the same token.
    state
        .attach_followup_host(
            &token,
            MergedHost {
                message_id: MessageId(7),
                html: "html".to_string(),
                rich: false,
                markdown: None,
            },
        )
        .await;
    let rows = repo.load_all().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].host.is_some());

    // Take consumes map + row.
    let (entry, text, _picked_idx) = state.take_pending_followup(&token, 0).await.unwrap();
    assert_eq!(text, "alpha");
    assert_eq!(entry.options.len(), 2);
    assert!(repo.load_all().await.unwrap().is_empty());
}

#[tokio::test]
async fn restore_revives_a_consumed_token_for_the_busy_guard() {
    // #1226 G: take_pending_followup runs BEFORE try_begin_turn, so a
    // mid-turn tap must re-arm the same token or the choice is eaten.
    let state = TelegramState::new();
    let sid = Uuid::new_v4();
    let token = state
        .register_pending_followups(sid, vec!["first".to_string(), "second".to_string()])
        .await;

    let (entry, _text, _picked_idx) = state.take_pending_followup(&token, 0).await.unwrap();
    assert!(state.take_pending_followup(&token, 0).await.is_none());

    // The guard's recovery path.
    state.restore_pending_followup(&token, entry).await;
    let (_revived, text, _picked_idx) = state.take_pending_followup(&token, 1).await.unwrap();
    assert_eq!(text, "second");
}

#[tokio::test]
async fn clear_pending_followups_also_clears_the_store() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingFollowupRepository::new(db.pool().clone());
    let state = TelegramState::new();
    state.set_followup_store(repo.clone()).await;

    let sid = Uuid::new_v4();
    state
        .register_pending_followups(sid, vec!["x".to_string()])
        .await;
    state.clear_pending_followups(sid).await;
    assert!(repo.load_all().await.unwrap().is_empty());
}

#[test]
fn standalone_fallback_body_marks_button_modes_only() {
    // Item 4: button modes carry the expiry marker (their keyboards are
    // subject to the stale-shell lifecycle); prose mode has no buttons to
    // expire, so its folded list stays clean.
    let options = vec!["run the suite".to_string(), "ship it".to_string()];
    let shared = standalone_fallback_body(&SuggestLayout::SharedRow, &options);
    assert!(
        shared.contains("choices may have expired"),
        "button fallback carries the expiry marker: {shared}"
    );
    let column = standalone_fallback_body(&SuggestLayout::Column, &options);
    assert!(column.contains("choices may have expired"));

    let prose = standalone_fallback_body(&SuggestLayout::NumberedProse, &options);
    assert!(
        !prose.contains("choices may have expired"),
        "prose fallback has no keyboard, no marker: {prose}"
    );
    assert!(!prose.is_empty(), "prose fallback keeps the folded list");
}
