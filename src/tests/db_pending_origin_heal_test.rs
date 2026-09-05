//! A database stamped past migration 37 without its column (#1401).
//!
//! Two branches each appended migration 37; the merge re-sorted them, and a
//! database stamped 37 by the followups-first build skipped the origin
//! ALTER. Every restart-recovery INSERT then failed for three days while
//! boot reported `interrupted=0`. The fixture below rebuilds that exact
//! state; the heal must add the column, the probe must report the broken
//! table before the heal and pass after it.

use crate::db::database::{MIGRATION_SQL, build_migrations};
use crate::db::{Database, PendingRequestRepository};

const FOLLOWUPS_SQL: &str = include_str!("../migrations/20260902000001_add_pending_followups.sql");

/// Migrations 1..=36 applied, followups applied as "37", stamped 38: the
/// origin column never ran.
async fn db_with_origin_skipped() -> Database {
    let db = Database::connect_in_memory().await.unwrap();
    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| -> Result<(), String> {
            build_migrations()
                .to_version(conn, 36)
                .map_err(|e| e.to_string())?;
            conn.execute_batch(FOLLOWUPS_SQL)
                .map_err(|e| e.to_string())?;
            conn.pragma_update(None, "user_version", MIGRATION_SQL.len() as i64)
                .map_err(|e| e.to_string())
        })
        .await
        .unwrap()
        .unwrap();
    db
}

async fn has_origin(db: &Database) -> bool {
    db.pool
        .get()
        .await
        .unwrap()
        .interact(|conn| crate::db::migration_heal::has_column(conn, "pending_requests", "origin"))
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn the_fixture_reproduces_the_skipped_column() {
    let db = db_with_origin_skipped().await;
    assert!(
        !has_origin(&db).await,
        "fixture must lack the origin column"
    );
    let err = PendingRequestRepository::new(db.pool().clone())
        .probe()
        .await
        .expect_err("the probe must fail on the broken table");
    assert!(
        format!("{err:#}").contains("cannot record a turn"),
        "probe names the consequence: {err:#}"
    );
}

#[tokio::test]
async fn run_migrations_heals_the_skipped_column() {
    let db = db_with_origin_skipped().await;
    db.run_migrations().await.unwrap();
    assert!(has_origin(&db).await, "the heal must add origin");
    PendingRequestRepository::new(db.pool().clone())
        .probe()
        .await
        .expect("recovery can record turns again");
    let version = db.get_user_version().await.unwrap();
    assert_eq!(version, Database::MIGRATION_COUNT as i64);
}

#[tokio::test]
async fn heal_is_a_no_op_on_a_correct_database_and_idempotent() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    assert!(has_origin(&db).await);
    // A second and third pass must not fail on "duplicate column name".
    db.run_migrations().await.unwrap();
    db.run_migrations().await.unwrap();
    PendingRequestRepository::new(db.pool().clone())
        .probe()
        .await
        .expect("probe passes on a correct schema");
}

#[tokio::test]
async fn probe_leaves_no_row_behind() {
    let db = Database::connect_in_memory().await.unwrap();
    db.run_migrations().await.unwrap();
    let repo = PendingRequestRepository::new(db.pool().clone());
    repo.probe().await.unwrap();
    assert!(
        repo.get_interrupted().await.unwrap().is_empty(),
        "the probe row must never survive to look like interrupted work"
    );
}

#[tokio::test]
async fn a_migrated_from_scratch_database_has_origin_at_the_right_index() {
    // Guards the fixture's premise: index 37 (1-based) is the origin ALTER.
    assert!(
        MIGRATION_SQL[36].contains("ADD COLUMN origin"),
        "migration 37 is not the origin column any more; update the heal and this fixture"
    );
}
