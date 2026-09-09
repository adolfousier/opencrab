//! Regression for #95: a GFM table that directly abuts a text line renders as
//! raw pipes in Telegram's rich-markdown dialect (probe matrix A/B/C/D — blank
//! line before the table = rendered, abutting = raw).
//! `ensure_blank_line_before_tables` inserts the missing blank line before
//! exactly the blocks `rich/table::try_parse` accepts, never mutates code
//! fences, and is idempotent.

use crate::channels::telegram::rich::table::ensure_blank_line_before_tables;

#[test]
fn abutting_table_gets_blank_line_inserted() {
    // Probe B → probe C: the failing GREEN-report shape (bold line glued to
    // the table, no blank line between).
    let input = "**Tests** (host-diag exits, disk, docker):\n| Check | Result |\n|---|---|\n| uptime | ok |";
    let expected = "**Tests** (host-diag exits, disk, docker):\n\n| Check | Result |\n|---|---|\n| uptime | ok |";
    assert_eq!(ensure_blank_line_before_tables(input), expected);
}

#[test]
fn blank_line_before_table_is_untouched() {
    // Probe A/C shape: blank line already present — byte-identical no-op.
    let input = "Text line:\n\n| Check | Result |\n|---|---|\n| uptime | ok |";
    assert_eq!(ensure_blank_line_before_tables(input), input);
}

#[test]
fn table_inside_code_fence_is_untouched() {
    // A pipe-table look-alike inside a fence is code content, never mutated.
    let input = "```text\nheader above\n| A | B |\n|---|---|\n```\n";
    assert_eq!(ensure_blank_line_before_tables(input), input);
}

#[test]
fn pass_is_idempotent() {
    let input = "**Tests**:\n| A | B |\n|---|---|\n| 1 | 2 |";
    let once = ensure_blank_line_before_tables(input);
    assert_eq!(ensure_blank_line_before_tables(&once), once);
}

#[test]
fn pipe_free_input_is_untouched() {
    let input = "# Report\n\nAll clear, no pipes in sight.\n";
    assert_eq!(ensure_blank_line_before_tables(input), input);
}

#[test]
fn stray_pipe_prose_is_untouched() {
    // A pipe without a separator row is not a table (the try_parse gate) —
    // prose is never mutated.
    let input = "cost | value\nno separator here\n| also not a table";
    assert_eq!(ensure_blank_line_before_tables(input), input);
}
