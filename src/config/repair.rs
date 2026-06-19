//! Best-effort mechanical repair of a syntactically-broken `config.toml`.
//!
//! Hand-editing the config and leaving an array (or inline table) unterminated
//! is the most common way to break the whole file — e.g.
//!
//! ```toml
//! models = ["a", "b", "c"
//!
//! [providers.custom.other]
//! ```
//!
//! That single missing `]` makes the ENTIRE file fail to parse, which (before
//! this) took down config loading and flipped auto-always (yolo) users into
//! tool-approval prompts. This module closes the dangling delimiters so the
//! file parses again.
//!
//! Every repair is gated on the result re-parsing as valid TOML, so a repair
//! that can't be made cleanly is reported as failure (`None`) rather than
//! producing a subtly-wrong config. The caller falls back to last-known-good in
//! that case.

use std::path::{Path, PathBuf};

/// Read each candidate path, repair any that is syntactically broken, and write
/// the fix back to disk (backing up the broken original to `<file>.autofix.bak`
/// first). Returns true if at least one file was repaired and saved.
///
/// The caller is responsible for serializing this against other config writes
/// (it does no locking of its own).
pub fn autofix_config_files(paths: &[PathBuf]) -> bool {
    let mut repaired_any = false;
    for path in paths {
        if !path.exists() {
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Auto-fix: cannot read {:?}: {e}", path);
                continue;
            }
        };
        let Some((fixed, fixes)) = repair_toml(&content) else {
            continue;
        };
        if save_repaired(path, &fixed, &fixes) {
            repaired_any = true;
        }
    }
    repaired_any
}

/// Back up the broken original, then overwrite with the repaired content.
fn save_repaired(path: &Path, fixed: &str, fixes: &[String]) -> bool {
    let backup = path.with_extension("toml.autofix.bak");
    if let Err(e) = std::fs::copy(path, &backup) {
        tracing::warn!("Auto-fix: failed to back up {:?}: {e}", path);
    }
    match std::fs::write(path, fixed) {
        Ok(()) => {
            for f in fixes {
                tracing::warn!("Auto-fixed {:?}: {f}", path);
            }
            tracing::warn!(
                "Auto-fixed {} config syntax error(s) in {:?}; original saved to {:?}",
                fixes.len(),
                path,
                backup
            );
            true
        }
        Err(e) => {
            tracing::error!("Auto-fix: failed to write repaired {:?}: {e}", path);
            false
        }
    }
}

/// Attempt to repair `content`.
///
/// Returns `Some((fixed, fixes))` when a repair was applied AND the result
/// parses as valid TOML. Returns `None` when the content already parses, when
/// no fixable problem was found, or when the attempted repair still doesn't
/// parse.
pub fn repair_toml(content: &str) -> Option<(String, Vec<String>)> {
    // Already valid — nothing to do.
    if toml::from_str::<toml::Value>(content).is_ok() {
        return None;
    }

    let mut fixes = Vec::new();
    let fixed = balance_delimiters(content, &mut fixes)?;
    if fixes.is_empty() {
        // Nothing we know how to repair (e.g. a semantic/type error).
        return None;
    }

    // Only accept the repair if it actually parses now.
    match toml::from_str::<toml::Value>(&fixed) {
        Ok(_) => Some((fixed, fixes)),
        Err(_) => None,
    }
}

/// Walk the file line by line, tracking open value-position `[`/`{` across
/// lines (arrays may legally span lines). When an array is still open and a line
/// begins a new table header or a new top-level key assignment, the previous
/// value was left unterminated — close it. Close anything still open at end of
/// file.
///
/// Returns `None` (abort) if an unterminated *inline table* (`{`) would have to
/// be closed: TOML forbids multi-line inline tables, so there is no safe way to
/// repair one across a line break — the caller falls back to last-known-good.
fn balance_delimiters(content: &str, fixes: &mut Vec<String>) -> Option<String> {
    let mut out = String::with_capacity(content.len() + 8);
    let mut stack: Vec<char> = Vec::new();

    for line in content.split_inclusive('\n') {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let trimmed = body.trim_start();

        // An array is open but this line starts a fresh item — the prior value
        // never closed. Emit the missing closers on their own line first.
        if !stack.is_empty() && starts_new_item(trimmed) {
            let mut closers = String::new();
            while let Some(open) = stack.pop() {
                if open == '{' {
                    return None; // can't close an inline table across lines
                }
                closers.push(']');
                fixes.push("inserted ']' to close an unterminated array".to_string());
            }
            out.push_str(&closers);
            out.push('\n');
        }

        // A bare table header at depth 0 carries no value brackets to scan.
        if stack.is_empty() && is_header_line(trimmed) {
            out.push_str(body);
            out.push_str(nl);
            continue;
        }

        scan_value_line(body, &mut stack);
        out.push_str(body);
        out.push_str(nl);
    }

    // End of file — close whatever is still dangling.
    while let Some(open) = stack.pop() {
        if open == '{' {
            return None; // unterminated inline table at EOF — not safely fixable
        }
        out.push(']');
        fixes.push("inserted ']' at end of file to close an unterminated array".to_string());
    }

    Some(out)
}

