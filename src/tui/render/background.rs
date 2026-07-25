//! Footer indicator for detached background commands (#762).
//!
//! A genuinely long command (`cargo test`, a build) runs detached so it does
//! not churn toward the bash 600s cap, and the turn ends immediately. That
//! left the TUI with nothing to draw: the agent says it is waiting, the
//! spinner is gone because no turn is active, and a five-minute build is
//! indistinguishable from a hang.
//!
//! Pure formatting, so the shape is testable without a terminal or a live
//! task. Elapsed seconds are passed in rather than read from a clock here.

/// `cargo test 32s`, or `cargo test 32s +2` when other tasks are also running.
///
/// The oldest task is the one named: it is the one the user has been waiting
/// on longest, and it is the one whose elapsed time answers "is this stuck".
/// `None` when nothing is running, so the caller omits the field entirely
/// rather than showing an empty slot.
pub(crate) fn format_background_tasks(tasks: &[(String, u64)]) -> Option<String> {
    let (label, secs) = tasks.first()?;
    let mut out = format!("{label} {}", humanize_elapsed(*secs));
    if tasks.len() > 1 {
        out.push_str(&format!(" +{}", tasks.len() - 1));
    }
    Some(out)
}

/// `32s` / `4m 20s` / `1h 5m`, matching the turn header's duration style so the
/// two read as the same kind of number.
fn humanize_elapsed(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => {
            let (m, r) = (s / 60, s % 60);
            if r == 0 {
                format!("{m}m")
            } else {
                format!("{m}m {r}s")
            }
        }
        s => {
            let (h, m) = (s / 3600, (s % 3600) / 60);
            if m == 0 {
                format!("{h}h")
            } else {
                format!("{h}h {m}m")
            }
        }
    }
}
