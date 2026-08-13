//! Make URLs and file paths clickable in the TUI via OSC 8 (#1031).
//!
//! Nothing here was clickable except what the TERMINAL guessed. Terminals scan
//! rendered text for URL-shaped runs and linkify those themselves, which has
//! two limits users hit constantly:
//!
//! - It only recognises URLs. A path like `/Users/me/src/main.rs` matches no
//!   scheme, so no terminal ever linkifies it.
//! - It cannot span a line break. We wrap chat output before it reaches the
//!   terminal, so a wrapped URL is genuinely two fragments and neither is a
//!   valid URL. Zooming out until it fits one line "fixes" it, which is the
//!   tell.
//!
//! OSC 8 marks a span as a link explicitly, so any text can be one and the link
//! is an attribute of the span rather than a property of a line.
//!
//! ## Why this is a post-render pass
//!
//! Escapes cannot ride inside a `Span`: `ratatui`'s buffer splits content by
//! grapheme, so the escape is chopped across cells and rendered as literal
//! garbage, and every escape byte would consume a column.
//!
//! But `ratatui-crossterm` writes a cell's symbol verbatim
//! (`queue!(self.writer, Print(cell.symbol()))`), so folding the escape INTO a
//! cell that already holds one visible grapheme works: the terminal renders the
//! escape as zero-width and the grapheme as one column, while the buffer still
//! counts that cell as exactly one column. Widths line up with no adjustment.
//!
//! So: render normally, then read the text back out of the buffer, find the
//! links, and patch the first and last cell of each.
//!
//! ## Wrapping
//!
//! A link is opened per rendered row, never across rows, because the terminal
//! resets the attribute at a newline. A URL split across rows therefore gets
//! its own opener on each row — which is what makes it clickable at any zoom,
//! the symptom that started this.
//!
//! Detecting the continuation relies on the wrap being ours: `ratatui` breaks
//! at the area width, so a split URL ends exactly at the final column. A run
//! that touches that column is treated as continuing into the next row's
//! leading run.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// Start of an OSC 8 hyperlink. Terminated with ST (`ESC \`).
fn osc8_open(uri: &str) -> String {
    format!("\x1b]8;;{uri}\x1b\\")
}

/// Closes the most recent hyperlink.
const OSC8_CLOSE: &str = "\x1b]8;;\x1b\\";

/// A link found in one rendered row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RowLink {
    /// Column offsets within the row, relative to the scanned text.
    pub start: usize,
    pub end: usize,
    pub uri: String,
    /// The run reaches the last column, so it may continue on the next row.
    pub continues: bool,
}

/// Characters that commonly trail a URL in prose and are not part of it.
const TRAILING_PUNCT: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\'', '>'];

/// Characters that commonly precede one — a URL in parentheses or quotes is
/// still a URL, and leaving the opener attached makes the scheme check fail.
const LEADING_PUNCT: &[char] = &['(', '[', '{', '<', '"', '\''];

/// Strip surrounding prose punctuation, returning the trimmed run and how many
/// characters were removed from the FRONT — the caller needs that to keep the
/// link's start column aligned with the cell it must patch.
fn trim_punct(s: &str) -> (&str, usize) {
    let front = s.trim_start_matches(|c| LEADING_PUNCT.contains(&c));
    let removed = s.chars().count() - front.chars().count();
    (
        front.trim_end_matches(|c| TRAILING_PUNCT.contains(&c)),
        removed,
    )
}

/// Whether `word` looks like an absolute filesystem path worth linking.
///
/// Deliberately strict. A false positive turns ordinary prose into a dead link,
/// which is worse than leaving it plain: relative paths, bare words with
/// slashes ("and/or"), and anything that is not rooted are all skipped.
fn is_path(word: &str) -> bool {
    if !word.starts_with('/') || word.len() < 2 {
        return false;
    }
    // A path is one token; a run with spaces was mis-split upstream.
    if word.contains(char::is_whitespace) {
        return false;
    }
    // Require a filename-ish tail so bare "/" or "/usr/" style prose is skipped.
    word.rsplit('/').next().is_some_and(|tail| !tail.is_empty())
}

fn is_url(word: &str) -> bool {
    word.starts_with("http://") || word.starts_with("https://")
}

/// Find linkable runs in one row of text.
///
/// `width` is the row's column count, used to decide whether a run touches the
/// final column and may therefore continue on the next row.
pub(crate) fn find_links(text: &str, width: usize) -> Vec<RowLink> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let raw: String = chars[start..i].iter().collect();
        let (trimmed, lead) = trim_punct(&raw);
        if trimmed.is_empty() {
            continue;
        }
        let start = start + lead;
        let end = start + trimmed.chars().count();

        let uri = if is_url(trimmed) {
            Some(trimmed.to_string())
        } else if is_path(trimmed) {
            Some(format!("file://{trimmed}"))
        } else {
            None
        };

        if let Some(uri) = uri {
            out.push(RowLink {
                start,
                end,
                uri,
                // Touching the last column means the wrap may have cut it.
                continues: end >= width,
            });
        }
    }
    out
}

/// Patch a rendered area so its URLs and absolute paths are OSC 8 links.
///
/// Reads text back out of `buf`, so it must run AFTER the widget is drawn.
/// Idempotent: a cell already carrying an escape is left alone, so a repeated
/// pass over the same buffer cannot nest links.
pub(crate) fn linkify(buf: &mut Buffer, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let width = area.width as usize;

    for row in 0..area.height {
        let y = area.y + row;
        let mut text = String::with_capacity(width);
        for col in 0..area.width {
            text.push_str(buf[(area.x + col, y)].symbol());
        }
        // A cell already carrying an escape means this row was patched before.
        if text.contains('\x1b') {
            continue;
        }

        for link in find_links(&text, width) {
            let first = area.x + link.start as u16;
            let last = area.x + (link.end.saturating_sub(1)) as u16;
            if link.start >= width || link.end == 0 {
                continue;
            }

            let opened = {
                let cell = &buf[(first, y)];
                format!("{}{}", osc8_open(&link.uri), cell.symbol())
            };
            buf[(first, y)].set_symbol(&opened);

            // Close on the same row. The terminal drops the attribute at the
            // newline anyway, and a run that continues gets its own opener on
            // the next row.
            let closed = {
                let cell = &buf[(last, y)];
                format!("{}{}", cell.symbol(), OSC8_CLOSE)
            };
            buf[(last, y)].set_symbol(&closed);
        }
    }
}
