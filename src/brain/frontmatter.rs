//! Brain-file YAML frontmatter reader.
//!
//! Brain files (`MEMORY.md`, `AGENTS.md`, user-created `*.md`, etc.) can
//! optionally declare metadata in a YAML frontmatter block at the top:
//!
//! ```markdown
//! ---
//! description: long-term memory the agent keeps across sessions
//! ---
//!
//! # MEMORY.md
//! ...content...
//! ```
//!
//! `extract_description` reads the `description` field from such a block
//! and returns it for the "Available Context Files" index that
//! `BrainLoader::build_core_brain` injects into every system prompt.
//!
//! ## Why a hand-rolled parser, not `serde_yaml`
//!
//! We need exactly one string field. A real YAML crate (`serde_yaml_ng`)
//! would add ~150 KB of build artifact and ~3s of compile time for
//! features (multi-document support, anchors, flow style, every escape
//! sequence) that brain-file frontmatter will never use. The hand-rolled
//! parser handles the shapes users actually write: single-line value,
//! single/double-quoted strings, CRLF, missing trailing newline, leading
//! whitespace, trailing `# comment`.
//!
//! ## What's intentionally NOT supported
//!
//! - Block scalars: `description: |\n  line one\n  line two`. The parser
//!   returns `|` as the value, the caller treats it as no-description
//!   and falls back to the hardcoded default. If we ever need multiline,
//!   the user can put it in a quoted string with `\n` escapes (or we
//!   bite the bullet and add the YAML dep).
//! - YAML anchors / references (`&anchor`, `*ref`).
//! - Flow style (`description: {foo: bar}`).
//! - Multi-document streams (`---` followed by more frontmatter).
//! - Inline escape sequences inside quoted strings (`\"` etc.) — the
//!   outermost matching quote pair is stripped and the value is taken
//!   verbatim from between them.
//!
//! All of these are unusual in markdown frontmatter and absent from any
//! brain file we ship. If a user needs them they can request the
//! `serde_yaml` upgrade and we'll re-evaluate.

/// Hard cap (in characters) on the frontmatter description shown in the
/// "Available Context Files" index. The whole index lands in every
/// system prompt for every turn, so a runaway multi-MB description in a
/// brain file would silently bloat the prompt and burn input-token
/// budget. 200 chars is one sentence of intent — enough for "long-term
/// memory the agent keeps across sessions; load when a request
/// references prior work." Anything longer is truncated with a trailing
/// `…` so both the LLM and the user see the cut.
pub const FRONTMATTER_DESCRIPTION_MAX_CHARS: usize = 200;

/// Extract the `description` field from YAML frontmatter at the top of
/// `content`. Returns `None` when there is no frontmatter, no
/// `description:` key, or the trimmed value is empty. The returned
/// string is capped at `FRONTMATTER_DESCRIPTION_MAX_CHARS` characters.
///
/// Frontmatter is delimited by `---` fences (the standard markdown
/// convention used by Jekyll / Hugo / Gatsby / MDX / Obsidian). Both
/// Unix (`\n`) and Windows (`\r\n`) line endings are accepted on every
/// fence and inside the block, and a file ending without a trailing
/// newline after the closing `---` still parses — markdown editors
/// sometimes strip the trailing newline on save.
pub fn extract_description(content: &str) -> Option<String> {
    let frontmatter = read_frontmatter_block(content)?;
    for raw_line in frontmatter.lines() {
        // Belt-and-suspenders CRLF: `str::lines` already strips `\n`
        // and a trailing `\r`, but be defensive in case future Rust
        // changes that contract.
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(after_key) = trimmed.strip_prefix("description:") else {
            continue;
        };
        let value_part = after_key.trim_start();
        let no_comment = strip_yaml_trailing_comment(value_part).trim_end();
        let unquoted = strip_outer_quotes(no_comment).trim();
        if unquoted.is_empty() {
            return None;
        }
        return Some(cap_description(unquoted));
    }
    None
}

/// Locate the frontmatter block bounded by `---` fences. Returns the
/// raw frontmatter contents (no surrounding fences), or `None` when
/// the opening fence is missing, the closing fence is missing, or the
/// opening fence isn't the very first thing in the file.
fn read_frontmatter_block(content: &str) -> Option<&str> {
    // The opening fence MUST be at byte 0; leading whitespace or a
    // blank line before it disqualifies the file from having
    // frontmatter (matches Jekyll / Hugo behaviour).
    let body = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))?;

    // Locate the closing fence. Three acceptable shapes:
    //   "\n---\n"     unix mid-file
    //   "\n---\r\n"   windows mid-file
    //   "\n---"       end of file with no trailing newline
    //
    // The unix and windows mid-file forms are searched in that order
    // so a file that mixes line endings still finds the right one.
    let close_idx = body
        .find("\n---\n")
        .or_else(|| body.find("\n---\r\n"))
        .or_else(|| body.ends_with("\n---").then(|| body.len() - "\n---".len()))?;

    Some(&body[..close_idx])
}

/// Strip a trailing YAML `#`-comment from a value, if any.
///
/// A `#` starts a comment when it appears outside of single/double
/// quotes AND is preceded by whitespace (so URLs like
/// `http://example.com/#anchor` and hex literals like `0x#…` don't
/// false-trigger). Comment-stripping returns everything BEFORE the `#`
/// trimmed of trailing whitespace.
fn strip_yaml_trailing_comment(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    // The value starts at byte 0 with the implicit "preceded by
    // whitespace" condition already satisfied — the caller has
    // already trim_start'd past the `description:` key.
    let mut prev_was_ws = true;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double && prev_was_ws => {
                return &s[..i];
            }
            _ => {}
        }
        prev_was_ws = b.is_ascii_whitespace();
    }
    s
}

/// Strip a matching outer pair of single or double quotes, if any.
/// Mismatched (`"foo'`), unterminated (`"foo`), or no-quote values are
/// returned verbatim — the caller already trim'd whitespace so a wrong
/// value just renders as itself rather than crashing.
fn strip_outer_quotes(s: &str) -> &str {
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        let first = bytes[0];
        let last = bytes[s.len() - 1];
        if first == last && (first == b'"' || first == b'\'') {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Cap `s` at `FRONTMATTER_DESCRIPTION_MAX_CHARS` characters,
/// appending a trailing `…` when truncation occurs. Counts characters,
/// not bytes, so multibyte glyphs (Cyrillic, CJK, emoji) don't push
/// the displayed length over the intended cap.
fn cap_description(s: &str) -> String {
    let char_count = s.chars().count();
    if char_count <= FRONTMATTER_DESCRIPTION_MAX_CHARS {
        s.to_string()
    } else {
        let head: String = s
            .chars()
            .take(FRONTMATTER_DESCRIPTION_MAX_CHARS)
            .collect();
        format!("{}…", head)
    }
}
