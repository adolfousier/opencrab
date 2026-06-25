//! `/help` and `/usage` must render as proper tables with headings under the
//! Telegram rich renderer — NOT a run-together paragraph (the reported bug,
//! where single-`\n` line lists collapsed into one block). Also pins the
//! `md_table` GFM builder it relies on.

use crate::channels::commands::{format_help, md_table};
use crate::channels::telegram::rich::markdown_to_html;

#[test]
fn help_is_authored_as_heading_plus_table() {
    let help = format_help();
    assert!(help.contains("# 📖 Available Commands"), "needs a heading");
    assert!(
        help.contains("| Command | Description |"),
        "needs a markdown table header"
    );
    assert!(help.contains("| --- |"), "needs a GFM separator row");
}

#[test]
fn help_renders_commands_on_separate_lines() {
    let html = markdown_to_html(&format_help());
    // Both commands present...
    assert!(html.contains("/new"));
    assert!(html.contains("/cd"));
    // ...and crucially on DIFFERENT lines. The 2-column command/description
    // table is wide, so the renderer emits a key-value list (one per line).
    // Between two consecutive commands there must be a newline, never a bare
    // space — a space would mean they collapsed into one paragraph again.
    let new_pos = html.find("/new").unwrap();
    let cd_pos = html.find("/cd").unwrap();
    assert!(
        html[new_pos..cd_pos].contains('\n'),
        "commands collapsed onto one line: {:?}",
        &html[new_pos..cd_pos]
    );
    // The heading became bold (Telegram HTML has no <h1>).
    assert!(html.contains("<b>") && html.contains("Available Commands"));
}

#[test]
fn help_qualifies_for_native_rich_rendering() {
    // The table structure is what makes Telegram render it natively (real
    // bordered tables) instead of the HTML/`<pre>` fallback.
    assert!(
        crate::channels::telegram::rich::has_rich_structure(&format_help()),
        "help must contain a table/heading so it routes to native rich"
    );
}

#[test]
fn md_table_emits_gfm_with_separator() {
    let t = md_table(&["A", "B"], &[vec!["1".into(), "2".into()]]);
    assert!(t.contains("| A | B |"));
    assert!(t.contains("| --- |"));
    assert!(t.contains("| 1 | 2 |"));
    // No rows → empty (caller skips the section).
    assert_eq!(md_table(&["A"], &[]), "");
}

#[test]
fn md_table_neutralizes_pipes_and_newlines_in_cells() {
    let t = md_table(&["X"], &[vec!["a|b\nc".into()]]);
    // A stray pipe/newline in a value must not spawn extra columns or rows.
    assert!(!t.contains("a|b"), "cell pipe leaked: {t:?}");
    // Exactly three lines: header, separator, one data row.
    assert_eq!(t.lines().count(), 3, "extra rows from cell newline: {t:?}");
    assert!(
        t.lines().any(|l| l.contains("a b c")),
        "cell pipe/newline should collapse to spaces: {t:?}"
    );
}
