//! Schedule data service — surfaces every cron job (enabled or paused)
//! as a uniform `Vec<McScheduleItem>` for the schedule panel.
//!
//! Pending-approval rows aren't yet wired here; they'll join when the
//! approval queue grows a global accessor (today it's session-scoped
//! state inside the agent loop). The `McScheduleKind::PendingApproval`
//! variant on the type side is ready for that data the moment it lands.
//!
//! `list` is async because the cron registry lives in the SQLite DB —
//! the renderer pre-fetches once on `actions::open` rather than calling
//! during each `draw`, so the per-frame cost is just a `Vec::clone`.

use super::types::{McScheduleItem, McScheduleKind};
use crate::db::Pool;
use crate::db::models::CronJob;
use crate::db::repository::CronJobRepository;

/// Read every cron job (enabled + paused), sorted by name. Returns an
/// empty list on DB error so a transient SQLite blip doesn't bring
/// the whole MC down.
pub async fn list(pool: Pool) -> Vec<McScheduleItem> {
    let repo = CronJobRepository::new(pool);
    let jobs = match repo.list_all().await {
        Ok(j) => j,
        Err(e) => {
            // `{e:#}` walks the full anyhow chain (top context →
            // interact_err → underlying rusqlite::Error). Without this
            // alternate flag we only see "Failed to list cron jobs"
            // and lose the actual SQL-side cause — which is exactly the
            // blind spot that hid the 2026-05-17 MC empty-schedule bug
            // until it was investigated row-by-row.
            tracing::warn!("schedule_service: failed to list cron jobs: {e:#}");
            return Vec::new();
        }
    };
    jobs.into_iter().map(item_from_cron).collect()
}

fn item_from_cron(job: CronJob) -> McScheduleItem {
    let schedule = format_cron_schedule(&job);
    McScheduleItem {
        id: job.id.to_string(),
        label: job.name,
        schedule,
        kind: McScheduleKind::Cron,
        // Disabled cron jobs stay visible so the user can re-enable
        // them from the UI later, but they're flagged as "awaiting
        // user" so the renderer can dim or badge them differently.
        awaiting_user: !job.enabled,
    }
}

/// Compose a human-friendly schedule string. Examples:
///   `0 9 * * *` (UTC)
///   `*/5 * * * *` (Europe/London) — paused, last 14:23
fn format_cron_schedule(job: &CronJob) -> String {
    let mut parts: Vec<String> = vec![job.cron_expr.clone()];
    if !job.timezone.is_empty() && job.timezone != "UTC" {
        parts.push(format!("({})", job.timezone));
    }
    if !job.enabled {
        parts.push("paused".to_string());
    } else if let Some(next) = job.next_run_at {
        parts.push(format!("next {}", next.format("%Y-%m-%d %H:%M")));
    }
    parts.join(" ")
}
