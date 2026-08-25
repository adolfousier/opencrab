//! #1159: A2A conversation continuity.
//!
//! Tasks sharing a `context_id` must continue the same chat session instead
//! of forking a fresh one per task. The mapping self-heals: a session that
//! was deleted or archived after the mapping was written yields no lookup
//! result, so a fresh session gets created and re-bound.

use crate::a2a::persistence;
use crate::db::Database;
use rusqlite::params;
use uuid::Uuid;

async fn test_db() -> crate::db::Pool {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    db.pool().clone()
}

/// Insert a sessions row directly with the given id. Uses the post-`modernize_schema`
/// column set: `provider` and `is_archived` no longer exist (`archived_at` replaced both).
async fn seed_session(pool: &crate::db::Pool, id: &str, archived: bool) {
    let sid = id.to_string();
    let archived_at = if archived { Some(1_i64) } else { None };
    pool.get()
        .await
        .unwrap()
        .interact(move |conn| {
            conn.execute(
                "INSERT INTO sessions (id, title, model, created_at, updated_at, archived_at)
                 VALUES (?1, 't', 'm', 0, 0, ?2)",
                params![sid, archived_at],
            )
        })
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn context_lookup_returns_live_session() {
    let pool = test_db().await;
    let live_id = Uuid::new_v4();
    seed_session(&pool, &live_id.to_string(), false).await;

    assert_eq!(
        persistence::lookup_session_for_context(&pool, "ctx-a").await,
        None
    );

    persistence::save_context_session(&pool, "ctx-a", live_id).await;
    assert_eq!(
        persistence::lookup_session_for_context(&pool, "ctx-a").await,
        Some(live_id)
    );
}

#[tokio::test]
async fn context_lookup_self_heals_deleted_session() {
    let pool = test_db().await;
    // Map to a session that does not exist at all.
    persistence::save_context_session(&pool, "ctx-b", Uuid::new_v4()).await;
    assert_eq!(
        persistence::lookup_session_for_context(&pool, "ctx-b").await,
        None
    );
}

#[tokio::test]
async fn context_lookup_ignores_archived_session() {
    let pool = test_db().await;
    let archived_id = Uuid::new_v4();
    seed_session(&pool, &archived_id.to_string(), true).await;

    persistence::save_context_session(&pool, "ctx-c", archived_id).await;
    assert_eq!(
        persistence::lookup_session_for_context(&pool, "ctx-c").await,
        None
    );
}

#[tokio::test]
async fn context_rebind_upserts_existing_mapping() {
    let pool = test_db().await;
    let old_id = Uuid::new_v4();
    let new_id = Uuid::new_v4();
    seed_session(&pool, &old_id.to_string(), false).await;
    seed_session(&pool, &new_id.to_string(), false).await;

    persistence::save_context_session(&pool, "ctx-d", old_id).await;
    persistence::save_context_session(&pool, "ctx-d", new_id).await;
    assert_eq!(
        persistence::lookup_session_for_context(&pool, "ctx-d").await,
        Some(new_id)
    );
}
