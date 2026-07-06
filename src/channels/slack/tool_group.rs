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

/// One tool row in a group.
#[derive(Debug, Clone)]
pub(crate) struct GroupEntry {
    pub name: String,
    pub context: String,
    /// None = running, Some(success) = finished.
    pub status: Option<bool>,
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

/// Summary line: overall icon + count + live status.
fn summary_line(entries: &[GroupEntry]) -> String {
    let n = entries.len();
    let running = entries.iter().filter(|e| e.status.is_none()).count();
    let failed = entries.iter().filter(|e| e.status == Some(false)).count();
    let (icon, tail) = if running > 0 {
        ("⚙️", format!(" · {running} running"))
    } else if failed > 0 {
        ("❌", format!(" · {failed} failed"))
    } else {
        ("✅", String::new())
    };
    format!(
        "{icon} *{n} tool call{}*{tail}",
        if n == 1 { "" } else { "s" }
    )
}

/// Render the group as message content. Collapsed shows only the summary
/// line; expanded lists every tool. Groups with more than one entry get
/// the toggle button (a single line has nothing extra to reveal).
pub(crate) fn render(group: &GroupState, ts: &SlackTs) -> SlackMessageContent {
    let text = if group.entries.len() == 1 && group.expanded {
        // Degenerate but possible via toggle: same as expanded list.
        let e = &group.entries[0];
        format!("{} *{}*{}", entry_icon(e.status), e.name, e.context)
    } else if group.expanded {
        let lines: Vec<String> = group
            .entries
            .iter()
            .map(|e| format!("{} *{}*{}", entry_icon(e.status), e.name, e.context))
            .collect();
        format!("{}\n{}", summary_line(&group.entries), lines.join("\n"))
    } else if group.entries.len() == 1 {
        let e = &group.entries[0];
        format!("{} *{}*{}", entry_icon(e.status), e.name, e.context)
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
