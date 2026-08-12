//! Tell the model how to format for Slack (#1016).
//!
//! Slack has no table markup. A markdown table therefore reaches the channel
//! as its own source — `| Metric | Before | After |` over `|---|---|---|` — in
//! a proportional font where no column can line up. One reported completion
//! carried six of them.
//!
//! Nothing downstream can rescue that. `markdown_to_mrkdwn` is a
//! character-level converter with no concept of a block, and Block Kit has no
//! table primitive to convert into, so by the time the text exists the table
//! has nowhere to go.
//!
//! Asked explicitly to format for Slack, the model produces exactly the right
//! shape: section headers, dividers, key-value lines, status glyphs, and no
//! tables at all. It was never told. This is that instruction, made permanent.
//!
//! The guidance names constructs the renderer actually honours — `---` becomes
//! a real divider block in `blocks::blocks_from_mrkdwn`, and single-asterisk
//! bold is mrkdwn's own — so the prompt and the renderer agree instead of the
//! model guessing at a dialect.

/// Formatting rules appended to the Slack channel preamble.
///
/// Kept as one block so the wording lives in a single place, and out of
/// `handler.rs`, where it would be the third multi-line string literal inside
/// an already long function.
pub(crate) const SLACK_FORMATTING: &str = "\
[Slack formatting — your answer is rendered as Slack mrkdwn, which is NOT \
markdown. Format for it deliberately:\n\
\n\
- NEVER use markdown tables. Slack has no table syntax, so pipes and dashes \
render literally as text and nothing aligns. Present tabular data as \
key-value lines instead: a bold label, then the value in `backticks`. For a \
before/after, write it inline: `RestartCount:` 272 ➜ 0\n\
- Bold is *single asterisks*, not **double**. Italic is _underscores_. \
Strikethrough is ~tildes~.\n\
- Separate major sections with a line containing only --- . That renders as a \
real divider, so use it to structure a long answer.\n\
- Give each section a short header line: an emoji, then the title in bold \
caps. Keep it to a few words.\n\
- Put every literal in `backticks`: commands, paths, file names, IDs, config \
keys, numeric readings. It is monospace and it survives copy-paste.\n\
- Use a status glyph where a reader scans for pass/fail: ✅ done, ⚠️ caveat, \
🟡 pending, ❌ failed.\n\
- Bulleted lists use a leading dash. Nest a detail under a line with └ rather \
than indenting, which Slack collapses.\n\
- Multi-line code, logs and command output go in a fenced block. Everything \
else should not.\n\
\n\
Write for someone scanning on a phone: lead with the outcome, keep paragraphs \
short, and let the structure carry the reading order.]";

/// The preamble line plus the formatting rules.
///
/// `channel_id` is surfaced so the agent can target this channel for
/// cross-surface sends without guessing (#533).
pub(crate) fn slack_preamble(channel_id: &str) -> String {
    format!(
        "[Channel: Slack (channel_id: {channel_id}) — your text response is automatically \
         sent to this channel. Do NOT call slack_send to deliver your answer. Only use \
         slack_send for: sending to a different channel, threads, blocks, reactions, \
         files, or moderation.]\n{SLACK_FORMATTING}\n"
    )
}
