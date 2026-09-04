//! Skills dialog top-level renderer.
//!
//! Layout:
//!
//! ```text
//! ┌─ 🦀 OpenCrabs AI Agent ──────────────────────────────────────┐
//! │                                                              │
//! │ ┌─ Filter ──────────────────────────────────────────────────┐│
//! │ │ > security                                                ││
//! │ └───────────────────────────────────────────────────────────┘│
//! │                                                              │
//! │ ╭─ /security-audit  [built-in] ─╮                            │
//! │ │ Run a comprehensive language… │                            │
//! │ ╰───────────────────────────────╯                            │
//! │ ...                                                          │
//! │                                                              │
//! │ Tab/↑↓: navigate  Enter: run  Esc: close  type to filter     │
//! └──────────────────────────────────────────────────────────────┘
//! ```

use super::card;
use crate::tui::app::App;
use crate::tui::app::skills_dialog::matching;

use super::super::palette;
use super::super::theme::{self, Role};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// Render the full skills dialog into `area`.
pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // filter input
            Constraint::Min(1),    // skill list
            Constraint::Length(1), // help bar
        ])
        .split(area);

    draw_filter(frame, app, chunks[0]);
    draw_list(frame, app, chunks[1]);
    if chunks[2].height > 0 {
        draw_help_bar(frame, chunks[2]);
    }
}

fn draw_filter(frame: &mut Frame, app: &App, area: Rect) {
    let title = format!(" Filter (skills: {}) ", app.skills.len());
    let block = Block::default()
        .title(title)
        .title_style(palette::title_style(theme::role(Role::AccentTeal)))
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(theme::role(Role::AccentTeal)));
    let line = Line::from(vec![
        Span::styled(" > ", Style::default().fg(theme::role(Role::AccentTeal))),
        Span::styled(
            app.skills_dialog.filter.clone(),
            Style::default().fg(theme::role(Role::TextPrimary)),
        ),
        // Soft cursor — block char in dim grey trailing the input.
        Span::styled("▎", Style::default().fg(theme::role(Role::TextDim))),
    ]);
    let para = Paragraph::new(line).block(block);
    frame.render_widget(para, area);
}

fn draw_list(frame: &mut Frame, app: &App, area: Rect) {
    let visible = matching(&app.skills, &app.skills_dialog.filter);

    if visible.is_empty() {
        let msg = if app.skills_dialog.filter.is_empty() {
            "No skills loaded."
        } else {
            "No skills match the filter."
        };
        let line = Line::from(vec![Span::raw("\n  "), Span::styled(msg, palette::muted())]);
        let para = Paragraph::new(line);
        frame.render_widget(para, area);
        return;
    }

    let selected = app.skills_dialog.selected_index.min(visible.len() - 1);
    // Each card is exactly 4 rows tall: top border + header + desc + bottom border.
    const CARD_H: u16 = 4;
    const GAP_H: u16 = 1;
    let card_block_h = CARD_H + GAP_H; // card + gap below

    // Auto-scroll so the selected card stays in view.
    let visible_h = area.height;
    let selected_top = (selected as u16) * card_block_h;
    let scroll = if selected_top + CARD_H > visible_h {
        // Push selected card to roughly two-thirds down the visible area.
        selected_top.saturating_sub(visible_h * 2 / 3)
    } else {
        0
    };

    for (idx, skill) in visible.iter().enumerate() {
        let row_top = (idx as u16) * card_block_h;
        if row_top + CARD_H <= scroll {
            continue; // entirely above viewport
        }
        let abs_y = area.y + row_top.saturating_sub(scroll);
        if abs_y >= area.y + area.height {
            break; // past viewport
        }
        let card_area = Rect {
            x: area.x,
            y: abs_y,
            width: area.width,
            height: CARD_H.min(area.y + area.height - abs_y),
        };
        card::draw(frame, card_area, skill, idx == selected);
    }
}

fn draw_help_bar(frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(" Tab/↑↓", palette::dim()),
        Span::styled(": navigate  ", palette::dim()),
        Span::styled("Enter", palette::dim()),
        Span::styled(": run  ", palette::dim()),
        Span::styled("Esc", palette::dim()),
        Span::styled(": close  ", palette::dim()),
        Span::styled("type", palette::dim()),
        Span::styled(": filter", palette::dim()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
