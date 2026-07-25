//! Background tasks survive a restart as reported interruptions (#763).
//!
//! A detached command used to live only in an in-memory map, so killing the
//! process dropped both the child and the record of it and the session waited
//! forever on a resume that could never arrive.

use crate::db::{BackgroundTaskRepository, Database};
use uuid::Uuid;

async fn repo() -> BackgroundTaskRepository {
    let db = Database::connect_in_memory()
        .await
        .expect("in-memory db connects");
    db.run_migrations().await.expect("migrations run");
    BackgroundTaskRepository::new(db.pool().clone())
}

#[tokio::test]
async fn a_recorded_task_survives_to_be_read_back() {
    let repo = repo().await;
    let (id, session) = (Uuid::new_v4(), Uuid::new_v4());
    repo.record(
        id,
        session,
        "cargo test",
        "cargo test --all-features",
        "/tmp",
    )
    .await
    .expect("record");

    let rows = repo.all().await.expect("read back");
    assert_eq!(rows.len(), 1, "the row is what a restart finds");
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].session_id, session, "resume targets this session");
    assert_eq!(rows[0].label, "cargo test");
    assert_eq!(
        rows[0].command, "cargo test --all-features",
        "the full command is kept so the agent can re-run it"
    );
}

#[tokio::test]
async fn a_task_that_finished_leaves_nothing_behind() {
    // Otherwise the next startup reports a phantom interruption for a command
    // that actually completed.
    let repo = repo().await;
    let (id, session) = (Uuid::new_v4(), Uuid::new_v4());
    repo.record(id, session, "cargo build", "cargo build", "/tmp")
        .await
        .expect("record");
    repo.clear(id).await.expect("clear");

    assert!(repo.all().await.expect("read back").is_empty());
}

#[tokio::test]
async fn clearing_one_task_leaves_its_siblings_running() {
    // Two identical commands can be in flight at once, which is why rows are
    // keyed by id rather than matched by label.
    let repo = repo().await;
    let session = Uuid::new_v4();
    let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
    repo.record(first, session, "cargo test", "cargo test", "/tmp")
        .await
        .expect("record");
    repo.record(second, session, "cargo test", "cargo test", "/tmp")
        .await
        .expect("record");

    repo.clear(first).await.expect("clear");

    let rows = repo.all().await.expect("read back");
    assert_eq!(rows.len(), 1, "only the finished one is gone");
    assert_eq!(rows[0].id, second);
}

#[tokio::test]
async fn tasks_from_several_sessions_are_all_reported() {
    // Startup accounts for every session's orphans, not just the foreground one.
    let repo = repo().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    repo.record(Uuid::new_v4(), a, "cargo test", "cargo test", "/tmp")
        .await
        .expect("record");
    repo.record(Uuid::new_v4(), b, "npm test", "npm test", "/tmp")
        .await
        .expect("record");

    let rows = repo.all().await.expect("read back");
    assert_eq!(rows.len(), 2);
    let sessions: Vec<Uuid> = rows.iter().map(|r| r.session_id).collect();
    assert!(sessions.contains(&a) && sessions.contains(&b));
}

#[tokio::test]
async fn clear_all_empties_the_table_after_reporting() {
    let repo = repo().await;
    let session = Uuid::new_v4();
    repo.record(Uuid::new_v4(), session, "cargo test", "cargo test", "/tmp")
        .await
        .expect("record");
    repo.clear_all().await.expect("clear all");
    assert!(
        repo.all().await.expect("read back").is_empty(),
        "leaving rows behind would re-report the same interruption forever"
    );
}
