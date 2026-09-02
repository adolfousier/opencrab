//! Finding a drag-dropped file path inside a message (#1288).
//!
//! Terminals insert a dropped file as text, so the path arrives mixed into
//! whatever the user typed around it. That is easy while the path has no
//! spaces and impossible with `split_whitespace` once it does — and macOS
//! names every screenshot `Screenshot 2026-09-01 at 18.18.16.png`, so the
//! spaces are the common case rather than the exotic one.
//!
//! The scan is anchored on the EXTENSION rather than on word boundaries. A
//! dropped path ends at a known media extension, so each occurrence of one is
//! a candidate end; the start is then found by walking left through the
//! plausible beginnings and keeping the longest that resolves to a real file.
//! Longest-wins matters: `/a/b/My File.png` must not resolve to `File.png`
//! sitting in the working directory.

use std::path::Path;

/// What a candidate path in the message turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Dropped {
    /// Resolved to a real file here. Carries the byte range it occupied in
    /// the original text and the resolved on-disk path.
    Here {
        start: usize,
        end: usize,
        path: String,
    },
    /// Looks like an absolute path but nothing is at that location. Almost
    /// always a drop from another machine into a session running over SSH,
    /// which is worth saying out loud rather than forwarding as prose.
    Elsewhere {
        start: usize,
        end: usize,
        path: String,
    },
}

impl Dropped {
    pub(crate) fn range(&self) -> (usize, usize) {
        match self {
            Dropped::Here { start, end, .. } | Dropped::Elsewhere { start, end, .. } => {
                (*start, *end)
            }
        }
    }
}

/// Resolve a drag-dropped path to its real on-disk form.
///
/// Terminals shell-escape dropped paths: spaces become `\ `, and the whole
/// thing may be wrapped in quotes. The raw string therefore fails
/// `Path::exists()` even though the file is right there.
///
/// Tries the raw string first (so Windows backslash *separators* are never
/// mangled), then an unquoted form, then a POSIX-unescaped form, returning
/// the first that actually exists. `None` means no real file.
pub(crate) fn resolve(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    if Path::new(raw).exists() {
        return Some(raw.to_string());
    }
    let unquoted = unquote(raw);
    if unquoted != raw && Path::new(unquoted).exists() {
        return Some(unquoted.to_string());
    }
    if unquoted.contains('\\') {
        let unescaped = unescape(unquoted);
        if Path::new(&unescaped).exists() {
            return Some(unescaped);
        }
    }
    None
}

/// Strip one layer of matching surrounding quotes.
fn unquote(raw: &str) -> &str {
    raw.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(raw)
}

/// POSIX shell unescape: drop the backslash before any escaped char.
///
/// Only ever applied when the result is then checked for existence, so a
/// genuine backslash in a path is never wrongly stripped.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Could a path plausibly begin at this byte offset?
///
/// Only absolute and explicitly-relative forms count. A bare `File.png` is
/// not treated as a drop, because bare words appear in ordinary prose and
/// resolving them against the working directory attaches the wrong file.
fn is_path_start(text: &str, at: usize) -> bool {
    if at > 0 && !text.as_bytes()[at - 1].is_ascii_whitespace() {
        return false;
    }
    let rest = &text[at..];
    rest.starts_with('/')
        || rest.starts_with("~/")
        || rest.starts_with("./")
        || rest.starts_with("../")
        // Quoted drop, e.g. "/a/b/My File.png".
        || ((rest.starts_with('"') || rest.starts_with('\'')) && rest.len() > 2)
}

/// Byte offsets just past every occurrence of one of `exts` in `lower`.
///
/// An extension only ends a path at a word boundary: `.png` inside
/// `.pngfoo` is not a file ending.
fn extension_ends(lower: &str, exts: &[&str]) -> Vec<usize> {
    let mut ends = Vec::new();
    for ext in exts {
        let mut from = 0usize;
        while let Some(rel) = lower[from..].find(ext) {
            let end = from + rel + ext.len();
            let boundary = lower[end..]
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || c == '"' || c == '\'');
            if boundary {
                ends.push(end);
            }
            from = from + rel + 1;
            if from >= lower.len() {
                break;
            }
        }
    }
    ends.sort_unstable();
    ends.dedup();
    ends
}

/// Find the first dropped path in `text` whose extension is in `exts`.
///
/// Returns the LONGEST candidate ending at the earliest extension, so a path
/// with spaces wins over the trailing fragment inside it. Prefers a path that
/// exists; falls back to reporting an absolute one that does not, which is
/// the cross-machine drop.
pub(crate) fn find(text: &str, exts: &[&str]) -> Option<Dropped> {
    let lower = text.to_lowercase();
    for end in extension_ends(&lower, exts) {
        let mut starts: Vec<usize> = (0..end).filter(|i| is_path_start(text, *i)).collect();
        // Longest first: the earliest viable start gives the longest path.
        starts.sort_unstable();

        // A real file always wins, however short.
        for &start in &starts {
            if let Some(path) = resolve(text[start..end].trim()) {
                return Some(Dropped::Here { start, end, path });
            }
        }
        // Nothing on disk. Report the longest absolute-looking candidate so
        // the caller can say WHICH path is unreachable.
        if let Some(&start) = starts.first() {
            let raw = text[start..end].trim();
            let cleaned = unescape(unquote(raw));
            if cleaned.starts_with('/') || cleaned.starts_with('~') {
                return Some(Dropped::Elsewhere {
                    start,
                    end,
                    path: cleaned,
                });
            }
        }
    }
    None
}
