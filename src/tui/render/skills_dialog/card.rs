//! One skill rendered as a rounded card — slug + source badge +
//! description. Mirrors the agentverse / mission-control card style
//! and reuses the same OpenCrabs palette via `mission_control::theme`.

use crate::brain::skills::{Skill, SkillSource};

use super::super::theme::{self, Role};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// Render one skill card into `area`. `selected` flips the border
/// from neutral grey to teal — same convention as MC's inbox cards.
pub fn draw(frame: &mut ratatui::Frame, area: Rect, skill: &Skill, selected: bool) {
    let border_color = if selected {
        theme::role(Role::AccentTeal)
    } else {
        theme::role(Role::TextMuted)
    };

    // Source badge: orange = built-in, teal = user. Same colour rules
    // as inbox tool / command badges so a glance carries meaning
    // across surfaces.
    let (source_label, badge_color) = match skill.source {
        SkillSource::Builtin => ("built-in", theme::role(Role::Accent)),
        SkillSource::User => ("user", theme::role(Role::AccentTeal)),
    };

    // Inner width budget for the body of the card.
    let inner_w = area.width.saturating_sub(2) as usize; // borders only

    // Header: slug + badge.
    let mut header_spans = vec![
        Span::raw(" "),
        Span::styled(
            skill.slash_name.clone(),
            Style::default()
                .fg(theme::role(Role::TextPrimary))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!(" {source_label} "),
            Style::default()
                .fg(theme::role(Role::Ink))
                .bg(badge_color)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    // Right-pad so the row doesn't render shorter than the card inner.
    let header_used = 1 + skill.slash_name.chars().count() + 2 + source_label.len() + 2;
    if inner_w > header_used {
        header_spans.push(Span::raw(" ".repeat(inner_w - header_used)));
    }
    let header = Line::from(header_spans);

    // Description body — truncated to one line. Detail-popup-style
    // multi-line lands later if the user asks for it.
    let desc_max = inner_w.saturating_sub(2);
    let desc = trunc(&skill.description, desc_max);
    let desc_line = Line::from(vec![
        Span::raw(" "),
        Span::styled(desc, Style::default().fg(theme::role(Role::TextSecondary))),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(border_color));
    let para = Paragraph::new(vec![header, desc_line]).block(block);
    frame.render_widget(para, area);
}

fn trunc(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let keep = max.saturating_sub(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}
