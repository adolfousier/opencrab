//! GitHub-flavored pipe-table parsing.
//!
//! A table is a header row (`| a | b |`), a separator row of dashes with
//! optional alignment colons (`| :-- | --: |`), then zero or more body rows.
//! Parsing stops at the first line that is not a pipe row.

use super::ast::{Align, Inline, Table};
use super::inline::parse_inlines;
use regex::Regex;
use std::sync::LazyLock;

/// A row/header/separator boundary in a table the model collapsed onto one line:
/// the trailing `|` of one row meets the leading `|` of the next, giving an
/// empty gap between two pipes (`| |` or `||`). Matched non-greedily on a single
/// line (never crosses a real newline).
static COLLAPSED_ROW_BOUNDARY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\|[ \t]*\|").expect("collapsed-table boundary regex"));

/// Re-expand a table the model collapsed onto ONE line — header, separator, and
/// rows jammed together with no row-newlines — into a proper multi-line table so
/// [`try_parse`] can detect it and Telegram renders a grid instead of raw pipes
/// (#690). A line is treated as collapsed only when it carries BOTH a dash-only
/// separator cell AND ordinary content cells, a combination a well-formed
/// multi-line table never has on a single line — so prose, a lone separator row,
/// and already-expanded tables are left untouched. Idempotent.
pub(crate) fn reflow_collapsed_tables(text: &str) -> String {
    if !text.contains('|') {
        return text.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if is_collapsed_table_line(line) {
            out.push(
                COLLAPSED_ROW_BOUNDARY
                    .replace_all(line, "|\n|")
                    .into_owned(),
            );
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

/// Whether a single line is a collapsed table: it has a dash-only separator cell
/// (`----`) AND at least one content cell on the same line. A proper multi-line
/// table has the separator alone on its own line, so this never fires on one.
fn is_collapsed_table_line(line: &str) -> bool {
    let mut has_dash = false;
    let mut has_content = false;
    for cell in line.split('|') {
        let c = cell.trim();
        if c.is_empty() {
            continue;
        }
        let core = c.trim_start_matches(':').trim_end_matches(':');
        if core.len() >= 2 && core.chars().all(|ch| ch == '-') {
            has_dash = true;
        } else {
            has_content = true;
        }
        if has_dash && has_content {
            return true;
        }
    }
    false
}

/// Insert one blank line before any table block whose preceding line is
/// non-blank (#95). Telegram's rich-markdown parser accepts a GFM table only
/// as a standalone block: a table that directly follows a text line renders
/// as raw pipes (probe matrix A/B/C/D — blank line present = rendered,
/// abutting = raw). Detection reuses [`try_parse`], so exactly the blocks the
/// AST parser accepts get normalized — prose with stray pipes is never
/// touched. Code fences (``` and ~~~) are never mutated, the pass is
/// idempotent, and pipe-free input returns unchanged.
pub(crate) fn ensure_blank_line_before_tables(text: &str) -> String {
    if !text.contains('|') {
        return text.to_string();
    }
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    let mut in_fence = false;
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push(line.clone());
            i += 1;
            continue;
        }
        if !in_fence && let Some((_, end)) = try_parse(&lines, i) {
            let prev_is_text = out.last().is_some_and(|prev| !prev.trim().is_empty());
            if prev_is_text {
                out.push(String::new());
            }
            while i < end {
                out.push(lines[i].clone());
                i += 1;
            }
            continue;
        }
        out.push(line.clone());
        i += 1;
    }
    let mut result = out.join("\n");
    if text.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// If a table begins at `lines[start]` (a pipe row immediately followed by a
/// separator row), parse it and return the table plus the index just past it.
pub(super) fn try_parse(lines: &[String], start: usize) -> Option<(Table, usize)> {
    let header_line = lines.get(start)?;
    let sep_line = lines.get(start + 1)?;
    if !looks_like_row(header_line) || !is_separator(sep_line) {
        return None;
    }

    let header: Vec<Vec<Inline>> = split_cells(header_line)
        .into_iter()
        .map(|c| parse_inlines(c.trim()))
        .collect();
    let align = parse_alignment(sep_line, header.len());

    let mut rows = Vec::new();
    let mut i = start + 2;
    while i < lines.len() && looks_like_row(&lines[i]) {
        let row: Vec<Vec<Inline>> = split_cells(&lines[i])
            .into_iter()
            .map(|c| parse_inlines(c.trim()))
            .collect();
        rows.push(row);
        i += 1;
    }

    Some((
        Table {
            align,
            header,
            rows,
        },
        i,
    ))
}

/// A line that could be a table row: contains a pipe and isn't blank.
fn looks_like_row(line: &str) -> bool {
    let t = line.trim();
    t.contains('|') && !t.is_empty()
}

/// A separator row: every cell is dashes with optional leading/trailing colon.
fn is_separator(line: &str) -> bool {
    let cells = split_cells(line);
    if cells.is_empty() {
        return false;
    }
    cells.iter().all(|c| {
        let c = c.trim();
        let core = c.trim_start_matches(':').trim_end_matches(':');
        !core.is_empty() && core.chars().all(|ch| ch == '-')
    })
}

/// Split a pipe row into trimmed cell strings, dropping the empty cells that
/// flank a row written with outer pipes (`| a | b |`).
fn split_cells(line: &str) -> Vec<&str> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').collect()
}

/// Derive per-column [`Align`] from the separator row, padded/truncated to
/// `cols` so it always matches the header width.
fn parse_alignment(sep: &str, cols: usize) -> Vec<Align> {
    let mut align: Vec<Align> = split_cells(sep)
        .into_iter()
        .map(|c| {
            let c = c.trim();
            let left = c.starts_with(':');
            let right = c.ends_with(':');
            match (left, right) {
                (true, true) => Align::Center,
                (true, false) => Align::Left,
                (false, true) => Align::Right,
                (false, false) => Align::None,
            }
        })
        .collect();
    align.resize(cols, Align::None);
    align
}
