//! Compaction signal (#29): unit tests for the flow-line builders, the
//! pinned-header const, and the #909 pressure band that drives the ❕.
//! All pure — no agent, no mocks, no locks.

use std::time::Duration;

use crate::brain::agent::service::nudge::{in_pressure_warning_band, should_emit_pressure_warning};
use crate::channels::telegram::flow::{
    COMPACTING_HEADER_TEXT, compacted_flow_line, compacting_flow_line,
};

#[test]
fn compacting_line_shows_fill_level_and_window() {
    assert_eq!(
        compacting_flow_line(68.0),
        "⏳ Compacting context — 68% full (≈10–60s)…"
    );
}

#[test]
fn compacting_line_rounds_fill_level() {
    // {:.0} rounding — the line carries a whole-number level, never a
    // fractional one.
    assert!(compacting_flow_line(67.7).contains("68% full"));
    assert!(compacting_flow_line(99.4).contains("99% full"));
}

#[test]
fn compacted_line_under_a_minute() {
    assert_eq!(
        compacted_flow_line(68.0, 26.0, Duration::from_secs(42)),
        "✅ Compacted: 68% → 26% in 42s"
    );
}

#[test]
fn compacted_line_multi_minute() {
    assert_eq!(
        compacted_flow_line(71.0, 24.0, Duration::from_secs(132)),
        "✅ Compacted: 71% → 24% in 2 min 12s"
    );
}

#[test]
fn compacted_line_floors_subsecond_elapsed_to_1s() {
    // A sub-second summarizer call still reads as a real duration — "0s"
    // would look like the line was printed before the work happened.
    assert_eq!(
        compacted_flow_line(66.0, 30.0, Duration::from_millis(300)),
        "✅ Compacted: 66% → 30% in 1s"
    );
}

#[test]
fn header_const_is_number_free() {
    // Design invariant (#29): the pinned header NEVER carries a number —
    // compaction progress is unknowable, so any digit reads as a fake
    // progress bar. The fill level lives on the START body line instead.
    assert_eq!(COMPACTING_HEADER_TEXT, "⏳ Compacting context…");
    assert!(!COMPACTING_HEADER_TEXT.contains('%'));
    assert!(!COMPACTING_HEADER_TEXT.chars().any(|c| c.is_ascii_digit()));
}

#[test]
fn pressure_band_boundaries() {
    // [55, 65) — the ceiling is exclusive: AT 65% compaction itself fires,
    // the nudge is for the approach.
    assert!(!in_pressure_warning_band(54.9));
    assert!(in_pressure_warning_band(55.0));
    assert!(in_pressure_warning_band(64.9));
    assert!(!in_pressure_warning_band(65.0));
}

#[test]
fn pressure_warning_once_per_entry() {
    // In-band, not yet emitted → warn; already emitted → silence until the
    // flag re-arms below the floor; below the floor → never warn.
    assert!(should_emit_pressure_warning(60.0, false).is_some());
    assert!(should_emit_pressure_warning(60.0, true).is_none());
    assert!(should_emit_pressure_warning(40.0, false).is_none());
}
