//! GitHub-flavored pipe-table parsing.
//!
//! A table is a header row (`| a | b |`), a separator row of dashes with
//! optional alignment colons (`| :-- | --: |`), then zero or more body rows.
//! Parsing stops at the first line that is not a pipe row.

use super::ast::{Align, Inline, Table};
use super::inline::parse_inlines;

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
