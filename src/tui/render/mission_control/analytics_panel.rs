//! Analytics panel (full-height right column). Brain sizes plus tool usage /
//! reliability and RSI application counts, read from the cached
//! `app.mc.analytics` snapshot. Read-only: the renderer never hits disk or the
//! DB itself.
//!
//! This is the native home for what the external `opencrabs-analytics` HTML
//! tool produced (discussion #178).
//!
//! The content is laid out responsively across two inner columns so it fills
//! the wide panel: tool-usage (with bars scaled to the column width) on the
//! left, reliability / RSI / brain sizes (values right-aligned to the edge) on
//! the right. Both adapt to the panel size.

use super::theme;
use crate::brain::mission_control::{McAnalytics, McToolStat};
use crate::tui::app::App;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn draw(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let a = &app.mc.analytics;
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

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Narrow panels (or a stray resize) fall back to a single column so the
    // two-column split never produces unreadable slivers.
    if inner.width < 56 {
        let lines = left_column(a, inner.width as usize)
            .into_iter()
            .chain(std::iter::once(Line::from("")))
            .chain(right_column(a, inner.width as usize))
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(54), Constraint::Percentage(46)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(left_column(a, cols[0].width as usize)),
        cols[0],
    );
    frame.render_widget(
        Paragraph::new(right_column(a, cols[1].width as usize)),
        cols[1],
    );
}

/// Summary + top tools (with proportional bars sized to the column width).
fn left_column(a: &McAnalytics, w: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
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
        for t in a.top_tools.iter().take(10) {
            lines.push(tool_bar_row(t, max, w));
        }
    }
    lines
}

/// Flakiest tools, RSI by dimension, and brain files, with values
/// right-aligned to the column's right edge so the width is filled.
fn right_column(a: &McAnalytics, w: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    if !a.flakiest_tools.is_empty() {
        lines.push(header("Flakiest (≥5 calls)"));
        for t in a.flakiest_tools.iter().take(8) {
            lines.push(value_row(
                &t.name,
                format!("{:.1}%", t.fail_rate),
                fail_color(t.fail_rate),
                w,
            ));
        }
        lines.push(blank());
    }
    if !a.rsi_top_dimensions.is_empty() {
        lines.push(header("RSI applied by dimension"));
        for (dim, n) in a.rsi_top_dimensions.iter().take(8) {
            lines.push(value_row(dim, n.to_string(), theme::GREEN, w));
        }
        lines.push(blank());
    }
    if !a.brain_files.is_empty() {
        lines.push(header("Brain files"));
        for f in a.brain_files.iter().take(16) {
            lines.push(value_row(
                &f.name,
                format!("{:.1} KB", f.kb),
                theme::TEXT_SECONDARY,
                w,
            ));
        }
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

/// `name  ███████▌ count rate%` with the bar filling the column width.
fn tool_bar_row(t: &McToolStat, max: i64, w: usize) -> Line<'static> {
    let name_w = 12usize;
    let count_w = 8usize; // "  24304 " style trailing block
    // Whatever is left after the name, count, and a 4% rate column is the bar.
    let bar_w = w.saturating_sub(name_w + count_w + 6).clamp(4, 40);
    let filled = ((t.total as f64 / max as f64) * bar_w as f64).round() as usize;
    let bar: String = "█".repeat(filled.min(bar_w));
    Line::from(vec![
        Span::raw("  "),
        Span::styled(
            pad(&trunc(&t.name, name_w), name_w),
            Style::default().fg(theme::TEXT_PRIMARY),
        ),
        Span::styled(format!("{bar:<bar_w$}"), Style::default().fg(theme::GREEN)),
        Span::styled(
            format!("{:>6}", t.total),
            Style::default().fg(theme::TEXT_SECONDARY),
        ),
        Span::styled(
            format!(" {:>3.0}%", t.fail_rate),
            Style::default().fg(fail_color(t.fail_rate)),
        ),
    ])
}

/// `name<spaces>value` with `value` flush against the right edge of width `w`.
fn value_row(name: &str, value: String, value_color: Color, w: usize) -> Line<'static> {
    let value_len = value.chars().count();
    let name_room = w.saturating_sub(value_len + 3);
    let name = trunc(name, name_room.max(4));
    let gap = w
        .saturating_sub(name.chars().count() + value_len + 2)
        .max(1);
    Line::from(vec![
        Span::raw("  "),
        Span::styled(name, Style::default().fg(theme::TEXT_PRIMARY)),
        Span::raw(" ".repeat(gap)),
        Span::styled(value, Style::default().fg(value_color)),
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
