//! Cron Scheduler
//!
//! Background service that polls the `cron_jobs` table every 60 seconds and
//! executes due jobs in the user's active session. Never spawns new sessions —
//! follows the user, falls back to initial session. Results are optionally
//! delivered to a configured channel (Telegram, Discord, Slack).

mod schedule_util;
mod scheduler;

pub use schedule_util::{format_upcoming, next_run_utc, parse_timezone, upcoming_in_tz};
pub use scheduler::{CronScheduler, REBUILD_JOB_NAME, schedule_background_rebuild};
