//! Detail popup overlay — opens on Enter, dismissed on Esc.
//!
//! Renders the full record for the currently-selected item in the
//! focused panel. Inbox proposals show their rationale + tool/command
//! definition; activity entries show the parsed metadata; schedule
//! rows show the cron expression + paused/next-run state.
//!
//! Pure read of `app.mc` snapshots — no service calls during render.

use super::theme;
use crate::brain::mission_control::{
    McActivity, McAnalytics, McInboxDetail, McInboxItem, McInboxKind, McScheduleItem,
    McScheduleKind, inbox_service,
};
use crate::tui::app::App;
use crate::tui::app::mission_control::McPanel;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

/// Render a centred popup occupying ~60% × 70% of `area`.
pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let pw = (area.width * 60 / 100).max(40);
    let ph = (area.height * 70 / 100).max(12);
    let px = area.x + area.width.saturating_sub(pw) / 2;
    let py = area.y + area.height.saturating_sub(ph) / 2;
    let popup = Rect::new(px, py, pw, ph);

    frame.render_widget(Clear, popup);

    let (title, accent, lines) = match app.mc.focused_panel {
        McPanel::Inbox => inbox_detail(app),
        McPanel::Activity => activity_detail(app),
        McPanel::Schedule => schedule_detail(app),
        McPanel::Analytics => analytics_detail(app),
    };

    let block = Block::default()
        .title(title)
        .title_style(theme::title_style(accent))
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(accent));

    let body = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block);
    frame.render_widget(body, popup);
}

// ── Inbox detail ────────────────────────────────────────────────────────────

fn inbox_detail(app: &App) -> (String, ratatui::style::Color, Vec<Line<'static>>) {
    let items = inbox_service::list();
    let item = items.get(app.mc.selected_index);
    let lines = match item {
        Some(item) => render_inbox_item(item),
        None => empty_lines("No proposal selected"),
    };
    (" Inbox detail ".to_string(), theme::TEAL, lines)
}

fn render_inbox_item(item: &McInboxItem) -> Vec<Line<'static>> {
    let kind_label = match item.kind {
        McInboxKind::ProposedTool => "tool",
        McInboxKind::ProposedCommand => "command",
        McInboxKind::ProposedSkill => "skill",
        McInboxKind::ProposedBrainDedup => "dedup",
    };
    let filed = item.created_at.format("%Y-%m-%d %H:%M UTC").to_string();
    let mut lines: Vec<Line<'static>> = vec![
        blank(),
        kv("Kind", kind_label),
        kv("Label", &item.label),
        kv("Source", &item.source),
        kv("Filed", &filed),
        kv("ID", &item.id),
        blank(),
        section_heading("Summary"),
    ];
    for s in wrap_paragraph(&item.summary) {
        lines.push(body_line(s));
    }

    // Render type-specific detail sections when available.
    if let Some(detail) = &item.detail {
        match detail {
            McInboxDetail::BrainDedup {
                duplicate_text,
                rationale,
                duplicate_of,
                warnings,
            } => {
                lines.extend([blank(), section_heading("Rationale")]);
                for s in wrap_paragraph(rationale) {
                    lines.push(body_line(s));
                }
                lines.extend([
                    blank(),
                    section_heading("Duplicates"),
                    kv("Of", duplicate_of),
                ]);
                lines.extend([blank(), section_heading("Duplicate text (to be removed)")]);
                for s in wrap_paragraph(duplicate_text) {
                    lines.push(body_line(s));
                }
                if !warnings.is_empty() {
                    lines.extend([blank(), section_heading("Warnings")]);
                    for w in warnings {
                        lines.push(body_line(format!("⚠ {w}")));
                    }
                }
            }
        }
    }

    lines.extend([
        blank(),
        section_heading("Apply / reject"),
        body_line(format!(
            "Use `rsi_proposals apply {}` to install or `reject {}` to discard.",
            item.id, item.id
        )),
    ]);
    lines
}

// ── Activity detail ─────────────────────────────────────────────────────────

fn activity_detail(app: &App) -> (String, ratatui::style::Color, Vec<Line<'static>>) {
    let entry = app.mc.activity.get(app.mc.selected_index);
    let lines = match entry {
        Some(e) => render_activity(e),
        None => empty_lines("No activity selected"),
    };
    (" Activity detail ".to_string(), theme::ORANGE, lines)
}

fn render_activity(entry: &McActivity) -> Vec<Line<'static>> {
    let when = entry.timestamp.format("%Y-%m-%d %H:%M UTC").to_string();
    let mut lines: Vec<Line<'static>> = vec![
        blank(),
        kv("When", &when),
        kv("Source", &entry.source),
        kv("Level", level_label(entry.level)),
        blank(),
        section_heading("Detail"),
    ];
    for s in wrap_paragraph(&entry.detail) {
        lines.push(body_line(s));
    }
    lines
}

