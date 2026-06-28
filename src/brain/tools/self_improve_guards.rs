//! Content guards for RSI brain-file writes.
//!
//! The `self_improve` tool lets the RSI loop edit brain files with no human
//! approval, so a handful of guards reject content that would degrade the
//! brain rather than improve it. This module holds the guard that stops the
//! RSI from disabling built-in tools (#236); the failure-log guard still lives
//! in `self_improve` itself.

/// Phrases that express a blanket prohibition on using a tool (as opposed to
/// nuanced routing like "prefer X over Y for case Z", which is fine).
const BAN_PHRASES: &[&str] = &[
    "do not use",
    "don't use",
    "do not call",
    "never use",
    "never call",
    "stop using",
    "avoid using",
    "blanket prohibition",
    "blanket ban",
    "fundamentally unreliable",
];

/// Reject brain-file content that tells the agent to BAN / avoid a built-in
/// tool. The RSI's job is to route ("prefer X over Y for case Z"), never to
/// disable a built-in: their failures are environmental or recoverable, and a
/// blanket prohibition just deletes capability (#236 — the RSI escalated
/// `hashline_edit` to "blanket DO NOT USE" over stale-hash retries).
///
/// Precise about the ban's OBJECT so it catches real tool bans without flagging
/// legitimate guidance:
/// - `do not use hashline_edit` / `never use telegram_send` → banned (object is
///   the tool, possibly after an article);
/// - `DO NOT USE it` under a `### hashline_edit` heading → banned (pronoun
///   object resolves to the section's tool);
/// - `do not use X; use edit_file` → NOT flagged on `edit_file` (that's the
///   recommended alternative, not the ban object);
/// - `never use \`git add -A\`` / `never use line numbers as a hash` → NOT
///   flagged (object isn't a tool).
///
/// Returns `Some(reason)` to reject.
pub fn bans_builtin_tool(content: &str) -> Option<String> {
    let mut section_tool: Option<String> = None;
    for line in content.lines() {
        // A heading scopes a section; remember its tool (if any) so a later
        // "DO NOT USE it" in the body can resolve the pronoun.
        if line.trim_start().starts_with('#') {
            section_tool = first_protected_tool(line);
        }
        if let Some(tool) = ban_target(line, &section_tool) {
            return Some(format!(
                "Refusing to write a rule that bans the built-in tool '{tool}'. Built-in tools \
                 must not be disabled — their failures are environmental or recoverable (a channel \
                 needing a live connection, a stale-hash retry, a declined prompt), not defects. \
                 Add routing guidance (when to prefer an alternative) instead of a blanket \
                 prohibition."
            ));
        }
    }
    None
}

/// First protected built-in tool name appearing as a token in `line`.
fn first_protected_tool(line: &str) -> Option<String> {
    line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .find(|t| !t.is_empty() && crate::brain::tools::catalog::is_protected_builtin(t))
        .map(|t| t.to_string())
}

/// If `line` contains a ban phrase whose OBJECT is a protected built-in tool,
/// return that tool name. The object is the first meaningful token(s) right
/// after the phrase (skipping leading articles); a pronoun object resolves to
/// `section_tool`. Anything else (a shell command, a parameter like "line
/// numbers", the recommended alternative further along) is not a tool ban.
fn ban_target(line: &str, section_tool: &Option<String>) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    for phrase in BAN_PHRASES {
        let Some(pos) = lower.find(phrase) else {
            continue;
        };
        let after = &line[pos + phrase.len()..];
        let toks: Vec<&str> = after
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .filter(|t| !t.is_empty())
            .collect();
        // Pronoun object → the section heading's tool, if known.
        if let Some(first) = toks.first() {
            let f = first.to_ascii_lowercase();
            if matches!(f.as_str(), "it" | "this" | "that" | "these" | "those") {
                if let Some(t) = section_tool {
                    return Some(t.clone());
                }
                continue;
            }
        }
        // Otherwise the banned object is the first few tokens; skip leading
        // articles, then a protected tool there is a ban. A non-article,
        // non-tool object (a command, a parameter name) ends the search.
        for tok in toks.iter().take(3) {
            let t = tok.to_ascii_lowercase();
            if matches!(t.as_str(), "the" | "a" | "an" | "any") {
                continue;
            }
            if crate::brain::tools::catalog::is_protected_builtin(tok) {
                return Some(tok.to_string());
            }
            break;
        }
    }
    None
}
