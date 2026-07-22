//! Regression for #690: re-expand a table the model collapsed onto one line so
//! it renders instead of showing raw pipes.
//!
//! table::try_parse needs the header, `|---|` separator, and each row on its OWN
//! line. A weaker model emits the whole table on one line; reflow_collapsed_tables
//! breaks it back into rows using the empty-gap (`| |`) boundaries, gated on the
//! line carrying BOTH a dash-only separator cell and content cells (a state a
//! well-formed multi-line table never has on one line).

use crate::channels::telegram::rich::{contains_table, reflow_collapsed_tables};

#[test]
fn collapsed_two_column_table_expands_and_parses() {
    let collapsed =
        "| Category | Cost | |----------|------| | Pricing | $0.17 | | Web Search | $0.0005 |";
    assert!(
        !contains_table(collapsed),
        "precondition: the collapsed one-liner is not parseable as a table"
    );
    let out = reflow_collapsed_tables(collapsed);
    assert_eq!(
        out,
        "| Category | Cost |\n|----------|------|\n| Pricing | $0.17 |\n| Web Search | $0.0005 |"
    );
    assert!(
        contains_table(&out),
        "after reflow the table must be detectable"
    );
}

#[test]
fn label_prefixed_collapsed_table_expands() {
    // The exact screenshot shape: a bold label glued to the header, rows collapsed.
    let collapsed = "**Pricing:** | Category | Cost | |----------|------| | Row | 1 |";
    let out = reflow_collapsed_tables(collapsed);
    assert!(
        contains_table(&out),
        "reflowed prefixed table must parse: {out}"
    );
    // Each row ends up on its own line.
    assert!(out.lines().count() >= 3, "rows must be split: {out}");
}

#[test]
fn already_multiline_table_is_unchanged() {
    let good = "| A | B |\n|---|---|\n| 1 | 2 |";
    assert_eq!(
        reflow_collapsed_tables(good),
        good,
        "a proper table is idempotent"
    );
}

#[test]
fn lone_separator_line_is_untouched() {
    // A real separator line, alone, must not be treated as collapsed.
    let sep = "|----------|------|";
    assert_eq!(reflow_collapsed_tables(sep), sep);
}

#[test]
fn prose_with_pipes_is_untouched() {
    for prose in [
        "Pick option A | B | C for the build",
        "run `cmd -a | grep x` to filter",
        "no pipes here at all",
        "a range like 10---20 in text",
    ] {
        assert_eq!(
            reflow_collapsed_tables(prose),
            prose,
            "prose without a separator+content mix must be untouched: {prose}"
        );
    }
}

#[test]
fn non_table_lines_around_a_collapsed_table_survive() {
    let input = "Here are the prices:\n| A | B | |---|---| | 1 | 2 |\nThat is all.";
    let out = reflow_collapsed_tables(input);
    assert!(out.starts_with("Here are the prices:\n"));
    assert!(out.ends_with("\nThat is all."));
    assert!(contains_table(&out));
}
