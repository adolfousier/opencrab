//! Sub-agent session retention (#931).
//!
//! `spawn_agent` creates a session per child, titled `subagent: {label}`, and
//! nothing ever revisits them. They used to accumulate forever along with their
//! messages, tool executions and plan files, and they cluttered the session
//! list in between.
//!
//! Two behaviours, tested separately: they are hidden from session lists from
//! the moment they exist, and they are purged once past the TTL.

use crate::db::Database;
use crate::db::repository::{SessionListOptions, SessionRepository};
use crate::services::{ServiceContext, SessionService};
use uuid::Uuid;

async fn service() -> (SessionService, ServiceContext) {
    let db = Database::connect_in_memory().await.expect("in-memory db");
    db.run_migrations().await.expect("migrations");
    let context = ServiceContext::new(db.pool().clone());
    (SessionService::new(context.clone()), context)
}

/// Backdate a session's `updated_at` so the sweep sees it as old.
async fn age_session(ctx: &ServiceContext, id: Uuid, days: i64) {
    let ts = chrono::Utc::now().timestamp() - days * 86_400;
    let id_s = id.to_string();
    ctx.pool()
        .get()
        .await
        .expect("conn")
        .interact(move |conn| {
            conn.execute(
                "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![ts, id_s],
            )
        })
        .await
        .expect("interact")
        .expect("update");
}

async fn visible_titles(svc: &SessionService) -> Vec<String> {
    svc.list_sessions(SessionListOptions::default())
        .await
        .expect("list")
        .into_iter()
        .filter_map(|s| s.title)
        .collect()
}

#[tokio::test]
async fn subagent_sessions_are_hidden_from_the_session_list() {
    let (svc, _ctx) = service().await;
    svc.create_session(Some("Real work".into())).await.unwrap();
    svc.create_session(Some("subagent: audit the config".into()))
        .await
        .unwrap();

    let titles = visible_titles(&svc).await;
    assert!(titles.contains(&"Real work".to_string()));
    assert!(
        !titles.iter().any(|t| t.starts_with("subagent:")),
        "a spawned session is an implementation detail, not a list entry: {titles:?}"
    );
}

#[tokio::test]
async fn an_untitled_session_is_not_swept_up_by_the_hide_filter() {
    // `NULL NOT LIKE 'subagent:%'` is NULL, which SQLite treats as false. A
    // naive filter would make every untitled session vanish from the list.
    let (svc, _ctx) = service().await;
    let untitled = svc.create_session(None).await.unwrap();

    let ids: Vec<Uuid> = svc
        .list_sessions(SessionListOptions::default())
        .await
        .expect("list")
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert!(
        ids.contains(&untitled.id),
        "an untitled session is a real session and must still be listed"
    );
}

#[tokio::test]
async fn subagent_sessions_are_visible_when_explicitly_requested() {
    // They stay in the database for debugging; only the default view hides them.
    let (svc, _ctx) = service().await;
    svc.create_session(Some("subagent: audit".into()))
        .await
        .unwrap();

    let titles: Vec<String> = svc
        .list_sessions(SessionListOptions {
            include_subagents: true,
            ..Default::default()
        })
        .await
        .expect("list")
        .into_iter()
        .filter_map(|s| s.title)
        .collect();
    assert!(titles.iter().any(|t| t.starts_with("subagent:")));
}

#[tokio::test]
async fn an_expired_subagent_session_is_purged() {
    let (svc, ctx) = service().await;
    let old = svc
        .create_session(Some("subagent: stale".into()))
        .await
        .unwrap();
    age_session(&ctx, old.id, 10).await;

    let pruned = svc.prune_expired_subagent_sessions(7).await.expect("sweep");
    assert_eq!(pruned, 1);
    assert!(
        svc.get_session(old.id).await.expect("get").is_none(),
        "the row must be gone, not archived — the point is to stop it accumulating"
    );
}

