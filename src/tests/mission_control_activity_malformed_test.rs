//! Non-conforming journal blocks must not corrupt their neighbours (#841).
//!
//! The Activity panel showed a 12-day hole with two rows stamped "10s ago".
//! Both rows were real entries, days and weeks old, that the parser misread:
//!
//! - A block written with a bare `---` instead of a `## ` header never opened
//!   a draft, so its four fields were applied to the entry above it. Its date
//!   string did not parse, and assigning that `None` destroyed the previous
//!   entry's good date. One entry was lost outright and its neighbour was
//!   misdated.
//! - A config-sync block put the timestamp in the header and carried no
//!   `**Date:**` field, so it fell back to `Utc::now()` and its title rendered
//!   as the raw date string.
//!
//! Fixtures reproduce those shapes with synthetic content and carry no user
//! identifiers.

use crate::brain::mission_control::activity_service::parse_improvements_md;

/// A headed entry followed by a headerless block, as the journal wrote it.
const HEADERLESS_NEIGHBOUR: &str = "\
## [Updated] Add a rule about confirmed data

**Date:** 2026-07-17 12:16 UTC
**Target:** SOUL.md
**Rationale:** First entry rationale.
**Status:** Updated (surgical replace)

---

**Date:** 2026-07-18 (RSI cycle)
**Target:** SOUL.md
**Rationale:** Second entry rationale.
**Status:** Applied
";

#[test]
fn a_headerless_block_becomes_its_own_entry() {
    let parsed = parse_improvements_md(HEADERLESS_NEIGHBOUR, 50);
    assert_eq!(parsed.len(), 2, "the headerless block was swallowed");
}

#[test]
fn a_headerless_block_does_not_steal_its_neighbours_date() {
    // The defect: the second block's unparseable date overwrote the first
    // block's correct one, and the first then rendered as "now".
    let parsed = parse_improvements_md(HEADERLESS_NEIGHBOUR, 50);
    let headed = parsed
        .iter()
        .find(|e| e.detail.contains("confirmed data"))
        .expect("the headed entry survives");
    assert_eq!(
        headed
            .timestamp
            .expect("its own date is intact")
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        "2026-07-17 12:16"
    );
}

#[test]
fn a_headerless_block_keeps_its_own_fields() {
    let parsed = parse_improvements_md(HEADERLESS_NEIGHBOUR, 50);
    let headed = parsed
        .iter()
        .find(|e| e.detail.contains("confirmed data"))
        .expect("the headed entry survives");
    // Target and rationale were overwritten too, not just the date.
    assert!(
        headed.detail.contains("First entry rationale"),
        "neighbour's rationale leaked in: {}",
        headed.detail
    );
    assert!(
        parsed
            .iter()
            .any(|e| e.detail.contains("Second entry rationale")),
        "the headerless block's own rationale was lost"
    );
}

#[test]
fn an_unparseable_date_leaves_the_entry_undated() {
    // Never "now": that is what made an old entry look like it happened
    // during the render.
    let raw = "## [Applied] x\n\n**Date:** 2026-07-18 (RSI cycle)\n**Status:** Applied\n";
    let parsed = parse_improvements_md(raw, 50);
    assert_eq!(parsed.len(), 1);
    assert!(parsed[0].timestamp.is_none());
}

#[test]
fn a_date_in_the_header_is_read() {
    // The config-sync writer's shape: timestamp in the header, no field.
    let raw = "## 2026-07-26 23:33 UTC - config.toml config sync\n\nAdded 20 key(s).\n";
    let parsed = parse_improvements_md(raw, 50);
    assert_eq!(parsed.len(), 1);
    assert_eq!(
        parsed[0]
            .timestamp
            .expect("header date is read")
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        "2026-07-26 23:33"
    );
}

#[test]
fn a_header_date_is_not_left_in_the_title() {
    let raw = "## 2026-07-26 23:33 UTC - config.toml config sync\n";
    let parsed = parse_improvements_md(raw, 50);
    assert_eq!(parsed[0].detail, "config.toml config sync");
}

#[test]
fn every_dash_the_journal_uses_as_a_separator_is_handled() {
    for sep in ["-", "\u{2013}", "\u{2014}", ":"] {
        let raw = format!("## 2026-07-26 23:33 UTC {sep} config sync\n");
        let parsed = parse_improvements_md(&raw, 50);
        assert_eq!(parsed[0].detail, "config sync", "separator {sep:?}");
    }
}

#[test]
fn an_ordinary_title_is_not_mistaken_for_a_date() {
    // Three tokens where the third is not UTC must stay a title.
    let raw = "## [Applied] 2026 budget review\n\n**Date:** 2026-04-12 23:01 UTC\n";
    let parsed = parse_improvements_md(raw, 50);
    assert!(parsed[0].detail.contains("2026 budget review"));
    assert_eq!(
        parsed[0]
            .timestamp
            .expect("field date wins")
            .format("%Y-%m-%d")
            .to_string(),
        "2026-04-12"
    );
}

#[test]
fn a_normal_run_of_entries_is_unaffected() {
    // The guard keys off a repeated `**Date:**`, so well-formed entries must
    // parse exactly as before.
    let raw = "\
## [Applied] first

**Date:** 2026-04-10 10:00 UTC
**Status:** Applied

## [Applied] second

**Date:** 2026-04-11 11:00 UTC
**Status:** Applied

## [Applied] third

**Date:** 2026-04-12 12:00 UTC
**Status:** Applied
";
    let parsed = parse_improvements_md(raw, 50);
    assert_eq!(parsed.len(), 3);
    // Newest first, and every date its own.
    assert!(parsed[0].detail.contains("third"));
    assert!(parsed[2].detail.contains("first"));
    for entry in &parsed {
        assert!(entry.timestamp.is_some(), "lost a date: {}", entry.detail);
    }
}
