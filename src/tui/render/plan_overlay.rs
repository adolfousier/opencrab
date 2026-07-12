//! Plan overlay: the TUI surface for the design track. While a plan is
//! Editing it shows the scrollable session `.md` body with an
//! Approve/Discard footer (deliberately NOT the tool-policy approve
//! flow); while Active it shows the checklist with progress. The badge
//! line distinguishes pre-init (no approvable document yet, no Approve
//! hint) from post-init Editing and Active.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::tui::app::App;
use crate::utils::plan_files::{self, PlanModeState};

pub(super) fn render_plan_overlay(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(Clear, area);

    let Some(sid) = app.current_session.as_ref().map(|s| s.id) else {
        let p = Paragraph::new("No active session.")
            .block(Block::default().borders(Borders::ALL).title(" Plan "));
        f.render_widget(p, area);
        return;
    };

    let state = plan_files::plan_mode_state(sid);
    let (badge, badge_style) = match state {
        PlanModeState::NoPlan => ("no plan", Style::default().fg(Color::DarkGray)),
        PlanModeState::PreInitEditing => (
            "Editing · pre-init",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        PlanModeState::PostInitEditing => (
            "Editing",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        PlanModeState::Active => (
            "Active",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let body: String = match state {
        PlanModeState::NoPlan => "No live plan for this session.\n\nStart one with /plan \
                                  (design track) or ask for a checklist."
            .to_string(),
        PlanModeState::PreInitEditing => "Plan mode is on, but `plan init` has not created \
                                          the design document yet.\n\nDescribe what you want \
                                          planned; the agent explores, then drafts the \
                                          SESSION PLAN for your approval. Leave Plan mode \
                                          with /discard."
            .to_string(),
        PlanModeState::PostInitEditing => std::fs::read_to_string(plan_files::plan_md_path(sid))
            .unwrap_or_else(|_| "(the session plan .md is unreadable)".to_string()),
        PlanModeState::Active => crate::utils::plan_mode::show_plan(sid),
    };

    let title = Line::from(vec![
        Span::raw(" 📋 Plan · "),
        Span::styled(badge, badge_style),
        Span::raw(" "),
    ]);

    let footer = match state {
        PlanModeState::PostInitEditing => {
            " a approve · d discard · ↑/↓ scroll · Esc close  (validator runs on approve) "
        }
        PlanModeState::Active => " d discard · ↑/↓ scroll · Esc close ",
        PlanModeState::PreInitEditing => " d discard (leave Plan mode) · Esc close ",
        PlanModeState::NoPlan => " Esc close ",
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_bottom(Line::from(Span::styled(
            footer,
            Style::default().fg(Color::DarkGray),
        )));

    let paragraph = Paragraph::new(body)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.plan_overlay_scroll as u16, 0));
    f.render_widget(paragraph, area);
}
