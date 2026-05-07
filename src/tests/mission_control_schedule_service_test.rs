//! Tests for the schedule service — verifies cron jobs surface
//! correctly into `McScheduleItem`s, including the `awaiting_user` flag
//! flipping when a job is paused.

use crate::brain::mission_control::{McScheduleKind, schedule_service};
use crate::db::repository::CronJobRepository;
use crate::db::{Database, models::CronJob};

async fn setup() -> (Database, CronJobRepository) {
    let db = Database::connect_in_memory()
        .await
        .expect("create in-memory db");
    db.run_migrations().await.expect("run migrations");
    let repo = CronJobRepository::new(db.pool().clone());
    (db, repo)
}

fn make_job(name: &str, cron: &str, timezone: &str, enabled: bool) -> CronJob {
    let mut job = CronJob::new(
        name.to_string(),
        cron.to_string(),
        timezone.to_string(),
        "Test prompt".to_string(),
        None,
        None,
        "off".to_string(),
        true,
        None,
        None,
    );
    job.enabled = enabled;
    job
}

#[tokio::test]
async fn empty_db_returns_empty_list() {
    let (db, _repo) = setup().await;
    let items = schedule_service::list(db.pool().clone()).await;
    assert!(items.is_empty());
}

#[tokio::test]
async fn enabled_cron_job_renders_as_cron_item_not_awaiting_user() {
    let (db, repo) = setup().await;
    let job = make_job("morning_brief", "0 9 * * *", "UTC", true);
    repo.insert(&job).await.unwrap();
    let items = schedule_service::list(db.pool().clone()).await;
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.label, "morning_brief");
    assert_eq!(item.kind, McScheduleKind::Cron);
    assert!(item.schedule.contains("0 9 * * *"));
    assert!(
        !item.awaiting_user,
        "enabled jobs should not flag awaiting_user"
    );
}

#[tokio::test]
async fn paused_cron_job_flags_awaiting_user_and_marks_paused_in_schedule() {
    let (db, repo) = setup().await;
    let job = make_job("weekly_report", "0 17 * * 5", "UTC", false);
    repo.insert(&job).await.unwrap();
    let items = schedule_service::list(db.pool().clone()).await;
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert!(item.awaiting_user, "paused job should flag awaiting_user");
    assert!(
        item.schedule.contains("paused"),
        "schedule string should mention paused: {:?}",
        item.schedule
    );
}

#[tokio::test]
async fn non_utc_timezone_appears_in_schedule_string() {
    let (db, repo) = setup().await;
    let job = make_job("london_brief", "0 9 * * *", "Europe/London", true);
    repo.insert(&job).await.unwrap();
    let items = schedule_service::list(db.pool().clone()).await;
    assert!(
        items[0].schedule.contains("Europe/London"),
        "non-UTC timezone should be visible: {:?}",
        items[0].schedule
    );
}

#[tokio::test]
async fn multiple_jobs_all_surface() {
    let (db, repo) = setup().await;
    repo.insert(&make_job("a_job", "0 9 * * *", "UTC", true))
        .await
        .unwrap();
    repo.insert(&make_job("b_job", "*/5 * * * *", "UTC", true))
        .await
        .unwrap();
    repo.insert(&make_job("c_job", "0 0 * * *", "UTC", false))
        .await
        .unwrap();
    let items = schedule_service::list(db.pool().clone()).await;
    assert_eq!(items.len(), 3);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"a_job"));
    assert!(labels.contains(&"b_job"));
    assert!(labels.contains(&"c_job"));
}