fn level_label(level: crate::brain::mission_control::McActivityLevel) -> &'static str {
    use crate::brain::mission_control::McActivityLevel as L;
    match level {
        L::Info => "info",
        L::Success => "success",
        L::Warn => "warn",
        L::Error => "error",
    }
}

// ── Schedule detail ─────────────────────────────────────────────────────────

fn schedule_detail(app: &App) -> (String, ratatui::style::Color, Vec<Line<'static>>) {
    let item = app.mc.schedule.get(app.mc.selected_index);
    let lines = match item {
        Some(i) => render_schedule(i),
        None => empty_lines("No schedule item selected"),
    };
    (" Schedule detail ".to_string(), theme::WHITE, lines)
}

fn render_schedule(item: &McScheduleItem) -> Vec<Line<'static>> {
    let kind = match item.kind {
        McScheduleKind::Cron => "cron job",
        McScheduleKind::PendingApproval => "pending approval",
    };
    let state = if item.awaiting_user {
        "awaiting user"
    } else {
        "active"
    };
    let mut lines: Vec<Line<'static>> = vec![
        blank(),
        kv("Kind", kind),
        kv("Label", &item.label),
        kv("State", state),
        blank(),
        section_heading("Schedule"),
    ];
    for s in wrap_paragraph(&item.schedule) {
        lines.push(body_line(s));
    }
    lines.extend([blank(), kv("ID", &item.id)]);
    lines
}

// ── Layout helpers ──────────────────────────────────────────────────────────

fn empty_lines(message: &str) -> Vec<Line<'static>> {
    vec![
        blank(),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(message.to_string(), theme::muted()),
        ]),
    ]
}

fn blank() -> Line<'static> {
    Line::raw("")
}

// ── Analytics detail ──────────────────────────────────────────────────────

fn analytics_detail(app: &App) -> (String, ratatui::style::Color, Vec<Line<'static>>) {
    let a: &McAnalytics = &app.mc.analytics;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let fail_pct = if a.tool_total_calls > 0 {
        (a.tool_total_fails as f64 / a.tool_total_calls as f64) * 100.0
    } else {
        0.0
    };

    lines.push(kv("Calls", &a.tool_total_calls.to_string()));
    lines.push(kv(
        "Fails",
        &format!("{} ({:.1}%)", a.tool_total_fails, fail_pct),
    ));
    lines.push(kv("RSI", &format!("{} applied", a.rsi_applied_total)));
    lines.push(kv(
        "Brain",
        &format!("{:.1} KB / {} files", a.brain_total_kb, a.brain_files.len()),
    ));

    if !a.top_tools.is_empty() {
        lines.push(Line::from(""));
        lines.push(section_heading("Top tools"));
        for t in &a.top_tools {
            lines.push(body_line(format!(
                "{:<16} {:>7}  {:>5.1}% fail",
                trunc_name(&t.name, 16),
                t.total,
                t.fail_rate
            )));
        }
    }
    if !a.flakiest_tools.is_empty() {
        lines.push(Line::from(""));
        lines.push(section_heading("Flakiest (≥5 calls)"));
        for t in &a.flakiest_tools {
            lines.push(body_line(format!(
                "{:<16} {:>5.1}% fail ({} calls)",
                trunc_name(&t.name, 16),
                t.fail_rate,
                t.total
            )));
        }
    }
    if !a.rsi_top_dimensions.is_empty() {
        lines.push(Line::from(""));
        lines.push(section_heading("RSI applied by dimension"));
        for (dim, n) in &a.rsi_top_dimensions {
            lines.push(body_line(format!("{dim}: {n}")));
        }
    }
    if !a.brain_files.is_empty() {
        lines.push(Line::from(""));
        lines.push(section_heading("Brain files"));
        for f in &a.brain_files {
            lines.push(body_line(format!(
                "{:<20} {:.1} KB",
                trunc_name(&f.name, 20),
                f.kb
            )));
        }
    }

    (
        " Analytics ".to_string(),
        theme::BORDER_ANALYTICS_FOCUS,
        lines,
    )
}

fn trunc_name(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn kv(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("{key:<8} "),
            Style::default()
                .fg(theme::TEXT_DIM)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(value.to_string(), Style::default().fg(theme::TEXT_PRIMARY)),
    ])
}

fn section_heading(text: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            text.to_string(),
            Style::default()
                .fg(theme::TEXT_DIM)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn body_line(text: String) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(text, Style::default().fg(theme::TEXT_PRIMARY)),
    ])
}

/// Greedy soft-wrap on whitespace so each line in the detail popup
/// fits comfortably within typical popup widths. We let the
/// `Paragraph` widget handle hard wrapping for over-long single
/// tokens via `Wrap { trim: false }`.
fn wrap_paragraph(text: &str) -> Vec<String> {
    text.split('\n').map(|s| s.to_string()).collect()
}
