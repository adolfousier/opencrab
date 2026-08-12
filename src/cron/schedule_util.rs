//! Timezone-aware cron schedule helpers, shared by the scheduler (when a
//! job is due / its next run) and the `cron_manage` tool + CLI (validating
//! a timezone and showing the user the upcoming run times).
//!
//! Two things every caller must know about our cron format:
//!   * The user/agent writes a **5-field** expression (`min hour dom mon
//!     dow`); we prepend `"0 "` for the seconds field the `cron` crate
//!     wants. That prefix is also why `@daily`/`@hourly` macros don't work
//!     through us — `"0 @daily"` is not a valid expression.
//!   * Day-of-week is **1-7 = Sun-Sat** in the `cron` crate (1=Sunday,
//!     7=Saturday; `0` is rejected). Day and month *names* (`Sun`-`Sat`,
//!     `Mon-Fri`, `Jan-Mar`) are accepted and unambiguous.
//!
//! Schedules are interpreted in the job's timezone's **wall clock** and
//! converted back to UTC for storage/comparison, so "09:00 America/New_York"
//! fires at the right instant year-round (DST-aware).

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use std::str::FromStr;

/// Parse a job timezone string (e.g. `"America/New_York"`, `"UTC"`) into a
/// [`Tz`]. Returns `None` for an unknown zone so callers can reject it at
/// creation rather than silently falling back and surprising the user.
pub fn parse_timezone(tz: &str) -> Option<Tz> {
    tz.parse::<Tz>().ok()
}

/// Rewrite Unix day-of-week `0` (Sunday) as `7`, which is what the `cron`
/// crate accepts (#1024).
///
/// Unix cron numbers days 0-6 with Sunday at 0; the `cron` crate numbers them
/// 1-7 with Sunday at 7. An expression written the way every crontab on earth
/// writes it — `0 4 * * 0` for Sunday at 04:00 — therefore fails to parse
/// entirely. The built-in dedup-scan job shipped with exactly that expression
/// and never ran once, logging a parse failure every minute instead.
///
/// Only whole tokens are rewritten, so `10` in a minute field is untouched and
/// step/range syntax (`0-3`, `*/2`, `0,3`) keeps working.
pub(crate) fn normalize_day_of_week(field: &str) -> String {
    field
        .split(',')
        .map(|part| {
            part.split('/')
                .map(|step| {
                    step.split('-')
                        .map(|tok| if tok.trim() == "0" { "7" } else { tok })
                        .collect::<Vec<_>>()
                        .join("-")
                })
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Parse a 5-field cron expression, prepending the seconds field the `cron`
/// crate requires. `None` if the expression is malformed.
fn parse_schedule(cron_expr: &str) -> Option<Schedule> {
    let fields: Vec<&str> = cron_expr.split_whitespace().collect();
    let normalized = if fields.len() == 5 {
        let mut f = fields.clone();
        let dow = normalize_day_of_week(fields[4]);
        f[4] = dow.as_str();
        f.join(" ")
    } else {
        cron_expr.to_string()
    };
    Schedule::from_str(&format!("0 {normalized}")).ok()
}

/// The next `n` fire times for a 5-field cron expression, interpreted in
/// `tz`'s wall clock (DST-aware), returned as tz-local datetimes for
/// display. Empty if the expression is malformed or has no future runs.
pub fn upcoming_in_tz(
    cron_expr: &str,
    tz: Tz,
    n: usize,
    after: DateTime<Utc>,
) -> Vec<DateTime<Tz>> {
    match parse_schedule(cron_expr) {
        Some(s) => s.after(&after.with_timezone(&tz)).take(n).collect(),
        None => Vec::new(),
    }
}

/// The next fire time as a UTC instant: the schedule's next wall-clock match
/// in `tz`, converted back to UTC. This is what the scheduler stores in
/// `next_run_at` and compares against `Utc::now()`.
pub fn next_run_utc(cron_expr: &str, tz: Tz, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    parse_schedule(cron_expr)
        .and_then(|s| s.after(&after.with_timezone(&tz)).next())
        .map(|dt| dt.with_timezone(&Utc))
}

/// Render the next `n` run times as indented `- <when>` lines in the job's
/// timezone, for the confirmation message the tool/CLI shows after creating
/// a job. The agent (and user) read this back to verify the schedule means
/// what they intended — the cheapest guard against the day-of-week footgun.
/// Falls back to a clear notice if no runs could be computed.
pub fn format_upcoming(cron_expr: &str, tz: Tz, n: usize, after: DateTime<Utc>) -> String {
    let runs = upcoming_in_tz(cron_expr, tz, n, after);
    if runs.is_empty() {
        return "    (could not compute upcoming runs — re-check the expression)".to_string();
    }
    runs.iter()
        .map(|dt| format!("    - {}", dt.format("%A %Y-%m-%d %H:%M %Z")))
        .collect::<Vec<_>>()
        .join("\n")
}