/// A line that opens a new structural item at the top level: a table header
/// (`[a.b]` / `[[a.b]]`) or a top-level key assignment (`key = ...`). Array
/// element continuations (`"x",`, `[1, 2],`, `{ k = 1 },`) are NOT new items.
fn starts_new_item(trimmed: &str) -> bool {
    is_header_line(trimmed) || starts_top_level_key(trimmed)
}

/// Strict table-header match: `[dotted.key]` or `[[dotted.key]]` as the whole
/// line (optionally trailed by a comment). Crucially this does NOT match a
/// nested array element like `[1, 2],` (which contains commas/values), so an
/// open multi-line array is not mistaken for a header.
fn is_header_line(trimmed: &str) -> bool {
    let s = trimmed.trim_end();
    // Drop a trailing comment for the structural check.
    let s = match s.find('#') {
        Some(i) => s[..i].trim_end(),
        None => s,
    };
    if !s.starts_with('[') || !s.ends_with(']') {
        return false;
    }
    let double = s.starts_with("[[") && s.ends_with("]]");
    let inner = if double {
        &s[2..s.len() - 2]
    } else {
        &s[1..s.len() - 1]
    };
    if inner.is_empty() {
        return false;
    }
    // Header keys are dotted identifiers / quoted keys — no commas, no `=`.
    inner
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '"' | '\'' | ' '))
        && !inner.contains(',')
}

/// Heuristic: the line begins a `key = ...` assignment at the top level. Scans
/// from the start over a (possibly dotted/quoted) key; if the first
/// non-string special char is `=`, it's an assignment. A line starting with
/// `[`, `{`, `,`, `#` (i.e. array/table content) is not.
fn starts_top_level_key(trimmed: &str) -> bool {
    let mut chars = trimmed.chars().peekable();
    let first = match chars.peek() {
        Some(&c) => c,
        None => return false,
    };
    if !(first.is_ascii_alphanumeric() || matches!(first, '_' | '-' | '"' | '\'')) {
        return false;
    }
    let mut in_q: Option<char> = None;
    for c in trimmed.chars() {
        match in_q {
            Some(q) => {
                if c == q {
                    in_q = None;
                }
            }
            None => match c {
                '"' | '\'' => in_q = Some(c),
                '=' => return true,
                '[' | '{' | '#' => return false,
                _ => {}
            },
        }
    }
    false
}

/// Scan one line's value text and update the bracket stack, ignoring brackets
/// inside strings or after a `#` comment. String state is line-local (multi-line
/// `"""` literals are uncommon in config and are handled by the re-parse gate:
/// a mis-repair simply fails to parse and we fall back to last-known-good).
fn scan_value_line(body: &str, stack: &mut Vec<char>) {
    let mut in_basic = false;
    let mut in_literal = false;
    let mut escaped = false;

    for c in body.chars() {
        if in_basic {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_basic = false;
            }
            continue;
        }
        if in_literal {
            if c == '\'' {
                in_literal = false;
            }
            continue;
        }
        match c {
            '#' => break,
            '"' => in_basic = true,
            '\'' => in_literal = true,
            '[' | '{' => stack.push(c),
            ']' if stack.last() == Some(&'[') => {
                stack.pop();
            }
            '}' if stack.last() == Some(&'{') => {
                stack.pop();
            }
            _ => {}
        }
    }
}
