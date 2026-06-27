//! Profiles dialog top-level renderer.
//!
//! Layout:
//!
//! ```text
//! ┌─ 🦀 OpenCrabs AI Agent ──────────────────────────────────────┐
//! │                                                              │
//! │ ┌─ Filter ──────────────────────────────────────────────────┐│
//! │ │ > dev                                                     ││
//! │ └───────────────────────────────────────────────────────────┘│
//! │                                                              │
//! │   [*] default   Default profile (~/.opencrabs/)              │
//! │   [ ] dev       Development profile                          │
//! │   [ ] ops       Operations profile                           │
//! │                                                              │
//! │ n: new  d: delete  m: migrate  Enter: switch  Esc: close     │
//! └──────────────────────────────────────────────────────────────┘
//! ```

use crate::tui::app::App;
use crate::tui::app::profiles_dialog::state::{ProfileAction, matching};
use crate::tui::render::palette;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// Render the full profiles dialog into `area`.
pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let state = &app.profiles_dialog;

    match &state.action {
        ProfileAction::CreateName | ProfileAction::CreateDesc => {
            draw_create_flow(frame, app, area);
        }
        ProfileAction::ConfirmDelete(name) => {
            draw_confirm_delete(frame, area, name);
        }
        ProfileAction::MigrateFrom | ProfileAction::MigrateTo => {
            draw_migrate_flow(frame, app, area);
        }
        ProfileAction::None => {
            draw_browse(frame, app, area);
        }
    }
}

/// Draw the main browse view: filter + profile list + help bar.
fn draw_browse(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // filter input
            Constraint::Min(1),    // profile list
            Constraint::Length(1), // help bar
        ])
        .split(area);

    draw_filter(frame, app, chunks[0]);
    draw_profile_list(frame, app, chunks[1]);
    if chunks[2].height > 0 {
        draw_help_bar(frame, chunks[2]);
    }
}

fn draw_filter(frame: &mut Frame, app: &App, area: Rect) {
    let state = &app.profiles_dialog;
    let title = format!(" Filter (profiles: {}) ", state.profiles.len());
    let block = Block::default()
        .title(title)
        .title_style(palette::title_style(palette::TEAL))
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(palette::TEAL));
    let line = Line::from(vec![
        Span::styled(" > ", Style::default().fg(palette::TEAL)),
        Span::styled(
            state.filter.clone(),
            Style::default().fg(palette::TEXT_PRIMARY),
        ),
        Span::styled("▎", Style::default().fg(palette::TEXT_DIM)),
    ]);
    let para = Paragraph::new(line).block(block);
    frame.render_widget(para, area);
}

fn draw_profile_list(frame: &mut Frame, app: &App, area: Rect) {
    let state = &app.profiles_dialog;
    let visible = matching(&state.profiles, &state.filter);

    if visible.is_empty() {
        let msg = if state.filter.is_empty() {
            "No profiles found."
        } else {
            "No profiles match the filter."
        };
        let line = Line::from(vec![Span::raw("\n  "), Span::styled(msg, palette::muted())]);
        let para = Paragraph::new(line);
        frame.render_widget(para, area);
        return;
    }

    let selected = state.selected_index.min(visible.len() - 1);
    let visible_height = area.height as usize;

    // Auto-scroll so selected entry stays visible
    let scroll = if selected >= state.scroll_offset as usize + visible_height {
        selected.saturating_sub(visible_height) + 1
    } else if selected < state.scroll_offset as usize {
        selected
    } else {
        state.scroll_offset as usize
    };

    let mut lines = Vec::new();
    for (idx, entry) in visible.iter().enumerate() {
        if idx < scroll {
            continue;
        }
        if lines.len() >= visible_height {
            break;
        }

        let is_active = entry.name == state.active_profile;
        let is_selected = idx == selected;

        let marker = if is_active { "[*]" } else { "[ ]" };
        let marker_style = if is_active {
            Style::default().fg(palette::TEAL)
        } else {
            palette::dim()
        };

        let name_style = if is_selected {
            Style::default()
                .fg(palette::TEXT_PRIMARY)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            Style::default().fg(palette::TEXT_PRIMARY)
        };

        let desc = entry.description.as_deref().unwrap_or("").to_string();

        let mut spans = vec![
            Span::styled(
                if is_selected { " > " } else { "   " },
                if is_selected {
                    Style::default().fg(palette::TEAL)
                } else {
                    palette::dim()
                },
            ),
            Span::styled(format!("{} ", marker), marker_style),
            Span::styled(format!("{:<16}", entry.name), name_style),
        ];
        if !desc.is_empty() {
            spans.push(Span::styled(desc, palette::dim()));
        }

        lines.push(Line::from(spans));
    }

    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}

