//! Compaction signal (#29): unit tests for the flow-line builders, the
//! pinned-header const, the ETA predictor, and the footer-dedupe contract.
//! All pure — no agent, no mocks, no locks.

use std::time::Duration;

use crate::brain::agent::service::nudge::{in_pressure_warning_band, should_emit_pressure_warning};
use crate::channels::telegram::flow::{
    COMPACTING_HEADER_TEXT, FlowHeader, FlowLine, compacted_flow_line, compacting_flow_line,
    render_flow_html_chrome_pref, render_flow_rich,
};
use crate::channels::telegram::flow_chrome::FlowSections;

#[test]
fn compacting_line_without_prediction() {
    // First compaction of a session: no observed history, no parenthetical.
    // The retired hardcoded `≈10–60s` window is gone — it never was right
    // (live range observed 10s → 29 min).
    assert_eq!(
        compacting_flow_line(68.0, None),
        "⏳ Compacting context — 68% full…"
    );
}

#[test]
fn compacting_line_shows_observed_eta() {
    assert_eq!(
        compacting_flow_line(68.0, Some(Duration::from_secs(42))),
        "⏳ Compacting context — 68% full (≈42s)…"
    );
}

#[test]
fn compacting_line_humanizes_minute_eta() {
    // ≥60s rides the shared humanize_duration formatting ("2 min 12s"),
    // same as the settled ✅ line.
    assert_eq!(
        compacting_flow_line(71.0, Some(Duration::from_secs(132))),
        "⏳ Compacting context — 71% full (≈2 min 12s)…"
    );
}

#[test]
fn compacting_line_rounds_fill_level() {
    // {:.0} rounding — the line carries a whole-number level, never a
    // fractional one.
    assert!(compacting_flow_line(67.7, None).contains("68% full"));
    assert!(compacting_flow_line(99.4, None).contains("99% full"));
}

#[test]
fn compacting_footer_suppresses_duplicate_activity_segment() {
    // Dedupe (#29): during the silent window the newest log line IS the ⏳
    // body entry, so the activity preview renders the compaction string a
    // second time next to the pinned header — the owner-sighted duplication.
    // While compacting the flag suppresses the activity segment: exactly ONE
    // compaction string. With the flag off the duplicate returns (documents
    // the suppression contract).
    let lines = [FlowLine::Text(
        "⏳ Compacting context — 66% full…".to_string(),
    )];
    let compacting = render_flow_html_chrome_pref(
        &lines,
        &FlowHeader::Live(Some(COMPACTING_HEADER_TEXT)),
        None,
        &FlowSections::default(),
        usize::MAX,
        5,
        None,
        true,
    );
    assert_eq!(
        compacting.matches("Compacting context").count(),
        1,
        "pinned header is the sole compaction string while compacting: {compacting:?}"
    );
    let idle = render_flow_html_chrome_pref(
        &lines,
        &FlowHeader::Live(Some("⚙️")),
        None,
        &FlowSections::default(),
        usize::MAX,
        5,
        None,
        false,
    );
    assert_eq!(
        idle.matches("Compacting context").count(),
        2,
        "without the flag the activity preview duplicates the body entry: {idle:?}"
    );
}

#[test]
fn compacting_rich_footer_suppresses_duplicate_status() {
    // Same dedupe contract on the rich-markdown path: status_msg suppressed
    // while the flag is set.
    let lines = [FlowLine::Text(
        "⏳ Compacting context — 66% full…".to_string(),
    )];
    let rich = render_flow_rich(&lines, Some(COMPACTING_HEADER_TEXT), true);
    assert_eq!(
        rich.matches("Compacting context").count(),
        1,
        "rich footer dedupes too: {rich:?}"
    );
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
