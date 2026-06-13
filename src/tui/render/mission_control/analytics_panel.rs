//! Analytics panel (left column, under the Inbox). Brain sizes plus tool
//! usage / reliability and RSI application counts, read from the cached
//! `app.mc.analytics` snapshot. Read-only: the renderer never hits disk or
//! the DB itself.
//!
//! This is the native home for what the external `opencrabs-analytics` HTML
//! tool produced (discussion #178), reclaiming the formerly idle Inbox space.

use super::theme;
use crate::brain::mission_control::{McAnalytics, McToolStat};
use crate::tui::app::App;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn draw(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let data = &app.mc.analytics;
    let border_color = if focused {
        theme::BORDER_ANALYTICS_FOCUS
    } else {
        theme::BORDER_IDLE
    };
    let block = Block::default()
        .title(" Analytics ")
        .title_style(theme::title_style(theme::BORDER_ANALYTICS_FOCUS))
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(border_color));

    let inner_w = area.width.saturating_sub(2) as usize;
    let lines = build_lines(data, inner_w);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn build_lines(a: &McAnalytics, w: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // ── Summary ───────────────────────────────────────────────────────────
    let fail_pct = if a.tool_total_calls > 0 {
        (a.tool_total_fails as f64 / a.tool_total_calls as f64) * 100.0
    } else {
        0.0
    };
    lines.push(summary_row(
        "Tools",
        format!("{} calls", a.tool_total_calls),
    ));
    lines.push(summary_row(
        "Fails",
        format!("{} ({:.1}%)", a.tool_total_fails, fail_pct),
    ));
    lines.push(summary_row(
        "RSI",
        format!("{} applied", a.rsi_applied_total),
    ));
    lines.push(summary_row(
        "Brain",
        format!("{:.1} KB / {} files", a.brain_total_kb, a.brain_files.len()),
    ));

    // ── Top tools (with a proportional bar) ───────────────────────────────
    if !a.top_tools.is_empty() {
        lines.push(blank());
        lines.push(header("Top tools"));
        let max = a
            .top_tools
            .iter()
            .map(|t| t.total)
            .max()
            .unwrap_or(1)
            .max(1);
        for t in a.top_tools.iter().take(8) {
            lines.push(tool_row(t, max, w));
        }
    }

    // ── Flakiest tools ────────────────────────────────────────────────────
    if !a.flakiest_tools.is_empty() {
        lines.push(blank());
        lines.push(header("Flakiest (≥5 calls)"));
        for t in a.flakiest_tools.iter().take(6) {
            lines.push(fail_row(t, w));
        }
    }

    // ── Brain files ───────────────────────────────────────────────────────
    if !a.brain_files.is_empty() {
        lines.push(blank());
        lines.push(header("Brain files"));
        for f in a.brain_files.iter().take(8) {
            lines.push(kv_row(&f.name, format!("{:.1} KB", f.kb), w));
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No analytics yet.",
            Style::default().fg(theme::TEXT_DIM),
        )));
    }
    lines
}

fn blank() -> Line<'static> {
    Line::from("")
}

fn header(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {text}"),
        Style::default()
            .fg(theme::BORDER_ANALYTICS_FOCUS)
            .add_modifier(Modifier::BOLD),
    ))
}

fn summary_row(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(format!("{label:<6}"), Style::default().fg(theme::TEXT_DIM)),
        Span::styled(value, Style::default().fg(theme::GREEN)),
    ])
}

/// `name  ███▌ count` with the bar scaled to the busiest tool.
fn tool_row(t: &McToolStat, max: i64, w: usize) -> Line<'static> {
    let name_w = 12usize;
    let bar_w = 8usize;
    let name = pad(&trunc(&t.name, name_w), name_w);
    let filled = ((t.total as f64 / max as f64) * bar_w as f64).round() as usize;
    let bar: String = "█".repeat(filled.min(bar_w));
    let rate_color = fail_color(t.fail_rate);
    let count = format!("{}", t.total);
    // Trim the trailing count if the panel is very narrow.
    let mut spans = vec![
        Span::raw("  "),
        Span::styled(name, Style::default().fg(theme::TEXT_PRIMARY)),
        Span::raw(" "),
        Span::styled(format!("{bar:<bar_w$}"), Style::default().fg(theme::GREEN)),
        Span::raw(" "),
        Span::styled(count, Style::default().fg(theme::TEXT_SECONDARY)),
    ];
    if w >= 34 && t.failures > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("{:.0}%", t.fail_rate),
            Style::default().fg(rate_color),
        ));
    }
    Line::from(spans)
}

fn fail_row(t: &McToolStat, w: usize) -> Line<'static> {
    let name_w = w.saturating_sub(12).clamp(8, 22);
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            pad(&trunc(&t.name, name_w), name_w),
            Style::default().fg(theme::TEXT_PRIMARY),
        ),
        Span::styled(
            format!("{:>5.1}%", t.fail_rate),
            Style::default().fg(fail_color(t.fail_rate)),
        ),
    ])
}

fn kv_row(name: &str, value: String, w: usize) -> Line<'static> {
    let name_w = w.saturating_sub(value.chars().count() + 4).clamp(8, 24);
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            pad(&trunc(name, name_w), name_w),
            Style::default().fg(theme::TEXT_PRIMARY),
        ),
        Span::styled(value, Style::default().fg(theme::TEXT_SECONDARY)),
    ])
}

fn fail_color(rate: f64) -> Color {
    if rate >= 25.0 {
        Color::Red
    } else if rate >= 10.0 {
        theme::ORANGE
    } else {
        theme::TEXT_SECONDARY
    }
}

fn pad(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n >= w {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(w - n))
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
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
