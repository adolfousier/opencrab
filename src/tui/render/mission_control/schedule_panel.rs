//! Schedule panel — bottom right (60% × 50%), cron + pending approvals.
//!
//! Reads `app.mc.schedule` (populated by `actions::refresh`). The DB
//! call is async and lives in `schedule_service`; the renderer is
//! synchronous and just paints the snapshot.

use super::theme;
use crate::brain::mission_control::{McScheduleItem, McScheduleKind};
use crate::tui::app::App;

use crate::tui::render::palette;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn draw(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let entries = &app.mc.schedule;
    let border_color = if focused {
        theme::BORDER_SCHEDULE_FOCUS
    } else {
        theme::BORDER_IDLE
    };
    let title = format!(" Schedule ({}) ", entries.len());
    let block = Block::default()
        .title(title)
        .title_style(theme::title_style(theme::BORDER_SCHEDULE_FOCUS))
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(border_color));

    if entries.is_empty() {
        let empty = Paragraph::new(Line::from(vec![
            Span::raw("\n  "),
            Span::styled("No scheduled jobs.", Style::default().fg(theme::TEXT_DIM)),
        ]))
        .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let inner_w = area.width.saturating_sub(2) as usize;
    let visible_h = area.height.saturating_sub(2) as usize;

    let selected = if focused {
        Some(app.mc.selected_index.min(entries.len().saturating_sub(1)))
    } else {
        None
    };

    let lines: Vec<Line> = entries
        .iter()
        .enumerate()
        .map(|(idx, item)| build_line(item, inner_w, selected == Some(idx)))
        .collect();

    let scroll = compute_scroll(selected, entries.len(), visible_h);
    let para = Paragraph::new(lines).block(block).scroll((scroll, 0));
    frame.render_widget(para, area);
}

fn build_line(item: &McScheduleItem, inner_w: usize, selected: bool) -> Line<'static> {
    let icon = icon_for(item.kind);
    let kind_label = item.kind.label();
    // Reserve: " {icon} " (3) + " {label} " (kind label + 2 spaces around).
    let badge_text = format!(" {kind_label} ");
    let prefix_chars = 1 + 1 + 1 + badge_text.chars().count() + 2;
    let label_max = inner_w
        .saturating_sub(prefix_chars + item.schedule.chars().count() + 2)
        .max(8);
    let label = trunc(&item.label, label_max);

    let badge_color = if item.awaiting_user {
        theme::ORANGE
    } else {
        theme::TEAL
    };

    let mut spans = vec![
        Span::raw(" "),
        Span::styled(icon.to_string(), Style::default().fg(badge_color)),
        Span::raw(" "),
        Span::styled(
            badge_text,
            Style::default()
                .fg(palette::INK)
                .bg(badge_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            label,
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            item.schedule.clone(),
            Style::default().fg(theme::TEXT_SECONDARY),
        ),
    ];

    if selected {
        for span in &mut spans {
            span.style = span.style.bg(palette::SURFACE_PANEL);
        }
    }
    Line::from(spans)
}

fn icon_for(kind: McScheduleKind) -> &'static str {
    match kind {
        McScheduleKind::Cron => "⏰",
        McScheduleKind::PendingApproval => "🔒",
    }
}

fn compute_scroll(selected: Option<usize>, count: usize, visible_h: usize) -> u16 {
    let Some(sel) = selected else { return 0 };
    if visible_h == 0 || count == 0 {
        return 0;
    }
    let sel = sel.min(count.saturating_sub(1));
    if sel >= visible_h {
        (sel - visible_h + 1) as u16
    } else {
        0
    }
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