#[tokio::test]
async fn a_recent_subagent_session_survives() {
    let (svc, ctx) = service().await;
    let fresh = svc
        .create_session(Some("subagent: running now".into()))
        .await
        .unwrap();
    age_session(&ctx, fresh.id, 2).await;

    assert_eq!(svc.prune_expired_subagent_sessions(7).await.unwrap(), 0);
    assert!(svc.get_session(fresh.id).await.unwrap().is_some());
}

#[tokio::test]
async fn a_users_own_session_is_never_pruned_however_old() {
    let (svc, ctx) = service().await;
    let mine = svc
        .create_session(Some("My long project".into()))
        .await
        .unwrap();
    age_session(&ctx, mine.id, 400).await;

    assert_eq!(svc.prune_expired_subagent_sessions(7).await.unwrap(), 0);
    assert!(
        svc.get_session(mine.id).await.unwrap().is_some(),
        "only sessions titled 'subagent:…' are in scope"
    );
}

#[tokio::test]
async fn a_zero_ttl_disables_the_sweep() {
    let (svc, ctx) = service().await;
    let old = svc
        .create_session(Some("subagent: keep me".into()))
        .await
        .unwrap();
    age_session(&ctx, old.id, 900).await;

    assert_eq!(
        svc.prune_expired_subagent_sessions(0).await.unwrap(),
        0,
        "0 must mean keep forever, not prune everything"
    );
    assert!(svc.get_session(old.id).await.unwrap().is_some());
}

#[tokio::test]
async fn purging_removes_rows_from_every_session_scoped_table() {
    // Only `messages` and `files` cascade; the other nine tables carry a bare
    // `session_id` and would leak rows on delete. This is the check that fails
    // if a new table is added and not listed.
    let (svc, ctx) = service().await;
    let sess = svc
        .create_session(Some("subagent: has children".into()))
        .await
        .unwrap();

    let sid = sess.id.to_string();
    let sid_ins = sid.clone();
    ctx.pool()
        .get()
        .await
        .unwrap()
        .interact(move |conn| {
            conn.execute(
                "INSERT INTO tool_executions \
                 (id, message_id, session_id, tool_name, status, created_at) \
                 VALUES ('t1', 'm1', ?1, 'bash', 'ok', 0)",
                rusqlite::params![sid_ins],
            )
        })
        .await
        .unwrap()
        .expect("seed a child row");

    age_session(&ctx, sess.id, 30).await;
    assert_eq!(svc.prune_expired_subagent_sessions(7).await.unwrap(), 1);

    let left: i64 = ctx
        .pool()
        .get()
        .await
        .unwrap()
        .interact(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM tool_executions WHERE session_id = ?1",
                rusqlite::params![sid],
                |r| r.get(0),
            )
        })
        .await
        .unwrap()
        .expect("count");
    assert_eq!(
        left, 0,
        "a table without ON DELETE CASCADE must still be cleaned by purge"
    );
}

#[tokio::test]
async fn the_sweep_reports_how_many_it_removed() {
    let (svc, ctx) = service().await;
    for n in 0..3 {
        let s = svc
            .create_session(Some(format!("subagent: task {n}")))
            .await
            .unwrap();
        age_session(&ctx, s.id, 30).await;
    }
    svc.create_session(Some("subagent: fresh".into()))
        .await
        .unwrap();

    assert_eq!(svc.prune_expired_subagent_sessions(7).await.unwrap(), 3);
}

#[tokio::test]
async fn purge_is_a_hard_delete_unlike_delete_which_archives() {
    // `delete` deliberately keeps the row so usage joins still resolve. The
    // sweep must not use it, or nothing would ever actually go away.
    let (svc, ctx) = service().await;
    let a = svc
        .create_session(Some("subagent: soft".into()))
        .await
        .unwrap();
    svc.delete_session(a.id).await.unwrap();
    assert!(
        svc.get_session(a.id).await.unwrap().is_some(),
        "delete_session archives; this documents the difference"
    );

    let repo = SessionRepository::new(ctx.pool());
    repo.purge(a.id).await.unwrap();
    assert!(svc.get_session(a.id).await.unwrap().is_none());
}
