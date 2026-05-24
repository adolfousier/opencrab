//! Fuzzy line-sequence matching for str_replace.
//!
//! Ported from OpenAI Codex `seek_sequence` (apply-patch/src/seek_sequence.rs)
//! and Alexey Leshchenko's Python port (agent-eval-matrix). Tolerates minor
//! whitespace and Unicode punctuation differences between the agent's
//! `old_text` and the actual file content.

use unicode_normalization::UnicodeNormalization;

/// Normalize Unicode punctuation/whitespace to ASCII equivalents so that
/// smart quotes, em-dashes, non-breaking spaces, etc. don't break matching.
fn normalise_unicode(s: &str) -> String {
    let nfc: String = s.nfc().collect();
    nfc.chars()
        .map(|c| match c {
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => '-',
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' => '\'',
            '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' => '"',
            '\u{00a0}' | '\u{2002}' | '\u{2003}' | '\u{2004}' | '\u{2005}' | '\u{2006}'
            | '\u{2007}' | '\u{2008}' | '\u{2009}' | '\u{200a}' | '\u{202f}' | '\u{205f}'
            | '\u{3000}' => ' ',
            other => other,
        })
        .collect()
}

/// Returns true if the two lines match under one of the progressively looser
/// comparators: exact, rstrip, strip, unicode-normalised.
fn line_matches(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.trim_end_matches(|c: char| c.is_ascii_whitespace())
        == b.trim_end_matches(|c: char| c.is_ascii_whitespace())
    {
        return true;
    }
    if a.trim() == b.trim() {
        return true;
    }
    normalise_unicode(a) == normalise_unicode(b)
}

/// Find all starting indices where `pattern` matches `lines` fuzzily.
/// Empty pattern matches only at `start` (if within bounds).
pub fn seek_sequence(lines: &[&str], pattern: &[&str], start: usize) -> Vec<usize> {
    if pattern.is_empty() {
        return if start <= lines.len() {
            vec![start]
        } else {
            Vec::new()
        };
    }
    if pattern.len() > lines.len() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let end = lines.len() - pattern.len() + 1;
    for i in start..end {
        if (0..pattern.len()).all(|j| line_matches(lines[i + j], pattern[j])) {
            matches.push(i);
        }
    }
    matches
}

/// Replace the first unique fuzzy line-sequence match of `old_str` with
/// `new_str` in `content`. Returns `Ok(new_content)` on success, or
/// `Err(message)` describing why the replace couldn't be applied.
pub fn fuzzy_replace_once(content: &str, old_str: &str, new_str: &str) -> Result<String, String> {
    if old_str.is_empty() {
        return Err("old_str must not be empty.".into());
    }

    // Fast path: exact substring match.
    if content.contains(old_str) {
        let count = content.matches(old_str).count();
        if count > 1 {
            return Err(format!(
                "old_str appears {count} times as exact substring. Include more context to make a unique match."
            ));
        }
        return Ok(content.replacen(old_str, new_str, 1));
    }

    // Slow path: fuzzy line-sequence match.
    let content_lines: Vec<&str> = content.lines().collect();
    let search_lines: Vec<&str> = old_str.lines().collect();
    let new_lines: Vec<&str> = new_str.lines().collect();

    let indices = seek_sequence(&content_lines, &search_lines, 0);
    if indices.is_empty() {
        return Err(
            "old_str not found (exact or fuzzy line match). Check whitespace and indentation."
                .into(),
        );
    }
    if indices.len() > 1 {
        return Err(format!(
            "old_str matches {} locations. Include more context to make a unique match.",
            indices.len()
        ));
    }

    let start = indices[0];
    let end_exclusive = start + search_lines.len();

    let mut result: Vec<&str> =
        Vec::with_capacity(content_lines.len() - search_lines.len() + new_lines.len());
    result.extend_from_slice(&content_lines[..start]);
    result.extend_from_slice(&new_lines);
    result.extend_from_slice(&content_lines[end_exclusive..]);

    let mut new_content = result.join("\n");
    if content.ends_with('\n') && !new_content.ends_with('\n') {
        new_content.push('\n');
    }

    Ok(new_content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seek_exact() {
        let lines = vec!["foo", "bar", "baz"];
        assert_eq!(seek_sequence(&lines, &["bar", "baz"], 0), vec![1]);
    }

    #[test]
    fn seek_rstrip() {
        let lines = vec!["foo ", "bar\t\t"];
        assert_eq!(seek_sequence(&lines, &["foo", "bar"], 0), vec![0]);
    }

    #[test]
    fn seek_trim_both() {
        let lines = vec![" foo ", " bar\t"];
        assert_eq!(seek_sequence(&lines, &["foo", "bar"], 0), vec![0]);
    }

    #[test]
    fn seek_pattern_longer() {
        let lines = vec!["one line"];
        let result = seek_sequence(&lines, &["too", "many", "lines"], 0);
        assert!(result.is_empty());
    }

    #[test]
    fn replace_exact_substring() {
        let content = "alpha\nbeta\ngamma\n";
        let new = fuzzy_replace_once(content, "beta", "BETA").unwrap();
        assert!(new.contains("BETA"));
        assert!(!new.contains("beta"));
    }

    #[test]
    fn replace_ambiguous_exact() {
        let content = "x\nx\n";
        let err = fuzzy_replace_once(content, "x", "y").unwrap_err();
        assert!(err.contains("2 times"));
    }

    #[test]
    fn replace_fuzzy_indent() {
        let content = "def main():\n    message = \"Hi\"\n";
        let old = "    message = \"Hi\"";
        let new = "    message = \"Hello\"";
        let result = fuzzy_replace_once(content, old, new).unwrap();
        assert!(result.contains("Hello"));
    }

    #[test]
    fn replace_smart_quotes() {
        let content = "println!(\"hello\");\n";
        let old = "println!(“hello”);";
        let new = "println!(\"hi\");";
        let result = fuzzy_replace_once(content, old, new).unwrap();
        assert!(result.contains("\"hi\""));
    }

    #[test]
    fn replace_preserves_trailing_newline() {
        let content = "a\nb\nc\n";
        let result = fuzzy_replace_once(content, "b", "B").unwrap();
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn replace_empty_old_errors() {
        let err = fuzzy_replace_once("x", "", "y").unwrap_err();
        assert!(err.contains("must not be empty"));
    }

    #[test]
    fn replace_not_found_errors() {
        let err = fuzzy_replace_once("hello world", "xyz", "abc").unwrap_err();
        assert!(err.contains("not found"));
    }
}
