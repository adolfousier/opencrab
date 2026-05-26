//! Integration tests for Telegram session title + label drift (issue #121).

use crate::channels::telegram::session_resolve::{
    build_session_title, chat_id_suffix, choose_resolve_source, session_idle_expired,
    should_refresh_label, ResolveSource,
};
use crate::channels::telegram::TelegramState;
use uuid::Uuid;
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
fn resolve_policy_prefers_chat_bound_over_suffix_winner() {
    let bound = Uuid::new_v4();
    let suffix = Uuid::new_v4();
    assert_eq!(
        choose_resolve_source(Some(bound), false, Some(suffix)),
        ResolveSource::ChatBound
    );
}

#[tokio::test]
async fn telegram_state_chat_map_survives_suffix_competition() {
    let state = TelegramState::new();
    let chat_id = 4242_i64;
    let bound = Uuid::new_v4();
    let suffix_winner = Uuid::new_v4();
    state.register_session_chat(bound, chat_id).await;
    assert_eq!(state.chat_session(chat_id).await, Some(bound));
    assert_eq!(
        choose_resolve_source(state.chat_session(chat_id).await, false, Some(suffix_winner)),
        ResolveSource::ChatBound
    );
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

/// Mirrors handler chat-bound idle branch: archive stale bound row, create replacement.
/// Guest /sessions switch only needs register_session_chat (extra_sessions map removed).
#[tokio::test]
async fn register_session_chat_binds_guest_dm() {
    let state = TelegramState::new();
    let guest_chat_id = 9988_i64;
    let session_id = Uuid::new_v4();
    state.register_session_chat(session_id, guest_chat_id).await;
    assert_eq!(state.chat_session(guest_chat_id).await, Some(session_id));
    assert_eq!(
        choose_resolve_source(state.chat_session(guest_chat_id).await, false, None),
        ResolveSource::ChatBound
    );
}

#[tokio::test]
async fn archived_chat_map_entry_uses_suffix_not_bound() {
    let bound = Uuid::new_v4();
    let suffix = Uuid::new_v4();
    assert_eq!(
        choose_resolve_source(Some(bound), true, Some(suffix)),
        ResolveSource::Suffix
    );
}

#[tokio::test]
async fn suffix_path_when_chat_map_empty() {
    let suffix = Uuid::new_v4();
    assert_eq!(
        choose_resolve_source(None, false, Some(suffix)),
        ResolveSource::Suffix
    );
    assert_eq!(
        choose_resolve_source(None, false, None),
        ResolveSource::Create
    );
}

#[tokio::test]
async fn chat_bound_idle_archives_and_creates_new_session() {
    let (db, repo) = fresh_repo().await;
    let ctx = ServiceContext::new(db.pool().clone());
    let svc = SessionService::new(ctx.clone());
    let chat_id = 77_i64;
    let title = build_session_title(true, "U", 1, "", chat_id);

    let mut bound = Session::new(Some(title.clone()), None, None);
    bound.updated_at = chrono::Utc::now() - chrono::Duration::hours(48);
    repo.create(&bound).await.expect("create bound");
    assert!(session_idle_expired(bound.updated_at, Some(1.0)));

    repo.archive(bound.id).await.expect("archive idle bound");
    let new_session = svc
        .create_session(Some(title))
        .await
        .expect("create replacement");

    assert_ne!(new_session.id, bound.id);
    let archived = svc
        .get_session(bound.id)
        .await
        .expect("get")
        .expect("row");
    assert!(archived.is_archived());
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
