//! `ToolExecutionRepository::stats_with_failures` powers the Mission Control
//! analytics panel's fail-rate view, so its totals and error counts must be
//! exact. (`status = 'error'` is a failure; anything else is a success.)

use crate::db::Database;
use crate::db::repository::ToolExecutionRepository;

async fn setup() -> (Database, ToolExecutionRepository) {
    let db = Database::connect_in_memory().await.expect("in-memory db");
    db.run_migrations().await.expect("migrations");
    let repo = ToolExecutionRepository::new(db.pool().clone());
    (db, repo)
}

#[tokio::test]
async fn counts_totals_and_errors_per_tool_ordered_by_usage() {
    let (_db, repo) = setup().await;
    // bash: 3 calls, 1 error
    repo.record("1", "m", "s", "bash", "success", None, None, None)
        .await
        .unwrap();
    repo.record("2", "m", "s", "bash", "success", None, None, None)
        .await
        .unwrap();
    repo.record("3", "m", "s", "bash", "error", None, None, None)
        .await
        .unwrap();
    // read_file: 2 calls, 0 errors
    repo.record("4", "m", "s", "read_file", "success", None, None, None)
        .await
        .unwrap();
    repo.record("5", "m", "s", "read_file", "success", None, None, None)
        .await
        .unwrap();

    let stats = repo.stats_with_failures(None).await.unwrap();

    let bash = stats.iter().find(|s| s.tool_name == "bash").unwrap();
    assert_eq!(bash.total, 3);
    assert_eq!(bash.failures, 1);

    let rf = stats.iter().find(|s| s.tool_name == "read_file").unwrap();
    assert_eq!(rf.total, 2);
    assert_eq!(rf.failures, 0);

    // Ordered by total calls descending.
    assert_eq!(stats[0].tool_name, "bash");
}

#[tokio::test]
async fn empty_table_yields_no_rows() {
    let (_db, repo) = setup().await;
    assert!(repo.stats_with_failures(None).await.unwrap().is_empty());
}
