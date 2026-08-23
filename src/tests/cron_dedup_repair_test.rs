//! Regression tests for #1163: the weekly dedup-scan seeder must repair
//! installs whose DB row still holds the pre-#1024 legacy schedule
//! (`0 4 * * 0`), which the `cron` crate rejects outright. Before this fix
//! the name-idempotent early-return made such a row immortal: the job never
//! ran and the scheduler logged a parse failure every minute (1,440/day).
//!
//! The repair is surgical: ONLY the exact legacy artifact is rewritten. Any
//! other expression — including user-customized schedules — must survive
//! untouched.

use crate::cron::scheduler::{
    DEDUP_SCAN_CRON, DEDUP_SCAN_JOB_NAME, LEGACY_DEDUP_SCAN_CRON, ensure_weekly_dedup_scan_job,
};
use crate::db::models::CronJob;
use crate::db::{CronJobRepository, Database};

async fn setup() -> CronJobRepository {
    let db = Database::connect_in_memory().await.expect("in-memory db");
    db.run_migrations().await.expect("migrations");
    CronJobRepository::new(db.pool().clone())
}

fn job_with_expr(expr: &str) -> CronJob {
    CronJob::new(
        DEDUP_SCAN_JOB_NAME.to_string(),
        expr.to_string(),
        "UTC".to_string(),
        "reserved: weekly cross-file brain dedup scan (report-only)".to_string(),
        None,
        None,
        "off".to_string(),
        true,
        None,
        None,
    )
}

#[tokio::test]
async fn legacy_row_is_repaired_in_place() {
    let repo = setup().await;
    let legacy = job_with_expr(LEGACY_DEDUP_SCAN_CRON);
    repo.insert(&legacy).await.unwrap();

    ensure_weekly_dedup_scan_job(&repo).await.unwrap();

    let row = repo
        .find_by_name(DEDUP_SCAN_JOB_NAME)
        .await
        .unwrap()
        .expect("row must exist");
    assert_eq!(
        row.cron_expr, DEDUP_SCAN_CRON,
        "legacy expr must be rewritten"
    );
    assert!(
        row.next_run_at.is_none(),
        "next_run_at must be reset for recompute"
    );
    assert!(row.enabled, "repair must not disable the job");
    // The repaired schedule must actually parse and schedule (Sunday 04:00).
    let next = crate::cron::next_run_utc(DEDUP_SCAN_CRON, chrono_tz::UTC, chrono::Utc::now());
    assert!(next.is_some(), "repaired expr must be parseable");
}

#[tokio::test]
async fn customized_row_is_left_alone() {
    let repo = setup().await;
    let customized = "30 5 * * 5"; // valid, user-chosen: NOT the legacy artifact
    let job = job_with_expr(customized);
    repo.insert(&job).await.unwrap();

    ensure_weekly_dedup_scan_job(&repo).await.unwrap();

    let row = repo
        .find_by_name(DEDUP_SCAN_JOB_NAME)
        .await
        .unwrap()
        .expect("row must exist");
    assert_eq!(row.cron_expr, customized, "user customization must survive");
}

#[tokio::test]
async fn fresh_db_seeds_and_stays_idempotent() {
    let repo = setup().await;

    ensure_weekly_dedup_scan_job(&repo).await.unwrap();
    ensure_weekly_dedup_scan_job(&repo).await.unwrap();

    let all = repo.list_all().await.unwrap();
    let matches = all.iter().filter(|j| j.name == DEDUP_SCAN_JOB_NAME).count();
    assert_eq!(matches, 1, "seeding must never stack duplicates");
    let row = &all.iter().find(|j| j.name == DEDUP_SCAN_JOB_NAME).unwrap();
    assert_eq!(row.cron_expr, DEDUP_SCAN_CRON);
}
