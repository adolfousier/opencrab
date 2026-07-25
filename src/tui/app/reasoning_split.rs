//! Split persisted assistant content back into its chronological segments.
//!
//! The tool loop persists a turn one iteration at a time, each append being a
//! `<!-- reasoning -->` block followed by that iteration's visible text. Reload
//! used to split only on TOOL markers, so a turn that ran no tools collapsed
//! every iteration into ONE assistant message: think, text, think, text became
//! a single row holding all the text and one merged `details` blob.
//!
//! That single row is then the turn's last non-empty assistant message, which
//! makes it the final answer, which the per-turn fold must always show. A
//! reasoning-heavy turn therefore rendered as a wall after a restart while the
//! live view, where each iteration was its own row, had folded it away.
//!
//! Splitting on the reasoning markers restores the live layout, and it works on
//! rows already in the database because it changes no storage format.

/// One chronological piece of a persisted assistant message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Segment {
    /// A `<!-- reasoning -->` block's contents.
    Reasoning(String),
    /// Visible text between blocks.
    Text(String),
}

const OPEN: &str = "<!-- reasoning -->";
const CLOSE: &str = "<!-- /reasoning -->";

/// Split `content` into alternating text and reasoning segments, in order.
///
/// Empty pieces are dropped, so a turn whose iterations were pure reasoning
/// yields only `Reasoning` segments and never a blank row. An unclosed block
/// takes the rest of the input as reasoning: the alternative is dumping a
/// half-written chain into the visible answer, which is the exact failure this
/// exists to prevent.
pub(crate) fn split_segments(content: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut rest = content;

    while let Some(start) = rest.find(OPEN) {
        let before = &rest[..start];
        if !before.trim().is_empty() {
            out.push(Segment::Text(before.trim().to_string()));
        }
        let after_open = &rest[start + OPEN.len()..];
        match after_open.find(CLOSE) {
            Some(end) => {
                let inner = after_open[..end].trim();
                if !inner.is_empty() {
                    out.push(Segment::Reasoning(inner.to_string()));
                }
                rest = &after_open[end + CLOSE.len()..];
            }
            None => {
                let inner = after_open.trim();
                if !inner.is_empty() {
                    out.push(Segment::Reasoning(inner.to_string()));
                }
                rest = "";
                break;
            }
        }
    }

    if !rest.trim().is_empty() {
        out.push(Segment::Text(rest.trim().to_string()));
    }
    out
}
