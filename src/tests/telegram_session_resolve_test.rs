//! Integration tests for Telegram session title + label drift (issue #121).

use crate::channels::telegram::session_resolve::{
    build_session_title, chat_id_suffix, should_refresh_label,
};
use crate::db::Database;
use crate::db::models::Session;
use crate::db::repository::SessionRepository;
use crate::services::{ServiceContext, SessionService};

async fn fresh_repo() -> (Database, SessionRepository) {
    let db = Database::connect_in_memory()
        .await
        .expect("in-memory DB connect");
    db.run_migrations().await.expect("migrations");
    let repo = SessionRepository::new(db.pool().clone());
    (db, repo)
}

#[test]
fn should_not_clobber_auto_titled_dm_title() {
    let auto = "Telegram: Fix deploy pipeline [chat:133526395]";
    let template = build_session_title(true, "Alexey", 133526395, "", 133526395);
    assert!(
        !should_refresh_label(auto, &template),
        "auto-titled DM must not revert to default template"
    );
}

#[test]
fn group_rename_still_refreshes() {
    let old = "Telegram: Old Group [chat:-5246593256]";
    let new = "Telegram: New Group [chat:-5246593256]";
    assert!(should_refresh_label(old, new));
}

#[tokio::test]
async fn suffix_lookup_after_switch_touch_picks_switched_row() {
    let (_db, repo) = fresh_repo().await;
    let chat_id = 42_i64;
    let suffix = chat_id_suffix(chat_id);
    let title = build_session_title(true, "U", 1, "", chat_id);

    let older = Session::new(Some(title.clone()), None, None);
    repo.create(&older).await.expect("create older");

    let mut newer = Session::new(Some(title), None, None);
    newer.updated_at = older.updated_at + chrono::Duration::seconds(1);
    repo.create(&newer).await.expect("create newer");

    // Simulate /sessions switch to older session (touch updated_at)
    let mut switched = older.clone();
    switched.updated_at = newer.updated_at + chrono::Duration::seconds(1);
    repo.update(&switched).await.expect("touch older");

    let hit = repo
        .find_by_title_suffix(&suffix)
        .await
        .expect("query")
        .expect("hit");
    assert_eq!(hit.id, older.id);
}

#[tokio::test]
async fn auto_titled_title_survives_should_refresh_check() {
    let template = build_session_title(true, "Alice", 1, "", 99);
    let auto_titled = format!(
        "Telegram: Deploy fix {}",
        chat_id_suffix(99)
    );
    assert!(!should_refresh_label(&auto_titled, &template));
}

#[tokio::test]
async fn service_update_session_title_preserves_suffix() {
    let db = Database::connect_in_memory()
        .await
        .expect("connect");
    db.run_migrations().await.expect("migrations");
    let ctx = ServiceContext::new(db.pool().clone());
    let svc = SessionService::new(ctx);

    let title = build_session_title(true, "U", 1, "", 77);
    let session = svc
        .create_session(Some(title.clone()))
        .await
        .expect("create");

    let new_title = format!("Telegram: Custom topic {}", chat_id_suffix(77));
    svc.update_session_title(session.id, Some(new_title.clone()))
        .await
        .expect("rename");

    let loaded = svc.get_session(session.id).await.expect("get").expect("row");
    assert_eq!(loaded.title.as_deref(), Some(new_title.as_str()));
    assert!(
        loaded.title.as_ref().unwrap().ends_with("[chat:77]"),
        "suffix must remain for lookup"
    );
}
