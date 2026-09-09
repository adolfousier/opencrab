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
    // #132: contains_table now normalizes before scanning, so it DOES detect
    // the collapsed one-liner (that is the fix). The raw AST parser still
    // cannot — the collapsed form is invisible to try_parse without reflow.
    assert!(
        contains_table(collapsed),
        "post-#132: the gate sees collapsed tables via normalization"
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

// #132: a prose label GLUED to a collapsed table (no newline before the first
// pipe) must be detached onto its own line — otherwise it stays inside the
// header row, the block never parses as a table, and the label renders as a
// table cell. Exact shape of the MIIDAS cron card.
#[test]
fn glued_label_is_detached_from_collapsed_table() {
    let input = "Состояние данных: | Что | Статус | |---|---| | Последний свежий фид | 02.09 |";
    let out = reflow_collapsed_tables(input);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0], "Состояние данных:",
        "label on its own line: {out:?}"
    );
    assert_eq!(lines[1], "| Что | Статус |", "header clean: {out:?}");
    assert!(
        contains_table(&out),
        "detached block must parse as a table: {out:?}"
    );
    // Pure-table collapsed line (no label) must NOT grow an empty first line.
    let no_label = "| A | B | |---|---| | 1 | 2 |";
    let out2 = reflow_collapsed_tables(no_label);
    assert!(
        out2.starts_with("| A | B |"),
        "no phantom leading line: {out2:?}"
    );
}

// #132: normalize_tables is the single canonical entry for the rich plane —
// gate (detect.rs) and renderer (rich/api.rs) both call it, so a collapsed
// table is detected AND rendered. Regression: the MIIDAS cron-card shape
// (label-prefixed collapsed table + list items) used to slip through the rich
// gate un-reflowed and ship as raw pipes in a rich message.
mod normalize_entry {
    use crate::channels::telegram::rich::{contains_table, normalize_tables};

    #[test]
    fn miidas_shape_detected_and_expanded_by_single_entry() {
        // Exact shape from the forwarded cron card: prose label, collapsed
        // table rows jammed on one line, trailing prose after the table.
        let miidas = "Состояние данных: | Что | Статус | |---|---| | Последний свежий фид | 02.09 | | Последний отчёт сверки | 04.09 |\nЗаписал в память дня + лог.";

        // Before the fix the gate could NOT see the table (raw collapsed line).
        // The single entry must make it visible AND produce send-ready text.
        assert!(
            contains_table(miidas),
            "gate must detect the collapsed table post-normalization"
        );
        let out = normalize_tables(miidas);
        assert!(
            contains_table(&out),
            "normalized output must still parse as a table"
        );
        // Reflowed into multi-line rows...
        assert!(out.lines().count() >= 4, "rows must be split: {out}");
        // ...and blank-line separated from the leading prose (#95 law).
        let table_start = out.find("| Что | Статус |").expect("header present");
        let before = &out[..table_start];
        assert!(
            before.ends_with("\n\n"),
            "blank line must precede the table block: {out:?}"
        );
    }

    #[test]
    fn normalize_is_idempotent() {
        // Idempotency: reflow expands a collapsed table exactly once; the
        // second pass sees a well-formed multi-line table and changes nothing.
        // (Note: reflow_collapsed_tables itself is NOT fence-safe — pre-existing
        // #690 behavior, out of #132 scope; fence safety lives in the
        // ensure_blank_line pass.)
        let collapsed = "x: | A | B | |---|---| | 1 | 2 |";
        let once = normalize_tables(collapsed);
        let twice = normalize_tables(&once);
        assert_eq!(once, twice, "normalization is idempotent");
    }
}