fn draw_help_bar(frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(" n", palette::dim()),
        Span::styled(": new  ", palette::dim()),
        Span::styled("d", palette::dim()),
        Span::styled(": delete  ", palette::dim()),
        Span::styled("m", palette::dim()),
        Span::styled(": migrate  ", palette::dim()),
        Span::styled("Enter", palette::dim()),
        Span::styled(": switch  ", palette::dim()),
        Span::styled("Tab/↑↓", palette::dim()),
        Span::styled(": navigate  ", palette::dim()),
        Span::styled("type", palette::dim()),
        Span::styled(": filter  ", palette::dim()),
        Span::styled("Esc", palette::dim()),
        Span::styled(": close", palette::dim()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/// Draw the create profile flow.
fn draw_create_flow(frame: &mut Frame, app: &App, area: Rect) {
    let state = &app.profiles_dialog;
    let is_name_step = state.action == ProfileAction::CreateName;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // instructions
            Constraint::Length(3), // input field
            Constraint::Min(1),    // padding
        ])
        .split(area);

    let title_line = Line::from(Span::styled(
        " Create New Profile",
        Style::default()
            .fg(palette::TEAL)
            .add_modifier(ratatui::style::Modifier::BOLD),
    ));
    let hint = if is_name_step {
        "Enter a profile name (alphanumeric, hyphens, underscores):"
    } else {
        "Enter a description (optional, Enter to skip):"
    };
    let lines = vec![
        title_line,
        Line::default(),
        Line::from(Span::styled(hint, palette::dim())),
    ];
    let para = Paragraph::new(lines);
    frame.render_widget(para, chunks[0]);

    let input_value = if is_name_step {
        &state.input_buffer
    } else {
        &state.input_buffer_2
    };
    let label = if is_name_step { "Name" } else { "Description" };
    let block = Block::default()
        .title(format!(" {} ", label))
        .title_style(palette::title_style(palette::TEAL))
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(palette::TEAL));
    let line = Line::from(vec![
        Span::styled(" > ", Style::default().fg(palette::TEAL)),
        Span::styled(
            input_value.clone(),
            Style::default().fg(palette::TEXT_PRIMARY),
        ),
        Span::styled("▎", Style::default().fg(palette::TEXT_DIM)),
    ]);
    let para = Paragraph::new(line).block(block);
    frame.render_widget(para, chunks[1]);
}

/// Draw the delete confirmation screen.
fn draw_confirm_delete(frame: &mut Frame, area: Rect, name: &str) {
    let lines = vec![
        Line::default(),
        Line::from(Span::styled(
            format!("  Delete profile '{}'?", name),
            Style::default()
                .fg(palette::TEXT_PRIMARY)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )),
        Line::default(),
        Line::from(Span::styled(
            "  This will remove the profile directory and all its data.",
            palette::dim(),
        )),
        Line::from(Span::styled(
            "  The default profile cannot be deleted.",
            palette::dim(),
        )),
        Line::default(),
        Line::from(vec![
            Span::styled("  y", Style::default().fg(palette::TEAL)),
            Span::styled(": confirm  ", palette::dim()),
            Span::styled("Esc", Style::default().fg(palette::TEAL)),
            Span::styled(": cancel", palette::dim()),
        ]),
    ];
    let para = Paragraph::new(lines);
    frame.render_widget(para, area);
}

/// Draw the migrate flow.
fn draw_migrate_flow(frame: &mut Frame, app: &App, area: Rect) {
    let state = &app.profiles_dialog;
    let is_from_step = state.action == ProfileAction::MigrateFrom;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // instructions
            Constraint::Length(3), // input field
            Constraint::Min(1),    // padding
        ])
        .split(area);

    let title_line = Line::from(Span::styled(
        " Migrate Profile",
        Style::default()
            .fg(palette::TEAL)
            .add_modifier(ratatui::style::Modifier::BOLD),
    ));
    let hint = if is_from_step {
        "Enter source profile name:"
    } else {
        "Enter destination profile name:"
    };
    let lines = vec![
        title_line,
        Line::default(),
        Line::from(Span::styled(hint, palette::dim())),
    ];
    let para = Paragraph::new(lines);
    frame.render_widget(para, chunks[0]);

    let input_value = if is_from_step {
        &state.input_buffer
    } else {
        &state.input_buffer_2
    };
    let label = if is_from_step {
        "Source"
    } else {
        "Destination"
    };
    let block = Block::default()
        .title(format!(" {} ", label))
        .title_style(palette::title_style(palette::TEAL))
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(palette::TEAL));
    let line = Line::from(vec![
        Span::styled(" > ", Style::default().fg(palette::TEAL)),
        Span::styled(
            input_value.clone(),
            Style::default().fg(palette::TEXT_PRIMARY),
        ),
        Span::styled("▎", Style::default().fg(palette::TEXT_DIM)),
    ]);
    let para = Paragraph::new(line).block(block);
    frame.render_widget(para, chunks[1]);
}
