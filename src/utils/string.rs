//! String utility functions.

/// Truncate a string to at most `max_bytes` bytes, ensuring the cut lands on a
/// valid UTF-8 char boundary. Returns the longest prefix that fits.
pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Returns true if `s` looks like a file path rather than a slash command.
///
/// Slash commands are `/` followed by a single word with no additional slashes
/// and no file extension (e.g. `/help`, `/models`, `/deploy`).
///
/// File paths have additional `/` segments (e.g. `/Users/alice/file.pdf`)
/// or a recognizable file extension on the first word (e.g. `/report.pdf check this`).
///
/// This prevents drag-and-dropped file paths from triggering "Unknown command" errors.
pub fn looks_like_file_path(s: &str) -> bool {
    if !s.starts_with('/') {
        return false;
    }
    // If it contains another `/` after the leading slash, it's a path
    // (e.g. `/Users/...`, `/tmp/...`, `./` resolved to absolute)
    if s[1..].contains('/') {
        return true;
    }
    // If the first word (before any space) has a file extension, treat as path
    // e.g. `/report.pdf check this` → the `/report.pdf` part is a file
    let first_word = s.split_whitespace().next().unwrap_or(s);
    if let Some(ext) = std::path::Path::new(first_word).extension()
        && !ext.is_empty()
    {
        return true;
    }
    false
}

/// Collapse the current user's `$HOME` prefix to `~` in paths/commands.
/// `/Users/alice/srv/foo/bar.rs` → `~/srv/foo/bar.rs`. Keeps absolute
/// paths OUTSIDE home untouched so `/tmp/...` or `/etc/...` still render
/// faithfully. No-op if home isn't resolvable or doesn't appear in `s`.
pub fn tilde_home(s: &str) -> String {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return s.to_string(),
    };
    let home_str = home.to_string_lossy();
    if home_str.is_empty() {
        return s.to_string();
    }
    // Replace all occurrences — bash commands like `cd /Users/me/a && cp /Users/me/b /Users/me/c`
    // benefit from every instance being collapsed.
    s.replace(home_str.as_ref(), "~")
}

/// Shorten a string to fit `max_bytes` while preserving both ends —
/// essential for file paths where the filename (tail) is usually the
/// most informative part. `~/a/b/c/d/very_long_name.rs` truncated to
/// 30 bytes becomes `~/a/b/…/very_long_name.rs` rather than
/// `~/a/b/c/d/very_long_na` which loses the `.rs` extension entirely.
///
/// Respects UTF-8 char boundaries on both sides of the ellipsis.
pub fn truncate_middle(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    const ELLIPSIS: &str = "…"; // 3 bytes
    if max_bytes <= ELLIPSIS.len() + 2 {
        // Too small to preserve both ends meaningfully — fall back to head truncation.
        return truncate_str(s, max_bytes).to_string();
    }
    let budget = max_bytes - ELLIPSIS.len();
    // Slight bias toward keeping the tail since filenames / final args
    // carry more signal than the leading path components.
    let tail_bytes = budget.div_ceil(2);
    let head_bytes = budget - tail_bytes;

    let mut head_end = head_bytes;
    while head_end > 0 && !s.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = s.len() - tail_bytes;
    while tail_start < s.len() && !s.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    if tail_start <= head_end {
        return truncate_str(s, max_bytes).to_string();
    }
    format!("{}{}{}", &s[..head_end], ELLIPSIS, &s[tail_start..])
}

/// Format a token count as a compact human-readable string (e.g. "150K", "1.2M").
fn format_token_count(tokens: u32) -> String {
    let tokens = tokens as f64;
    if tokens >= 1_000_000.0 {
        format!("{:.1}M", tokens / 1_000_000.0)
    } else if tokens >= 1_000.0 {
        format!("{:.0}K", tokens / 1_000.0)
    } else if tokens > 0.0 {
        format!("{}", tokens as u32)
    } else {
        "0".to_string()
    }
}

