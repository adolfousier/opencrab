//! Collapsible tool-call group for Discord (#380), matching the Telegram
//! block and the Slack Block Kit port: ONE message per turn, collapsed to
//! a live summary with an Expand button, toggled in place via component
//! interaction. State lives in [`super::DiscordState`] keyed by message id
//! so the click handler can re-render after the turn's closures are gone.
//! Expansion is per-message: everyone in the channel shares it.

use serenity::builder::{CreateActionRow, CreateButton};
use serenity::model::application::ButtonStyle;

/// One tool row in a group.
#[derive(Debug, Clone)]
pub(crate) struct GroupEntry {
    pub name: String,
    pub context: String,
    /// None = running, Some(success) = finished.
    pub status: Option<bool>,
}

/// A turn's tool group: contents plus display state.
#[derive(Debug, Clone)]
pub(crate) struct GroupState {
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
        "{icon} **{n} tool call{}**{tail}",
        if n == 1 { "" } else { "s" }
    )
}

/// Message body for the group in its current display state.
pub(crate) fn render_content(group: &GroupState) -> String {
    if group.entries.len() == 1 && !group.expanded {
        let e = &group.entries[0];
        return format!("{} **{}**{}", entry_icon(e.status), e.name, e.context);
    }
    if group.expanded {
        let lines: Vec<String> = group
            .entries
            .iter()
            .map(|e| format!("{} **{}**{}", entry_icon(e.status), e.name, e.context))
            .collect();
        format!("{}\n{}", summary_line(&group.entries), lines.join("\n"))
    } else {
        summary_line(&group.entries)
    }
}

/// Toggle components for the group message; empty for single-tool groups
/// (a lone line has nothing extra to reveal).
pub(crate) fn render_components(group: &GroupState, message_id: u64) -> Vec<CreateActionRow> {
    if group.entries.len() < 2 {
        return Vec::new();
    }
    let label = if group.expanded {
        "Collapse ▲"
    } else {
        "Expand ▼"
    };
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(format!("toolgroup:{message_id}"))
            .label(label)
            .style(ButtonStyle::Secondary),
    ])]
}
