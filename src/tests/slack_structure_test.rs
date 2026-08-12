//! Slack renders neither markdown tables nor headings (#1016).
//!
//! A pipe table reaches the channel as its own source, in a proportional font
//! where no column lines up; one reported completion carried six of them. A
//! `##` heading renders as two literal hashes. Block Kit has no table
//! primitive, so nothing downstream can rescue either — the markdown has to
//! stop being markdown before it is converted.
//!
//! The target shape is the one the model produces when asked to format for
//! Slack directly: bold labels, `└` continuations, dividers between sections.

use crate::channels::slack::table_convert::structure_to_slack;

/// The reported shape: a multi-column table becomes label-plus-continuations.
#[test]
fn a_table_becomes_labels_and_continuations() {
    let input = "| Metric | Before | After |\n\
                 |---|---|---|\n\
                 | RestartCount | 272 | 0 |\n\
                 | Uptime | 30 min | 1h 44m |";

    let out = structure_to_slack(input);
    assert!(!out.contains('|'), "pipes survived: {out}");
    assert!(out.contains("*RestartCount*"), "missing label: {out}");
    assert!(out.contains("└ Before: 272"), "missing continuation: {out}");
    assert!(out.contains("└ After: 0"), "missing continuation: {out}");
    assert!(out.contains("*Uptime*"), "second row lost: {out}");
}

/// Two columns collapse to one line — a continuation for a single value is
/// ceremony.
#[test]
fn a_two_column_table_collapses_to_one_line_per_row() {
    let input = "| Check | Result |\n|---|---|\n| Listening | 0.0.0.0:3310 |";
    let out = structure_to_slack(input);
    assert_eq!(out, "*Listening* — 0.0.0.0:3310");
}

/// Prose containing pipes is not a table and must not be mangled.
#[test]
fn prose_with_pipes_is_not_treated_as_a_table() {
    let input = "Run `ps aux | grep clamd` and check the output.";
    assert_eq!(structure_to_slack(input), input);
}

/// A header row with no separator beneath it is not a table.
#[test]
fn a_row_without_a_separator_is_left_alone() {
    let input = "| this looks like a row |\nbut nothing follows it";
    assert_eq!(structure_to_slack(input), input);
}

/// Someone demonstrating table syntax in a fence means it literally.
#[test]
fn a_table_inside_a_code_fence_is_untouched() {
    let input = "```\n| a | b |\n|---|---|\n| 1 | 2 |\n```";
    assert_eq!(structure_to_slack(input), input);
}

/// Headings become bold caps, since Slack has no heading syntax.
#[test]
fn headings_become_bold_caps() {
    let out = structure_to_slack("## Container state\nAll good.");
    assert!(out.starts_with("*CONTAINER STATE*"), "got: {out}");
    assert!(!out.contains('#'), "hashes survived: {out}");
}

/// Later headings get a divider; the first does not, or the message opens on
/// a horizontal line.
#[test]
fn a_divider_precedes_every_heading_but_the_first() {
    let out = structure_to_slack("# One\nbody\n\n## Two\nbody");
    assert!(!out.starts_with("---"), "message opened with a rule: {out}");
    assert_eq!(out.matches("---").count(), 1, "expected one divider: {out}");
    assert!(out.contains("*TWO*"));
}

/// A heading inside a fence is code, not structure.
#[test]
fn a_heading_inside_a_code_fence_is_untouched() {
    let input = "```\n# not a heading\n```";
    assert_eq!(structure_to_slack(input), input);
}

/// Content with neither construct is returned unchanged.
#[test]
fn ordinary_prose_is_untouched() {
    let input = "Verified live at 00:47 UTC.\n\nEvery figure read off the box.";
    assert_eq!(structure_to_slack(input), input);
}

/// A table and headings in one message both convert.
#[test]
fn a_table_under_a_heading_converts_both() {
    let input = "## What shipped\n| Commit | Status |\n|---|---|\n| a01fe4c3 | live |";
    let out = structure_to_slack(input);
    assert!(out.contains("*WHAT SHIPPED*"), "got: {out}");
    assert!(out.contains("*a01fe4c3* — live"), "got: {out}");
    assert!(!out.contains('|'), "got: {out}");
}
