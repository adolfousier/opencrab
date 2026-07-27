//! Tests for the activity service's `improvements.md` parser.
//!
//! The on-disk journal is unbounded and the parser has to be tolerant
//! of partial / odd entries (`(none)` rationale, missing date, etc.)
//! without dropping records or panicking.

use crate::brain::mission_control::McActivityLevel;
use crate::brain::mission_control::activity_service::parse_improvements_md;

#[test]
fn empty_input_returns_empty_list() {
    let parsed = parse_improvements_md("", 50);
    assert!(parsed.is_empty());
}

#[test]
fn parses_one_well_formed_entry() {
    let raw = "## [Applied] Add conciseness guideline\n\
               \n\
               **Date:** 2026-04-12 23:01 UTC\n\
               **Target:** SOUL.md\n\
               **Rationale:** Users prefer shorter responses\n\
               **Status:** Applied\n";
    let parsed = parse_improvements_md(raw, 50);
    assert_eq!(parsed.len(), 1);
    let entry = &parsed[0];
    assert_eq!(entry.level, McActivityLevel::Success);
    assert_eq!(entry.source, "rsi");
    assert!(entry.detail.contains("Add conciseness guideline"));
    assert!(entry.detail.contains("SOUL.md"));
    assert!(entry.detail.contains("Users prefer shorter responses"));
    // Date parsed and sane (2026-04-12 23:01 UTC).
    assert_eq!(
        entry
            .timestamp
            .expect("date parses")
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        "2026-04-12 23:01"
    );
}

#[test]
fn newest_entries_appear_first() {
    // Two entries; oldest at top per the on-disk format. Parser flips.
    let raw = "## [Applied] First\n\
               \n\
               **Date:** 2026-04-10 09:00 UTC\n\
               **Status:** Applied\n\
               \n\
               ## [Applied] Second\n\
               \n\
               **Date:** 2026-04-11 09:00 UTC\n\
               **Status:** Applied\n";
    let parsed = parse_improvements_md(raw, 50);
    assert_eq!(parsed.len(), 2);
    assert!(
        parsed[0].detail.contains("Second"),
        "newest first; got detail: {}",
        parsed[0].detail
    );
    assert!(parsed[1].detail.contains("First"));
}

#[test]
fn limit_caps_returned_count() {
    let mut raw = String::new();
    for i in 0..10 {
        raw.push_str(&format!(
            "## [Applied] Entry {i}\n\n**Date:** 2026-04-{:02} 09:00 UTC\n**Status:** Applied\n\n",
            i + 1
        ));
    }
    let parsed = parse_improvements_md(&raw, 3);
    assert_eq!(parsed.len(), 3);
}

#[test]
fn drops_target_and_rationale_when_marked_none() {
    let raw = "## [Applied] test\n\
               \n\
               **Date:** 2026-04-12 23:01 UTC\n\
               **Target:** (none)\n\
               **Rationale:** (none)\n\
               **Status:** Applied\n";
    let parsed = parse_improvements_md(raw, 50);
    let entry = &parsed[0];
    // Detail collapses to just the title when both target and
    // rationale read "(none)".
    assert_eq!(entry.detail, "test");
}

#[test]
fn level_maps_status_field_first_then_header() {
    let applied = parse_improvements_md("## [Applied] x\n\n**Status:** Applied\n", 50);
    assert_eq!(applied[0].level, McActivityLevel::Success);

    let failed = parse_improvements_md("## [Applied] x\n\n**Status:** Failed\n", 50);
    // Status field beats header status.
    assert_eq!(failed[0].level, McActivityLevel::Error);

    let reverted = parse_improvements_md("## [Applied] x\n\n**Status:** Reverted\n", 50);
    assert_eq!(reverted[0].level, McActivityLevel::Warn);
}

#[test]
fn missing_date_does_not_drop_entry() {
    let raw = "## [Applied] dateless\n\n**Status:** Applied\n";
    let parsed = parse_improvements_md(raw, 50);
    assert_eq!(parsed.len(), 1);
    assert!(parsed[0].detail.contains("dateless"));
    // Undated, NOT "now". The old fallback stamped the render instant onto
    // entries whose date was missing, so weeks-old journal rows displayed as
    // seconds old (#841).
    assert!(parsed[0].timestamp.is_none());
}

#[test]
fn malformed_header_without_brackets_still_parses_title() {
    let raw = "## Plain title\n\n**Date:** 2026-04-12 23:01 UTC\n**Status:** Applied\n";
    let parsed = parse_improvements_md(raw, 50);
    assert_eq!(parsed.len(), 1);
    assert!(parsed[0].detail.starts_with("Plain title"));
}
