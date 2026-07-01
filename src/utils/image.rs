/// Extract `<<IMG:path>>` markers from text.
///
/// Returns `(cleaned_text, vec_of_paths)` — the text has all markers removed
/// and trimmed, the vec contains the file paths in order of appearance.
pub fn extract_img_markers(text: &str) -> (String, Vec<String>) {
    extract_markers_with_prefix(text, "<<IMG:")
}

/// Extract `<<VID:path>>` markers from text — mirror of `extract_img_markers`
/// for video attachments. Used by channel handlers to strip the marker from
/// bot replies before display (the agent shouldn't normally echo it back, but
/// strip defensively so a leaking marker never lands in front of the user).
pub fn extract_vid_markers(text: &str) -> (String, Vec<String>) {
    extract_markers_with_prefix(text, "<<VID:")
}

/// Extract `<<react:emoji>>` directive from text.
///
/// Returns `(cleaned_text, Option<emoji>)` — the text has all `<<react:...>>`
/// markers removed and trimmed, and the first valid emoji is returned.
/// Follows the same `<<PREFIX:value>>` pattern as `<<IMG:path>>` and
/// `<<VID:path>>`. Multiple directives are stripped but only the first
/// emoji is returned.
///
/// The LLM outputs `<<react:👍>>` to signal a reaction-only response
/// (or a reaction alongside text). Channel handlers use the returned
/// emoji to call `set_message_reaction` on the user's message.
pub fn extract_react_marker(text: &str) -> (String, Option<String>) {
    let (cleaned, markers) = extract_markers_with_prefix(text, "<<react:");
    let emoji = markers.into_iter().next();
    (cleaned, emoji)
}

/// Generic `<<PREFIX:path>>` marker extractor. Walks the text, removes every
/// `<<PREFIX:...>>` occurrence, and collects the inner paths in order. UTF-8
/// safe (works on byte indices that lie on char boundaries — `find`/`replace_range`
/// handle that correctly for the ASCII delimiters used here).
fn extract_markers_with_prefix(text: &str, prefix: &str) -> (String, Vec<String>) {
    let mut out = text.to_string();
    let mut paths = Vec::new();
    let prefix_len = prefix.len();

    while let Some(start) = out.find(prefix) {
        let Some(rel_end) = out[start..].find(">>") else {
            break;
        };
        let end = start + rel_end + 2; // past ">>"
        let path = out[start + prefix_len..start + rel_end].trim().to_string();
        if !path.is_empty() {
            paths.push(path);
        }
        out.replace_range(start..end, "");
    }

    (out.trim().to_string(), paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_react_marker ─────────────────────────────────────────────

    #[test]
    fn react_bare_directive() {
        let (text, emoji) = extract_react_marker("<<react:👍>>");
        assert_eq!(text, "");
        assert_eq!(emoji.as_deref(), Some("👍"));
    }

    #[test]
    fn react_directive_with_text() {
        let (text, emoji) = extract_react_marker("Sure thing! <<react:✅>>");
        assert_eq!(text, "Sure thing!");
        assert_eq!(emoji.as_deref(), Some("✅"));
    }

    #[test]
    fn react_no_directive() {
        let (text, emoji) = extract_react_marker("Just a normal message.");
        assert_eq!(text, "Just a normal message.");
        assert!(emoji.is_none());
    }

    #[test]
    fn react_multiple_directives_uses_first() {
        let (text, emoji) = extract_react_marker("<<react:👍>> and <<react:❤️>>");
        assert_eq!(text, "and");
        assert_eq!(emoji.as_deref(), Some("👍"));
    }

    #[test]
    fn react_empty_directive_ignored() {
        let (text, emoji) = extract_react_marker("<<react:>>");
        assert_eq!(text, "");
        assert!(emoji.is_none());
    }

    #[test]
    fn react_malformed_no_closing() {
        // Missing >> — should be left untouched
        let (text, emoji) = extract_react_marker("<<react:👍");
        assert_eq!(text, "<<react:👍");
        assert!(emoji.is_none());
    }

    #[test]
    fn react_whitespace_trimmed() {
        let (text, emoji) = extract_react_marker("  <<react:🔥>>  ");
        assert_eq!(text, "");
        assert_eq!(emoji.as_deref(), Some("🔥"));
    }

    #[test]
    fn react_non_emoji_text_still_extracted() {
        // Even non-standard emoji text is extracted as-is; the caller decides validity
        let (text, emoji) = extract_react_marker("<<react:hello>>");
        assert_eq!(text, "");
        assert_eq!(emoji.as_deref(), Some("hello"));
    }

    #[test]
    fn react_with_surrounding_newlines() {
        let (text, emoji) = extract_react_marker("\n\n<<react:🔥>>\n\n");
        assert_eq!(text, "");
        assert_eq!(emoji.as_deref(), Some("🔥"));
    }

    #[test]
    fn react_embedded_in_middle() {
        let (text, emoji) = extract_react_marker("Hello <<react:👋>> world");
        assert_eq!(text, "Hello  world");
        assert_eq!(emoji.as_deref(), Some("👋"));
    }

    #[test]
    fn react_only_react_with_no_extra_text() {
        // The common reaction-only case: LLM outputs just the directive
        let (text, emoji) = extract_react_marker("<<react:✅>>");
        assert!(text.trim().is_empty());
        assert_eq!(emoji.as_deref(), Some("✅"));
    }

    // ── extract_img_markers (existing, regression) ───────────────────────

    #[test]
    fn img_basic() {
        let (text, paths) = extract_img_markers("here <<IMG:/tmp/a.png>> done");
        // The extractor removes the marker but doesn't collapse interior whitespace.
        assert_eq!(text, "here  done");
        assert_eq!(paths, vec!["/tmp/a.png"]);
    }
}