/// Format a context budget footer line: "ctx: 8K/200K 4%".
///
/// Used by channel handlers to append a context usage indicator to the
/// final message delivered to the user. Plain text so it works across all
/// channel-specific formatters (Telegram HTML, Discord markdown, Slack mrkdwn, WhatsApp).
pub fn format_ctx_footer(used: u32, max: u32) -> String {
    let pct = if max > 0 {
        (used as f64 / max as f64) * 100.0
    } else {
        0.0
    };
    format!(
        "ctx: {}/{} {:.0}%",
        format_token_count(used),
        format_token_count(max),
        pct
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_str_ascii() {
        assert_eq!(truncate_str("hello world", 5), "hello");
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_multibyte_boundary() {
        // █ is U+2588, 3 bytes in UTF-8
        let s = "abc█def";
        // "abc" = 3 bytes, "█" = bytes 3..6, "def" = bytes 6..9
        assert_eq!(truncate_str(s, 3), "abc"); // exact boundary before █
        assert_eq!(truncate_str(s, 4), "abc"); // inside █, backs up to 3
        assert_eq!(truncate_str(s, 5), "abc"); // inside █, backs up to 3
        assert_eq!(truncate_str(s, 6), "abc█"); // exact boundary after █
    }

    #[test]
    fn test_truncate_str_emoji() {
        // 🦀 is U+1F980, 4 bytes in UTF-8
        let s = "hi🦀bye";
        // "hi" = 2 bytes, "🦀" = bytes 2..6, "bye" = bytes 6..9
        assert_eq!(truncate_str(s, 2), "hi");
        assert_eq!(truncate_str(s, 3), "hi"); // inside 🦀
        assert_eq!(truncate_str(s, 5), "hi"); // inside 🦀
        assert_eq!(truncate_str(s, 6), "hi🦀");
    }

    #[test]
    fn test_truncate_str_zero() {
        assert_eq!(truncate_str("hello", 0), "");
        assert_eq!(truncate_str("🦀", 0), "");
    }

    #[test]
    fn test_truncate_str_empty() {
        assert_eq!(truncate_str("", 5), "");
        assert_eq!(truncate_str("", 0), "");
    }

    #[test]
    fn test_truncate_str_all_multibyte() {
        // Each char is 3 bytes
        let s = "███"; // 9 bytes
        assert_eq!(truncate_str(s, 1), ""); // inside first █
        assert_eq!(truncate_str(s, 3), "█");
        assert_eq!(truncate_str(s, 7), "██"); // inside third █
        assert_eq!(truncate_str(s, 9), "███");
    }

    #[test]
    fn test_truncate_middle_preserves_tail() {
        let s = "/Users/alice/srv/dart/heyiolo/lib/presentation/pages/some_really_long_widget_name.dart";
        let out = truncate_middle(s, 40);
        // The filename must survive — that's the whole point of middle-elide.
        assert!(
            out.ends_with("some_really_long_widget_name.dart")
                || out.ends_with("_name.dart")
                || out.ends_with(".dart"),
            "expected filename tail preserved, got: {}",
            out
        );
        assert!(out.contains('…'), "expected ellipsis marker in middle");
        assert!(out.len() <= 40, "expected <= 40 bytes, got {}", out.len());
    }

    #[test]
    fn test_truncate_middle_short_string_unchanged() {
        assert_eq!(truncate_middle("short.rs", 80), "short.rs");
    }

    #[test]
    fn test_truncate_middle_tiny_budget_falls_back() {
        let out = truncate_middle("hello world", 4);
        // Budget too small for head+…+tail — falls back to head-only truncation.
        assert!(out.len() <= 4);
    }

    #[test]
    fn test_tilde_home_collapses_prefix() {
        if let Some(h) = dirs::home_dir() {
            let home_str = h.to_string_lossy();
            let full = format!("{}/srv/project/file.rs", home_str);
            assert_eq!(tilde_home(&full), "~/srv/project/file.rs");
            // Non-home paths untouched.
            assert_eq!(tilde_home("/tmp/foo"), "/tmp/foo");
            // Bash commands with multiple occurrences all collapsed.
            let cmd = format!("cp {}/a {}/b", home_str, home_str);
            assert_eq!(tilde_home(&cmd), "cp ~/a ~/b");
        }
    }

    // ── looks_like_file_path ───────────────────────────────────────────────

    #[test]
    fn test_looks_like_file_path_absolute_with_segments() {
        assert!(looks_like_file_path("/Users/alice/Downloads/report.pdf"));
        assert!(looks_like_file_path("/tmp/foo.txt"));
        assert!(looks_like_file_path("/etc/hosts"));
    }

    #[test]
    fn test_looks_like_file_path_with_extension_and_text() {
        // Drag-and-drop pattern: path followed by a message
        assert!(looks_like_file_path("/report.pdf check this"));
        assert!(looks_like_file_path("/data.csv analyze"));
    }

    #[test]
    fn test_looks_like_file_path_slash_commands_are_not_paths() {
        assert!(!looks_like_file_path("/help"));
        assert!(!looks_like_file_path("/models"));
        assert!(!looks_like_file_path("/deploy staging"));
        assert!(!looks_like_file_path("/credits"));
    }

    #[test]
    fn test_looks_like_file_path_no_slash_prefix() {
        assert!(!looks_like_file_path("hello world"));
        assert!(!looks_like_file_path("report.pdf"));
    }

    // ── format_ctx_footer ───────────────────────────────────────────────────

    #[test]
    fn test_format_ctx_footer_k_values() {
        assert_eq!(format_ctx_footer(8000, 200000), "ctx: 8K/200K 4%");
    }

    #[test]
    fn test_format_ctx_footer_small_values() {
        assert_eq!(format_ctx_footer(500, 200000), "ctx: 500/200K 0%");
    }

    #[test]
    fn test_format_ctx_footer_m_values() {
        assert_eq!(format_ctx_footer(1200000, 2000000), "ctx: 1.2M/2.0M 60%");
    }

    #[test]
    fn test_format_ctx_footer_zero_used() {
        assert_eq!(format_ctx_footer(0, 200000), "ctx: 0/200K 0%");
    }

    #[test]
    fn test_format_ctx_footer_zero_max() {
        assert_eq!(format_ctx_footer(5000, 0), "ctx: 5K/0 0%");
    }

    #[test]
    fn test_format_ctx_footer_full() {
        assert_eq!(format_ctx_footer(200000, 200000), "ctx: 200K/200K 100%");
    }
}
