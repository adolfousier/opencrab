//! Collapsible tool-call group for Slack, the Telegram `<blockquote
//! expandable>` equivalent. Slack has no native collapsed quote, so the
//! grouped message renders a summary line with an Expand button; clicking
//! it flips the stored group state and re-renders the SAME message with
//! the full tool list and a Collapse button. State lives in
//! [`super::SlackState`] keyed by the group message's ts so the
//! interaction handler can re-render long after the turn's closures are
//! gone. Expansion is per-message, not per-viewer: everyone in the
//! channel sees the same state (one shared message).

use slack_morphism::prelude::*;

/// One step in a group, in the order it happened.
///
/// Narration lives here rather than in its own chat message so the agent's
/// between-tool thinking folds away with the tools it sits between, the way
/// Telegram's flow block already does it. Posted standalone it read as an
/// answer, and on a turn that ended with an empty final it was promoted to
/// one (#943).
#[derive(Debug, Clone)]
pub(crate) enum GroupEntry {
    /// A tool invocation.
    Tool {
        name: String,
        context: String,
        /// None = running, Some(success) = finished.
        status: Option<bool>,
    },
    /// Inter-iteration narration: what the agent said between tool calls.
    Note(String),
}

impl GroupEntry {
    /// A running tool has no verdict yet. Notes are never "running" — they are
    /// a record of something already said.
    fn is_running(&self) -> bool {
        matches!(self, Self::Tool { status: None, .. })
    }

    fn is_failed(&self) -> bool {
        matches!(
            self,
            Self::Tool {
                status: Some(false),
                ..
            }
        )
    }

    fn is_tool(&self) -> bool {
        matches!(self, Self::Tool { .. })
    }
}

/// A turn's tool group: what it contains and how it is displayed.
#[derive(Debug, Clone)]
pub(crate) struct GroupState {
    pub channel: SlackChannelId,
    pub entries: Vec<GroupEntry>,
    pub expanded: bool,
}

fn entry_icon(status: Option<bool>) -> &'static str {
    match status {
        None => "⚙️",
        Some(true) => "✅",
        Some(false) => "❌",
    }
}

/// The narration held in a group, joined, or `None` if there is none.
///
/// Used only when a turn's final response comes back empty: the folded text is
/// then the whole answer, and leaving it inside a collapsed group means posting
/// nothing at all (#951). Tool rows are excluded — they are a record of what
/// ran, not something to say back to the user.
pub(crate) fn notes_text(entries: &[GroupEntry]) -> Option<String> {
    let joined = entries
        .iter()
        .filter_map(|e| match e {
            GroupEntry::Note(text) => Some(text.trim()),
            GroupEntry::Tool { .. } => None,
        })
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (!joined.is_empty()).then_some(joined)
}

/// One rendered line for a step.
fn entry_line(entry: &GroupEntry) -> String {
    match entry {
        GroupEntry::Tool {
            name,
            context,
            status,
        } => format!("{} *{}*{}", entry_icon(*status), name, context),
        GroupEntry::Note(text) => format!("💭 _{}_", text.trim()),
    }
}

/// Summary line: overall icon + counts + live status.
///
/// Counts steps AND tool calls, because a group now holds narration too and
/// "3 tool calls" beside five folded lines would not describe what is inside.
fn summary_line(entries: &[GroupEntry]) -> String {
    let steps = entries.len();
    let tools = entries.iter().filter(|e| e.is_tool()).count();
    let running = entries.iter().filter(|e| e.is_running()).count();
    let failed = entries.iter().filter(|e| e.is_failed()).count();
    let (icon, tail) = if running > 0 {
        ("⚙️", format!(" · {running} running"))
    } else if failed > 0 {
        ("❌", format!(" · {failed} failed"))
    } else {
        ("✅", String::new())
    };
    let counts = if steps == tools {
        format!("{tools} tool call{}", if tools == 1 { "" } else { "s" })
    } else {
        format!(
            "{steps} step{} · {tools} tool call{}",
            if steps == 1 { "" } else { "s" },
            if tools == 1 { "" } else { "s" }
        )
    };
    format!("{icon} *{counts}*{tail}")
}

/// Render the group as message content. Collapsed shows only the summary
/// line; expanded lists every tool. Groups with more than one entry get
/// the toggle button (a single line has nothing extra to reveal).
pub(crate) fn render(group: &GroupState, ts: &SlackTs) -> SlackMessageContent {
    let text = if group.entries.len() == 1 && group.expanded {
        // Degenerate but possible via toggle: same as expanded list.
        entry_line(&group.entries[0])
    } else if group.expanded {
        let lines: Vec<String> = group.entries.iter().map(entry_line).collect();
        format!("{}\n{}", summary_line(&group.entries), lines.join("\n"))
    } else if group.entries.len() == 1 {
        entry_line(&group.entries[0])
    } else {
        summary_line(&group.entries)
    };

    let mut blocks = vec![SlackBlock::Section(SlackSectionBlock::new().with_text(
        SlackBlockText::MarkDown(SlackBlockMarkDownText::new(text.clone())),
    ))];
    if group.entries.len() > 1 {
        let label = if group.expanded {
            "Collapse ▲"
        } else {
            "Expand ▼"
        };
        blocks.push(SlackBlock::Actions(SlackActionsBlock::new(vec![
            SlackActionBlockElement::Button(SlackBlockButtonElement::new(
                SlackActionId::new(format!("toolgroup:{}", ts)),
                SlackBlockPlainTextOnly::from(SlackBlockPlainText::new(label.to_string())),
            )),
        ])));
    }
    SlackMessageContent::new()
        .with_text(text)
        .with_blocks(blocks)
}
