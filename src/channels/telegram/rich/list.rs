//! Bulleted / numbered list parsing, including task checkboxes and nested
//! child blocks. Nesting is driven by indentation: lines indented past an
//! item's marker are dedented and recursively parsed as that item's children.

use super::ast::{List, ListItem};
use super::inline::parse_inlines;
use super::parse::parse_blocks;

struct Marker {
    ordered: bool,
    /// Length (in bytes, measured from the first non-space char) of the marker
    /// token including its trailing space, e.g. `"- "` = 2, `"10. "` = 4.
    len: usize,
}

/// Whether `line` begins a list item.
pub(super) fn is_item(line: &str) -> bool {
    marker(line).is_some()
}

/// Parse a list beginning at `lines[*i]`, advancing `*i` past it. The list's
/// ordered-ness is taken from its first item.
pub(super) fn parse_list(lines: &[String], i: &mut usize) -> List {
    let base = indent_of(&lines[*i]);
    let ordered = marker(&lines[*i]).map(|m| m.ordered).unwrap_or(false);
    let mut items = Vec::new();

    while *i < lines.len() {
        let line = &lines[*i];
        if line.trim().is_empty() {
            match next_nonblank(lines, *i + 1) {
                Some(n) if indent_of(&lines[n]) >= base && is_item(&lines[n]) => {
                    *i += 1;
                    continue;
                }
                _ => break,
            }
        }
        if indent_of(line) != base {
            break;
        }
        let Some(m) = marker(line) else { break };

        let after_marker = &line.trim_start()[m.len..];
        let (task, content) = split_task(after_marker);
        *i += 1;

        // Gather everything indented deeper than this item as its children.
        let mut child: Vec<String> = Vec::new();
        while *i < lines.len() {
            let l = &lines[*i];
            if l.trim().is_empty() {
                match next_nonblank(lines, *i + 1) {
                    Some(n) if indent_of(&lines[n]) > base => {
                        child.push(String::new());
                        *i += 1;
                    }
                    _ => break,
                }
            } else if indent_of(l) > base {
                child.push(l.clone());
                *i += 1;
            } else {
                break;
            }
        }

        let children = if child.is_empty() {
            Vec::new()
        } else {
            parse_blocks(&dedent(&child))
        };

        items.push(ListItem {
            task,
            content: parse_inlines(content.trim()),
            children,
        });
    }

    List { ordered, items }
}

/// Identify a list marker at the start of `line` (after its indent).
fn marker(line: &str) -> Option<Marker> {
    let t = line.trim_start();
    for p in ["- ", "* ", "+ "] {
        if t.starts_with(p) {
            return Some(Marker {
                ordered: false,
                len: 2,
            });
        }
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let after = &t[digits..];
        if after.starts_with(". ") || after.starts_with(") ") {
            return Some(Marker {
                ordered: true,
                len: digits + 2,
            });
        }
    }
    None
}

/// Split a `- [ ]` / `- [x]` task checkbox off the front of an item's content.
fn split_task(s: &str) -> (Option<bool>, &str) {
    let t = s.trim_start();
    let checked = if t.starts_with("[ ]") {
        false
    } else if t.starts_with("[x]") || t.starts_with("[X]") {
        true
    } else {
        return (None, s);
    };
    (Some(checked), &t[3..])
}

/// Count leading indentation columns, treating a tab as four columns.
fn indent_of(line: &str) -> usize {
    let mut n = 0;
    for c in line.chars() {
        match c {
            ' ' => n += 1,
            '\t' => n += 4,
            _ => break,
        }
    }
    n
}

/// Index of the next non-blank line at or after `from`.
fn next_nonblank(lines: &[String], from: usize) -> Option<usize> {
    (from..lines.len()).find(|&j| !lines[j].trim().is_empty())
}

/// Strip the common leading indentation from a child block so it parses as if
/// it were top-level.
fn dedent(lines: &[String]) -> Vec<String> {
    let min = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_of(l))
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                strip_columns(l, min)
            }
        })
        .collect()
}

/// Remove up to `cols` columns of leading whitespace from `line`.
fn strip_columns(line: &str, cols: usize) -> String {
    let mut removed = 0;
    let mut cut = line.len();
    for (bi, c) in line.char_indices() {
        if removed >= cols {
            cut = bi;
            break;
        }
        match c {
            ' ' => removed += 1,
            '\t' => removed += 4,
            _ => {
                cut = bi;
                break;
            }
        }
    }
    line[cut..].to_string()
}
