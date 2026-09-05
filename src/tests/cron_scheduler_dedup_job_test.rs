//! The weekly dedup-scan cron job is registered once and never duplicated.

use crate::cron::scheduler::{DEDUP_SCAN_JOB_NAME, ensure_weekly_dedup_scan_job};
use crate::db::{CronJobRepository, Database};

/// The weekly safety-net job must seed exactly once: repeated scheduler
/// starts (process restarts, the multi-profile daemon) must never stack
/// duplicate reserved jobs (#765).
#[tokio::test]
async fn ensure_weekly_dedup_scan_job_is_idempotent() {
    let db = Database::connect_in_memory().await.expect("db connect");
    db.run_migrations().await.expect("migrations");
    let repo = CronJobRepository::new(db.pool().clone());

    ensure_weekly_dedup_scan_job(&repo).await.unwrap();
    ensure_weekly_dedup_scan_job(&repo).await.unwrap();

    let jobs = repo.list_all().await.unwrap();
    let dedup_jobs: Vec<_> = jobs
        .iter()
        .filter(|j| j.name == DEDUP_SCAN_JOB_NAME)
        .collect();
    assert_eq!(
        dedup_jobs.len(),
        1,
        "weekly dedup job must seed exactly once, never stack"
    );

    let job = dedup_jobs[0];
    assert_eq!(
        job.cron_expr, "0 4 * * 1",
        "weekly Sunday 04:00 UTC (1 = Sunday)"
    );
    assert!(job.enabled, "safety-net job must be enabled");
}
