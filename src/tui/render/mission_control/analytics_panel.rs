//! Analytics panel (full-height right column). Brain sizes plus tool usage /
//! reliability and RSI application counts, read from the cached
//! `app.mc.analytics` snapshot. Read-only: the renderer never hits disk or the
//! DB itself.
//!
//! This is the native home for what the external `opencrabs-analytics` HTML
//! tool produced (discussion #178).
//!
//! Layout fills the wide panel: a 2x2 grid of the four compact sections
//! (totals, flakiest tools, RSI by dimension, brain files) on top, then Top
//! tools full-width across the bottom so its usage bars get the whole panel
//! width. Narrow panels fall back to a single stacked column.

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
    render(frame, &app.mc.analytics, area, focused);
}

/// Render the analytics view (block + 2x2 grid over a full-width Top tools
/// strip) into any rect. Shared by the Mission Control panel and the Enter
/// detail popup, so the popup is the same rich view, just larger.
pub(crate) fn render(frame: &mut Frame, a: &McAnalytics, area: Rect, focused: bool) {
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

    // Narrow panels (or a stray resize) fall back to a single stacked column so
    // the 2x2 split never produces unreadable slivers.
    if inner.width < 56 {
        let w = inner.width as usize;
        let mut lines = summary_lines(a);
        for section in [
            flakiest_lines(a, w),
            rsi_lines(a, w),
            brain_lines(a, w),
            top_tools_lines(a, w),
        ] {
            if !section.is_empty() {
                lines.push(blank());
                lines.extend(section);
            }
        }
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    // 2x2 grid (totals / flakiest / RSI / brain) takes the top ~3/4; Top tools
    // spans the full width below so its bars get the whole panel.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(72), Constraint::Percentage(28)])
        .split(inner);
    let grid_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(grid_rows[0]);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(grid_rows[1]);

    frame.render_widget(Paragraph::new(summary_lines(a)), top[0]);
    frame.render_widget(
        Paragraph::new(flakiest_lines(a, top[1].width as usize)),
        top[1],
    );
    frame.render_widget(
        Paragraph::new(rsi_lines(a, bottom[0].width as usize)),
        bottom[0],
    );
    frame.render_widget(
        Paragraph::new(brain_lines(a, bottom[1].width as usize)),
        bottom[1],
    );
    frame.render_widget(
        Paragraph::new(top_tools_lines(a, rows[1].width as usize)),
        rows[1],
    );
}

/// Totals quadrant: calls / fails / RSI / brain.
fn summary_lines(a: &McAnalytics) -> Vec<Line<'static>> {
    let fail_pct = if a.tool_total_calls > 0 {
        (a.tool_total_fails as f64 / a.tool_total_calls as f64) * 100.0
    } else {
        0.0
    };
    vec![
        header("Totals"),
        summary_row("Tools", format!("{} calls", a.tool_total_calls)),
        summary_row(
            "Fails",
            format!("{} ({:.1}%)", a.tool_total_fails, fail_pct),
        ),
        summary_row("RSI", format!("{} applied", a.rsi_applied_total)),
        summary_row(
            "Brain",
            format!("{:.1} KB / {} files", a.brain_total_kb, a.brain_files.len()),
        ),
    ]
}

/// Flakiest quadrant: highest failure rates, values right-aligned to `w`.
fn flakiest_lines(a: &McAnalytics, w: usize) -> Vec<Line<'static>> {
    if a.flakiest_tools.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![header("Flakiest (≥5 calls)")];
    for t in a.flakiest_tools.iter().take(8) {
        lines.push(value_row(
            &t.name,
            format!("{:.1}%", t.fail_rate),
            fail_color(t.fail_rate),
            w,
        ));
    }
    lines
}

/// RSI-by-dimension quadrant, counts right-aligned to `w`.
fn rsi_lines(a: &McAnalytics, w: usize) -> Vec<Line<'static>> {
    if a.rsi_top_dimensions.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![header("RSI applied by dimension")];
    for (dim, n) in a.rsi_top_dimensions.iter().take(8) {
        lines.push(value_row(dim, n.to_string(), theme::GREEN, w));
    }
    lines
}

/// Brain-files quadrant, sizes right-aligned to `w`.
fn brain_lines(a: &McAnalytics, w: usize) -> Vec<Line<'static>> {
    if a.brain_files.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![header("Brain files")];
    for f in a.brain_files.iter().take(14) {
        lines.push(value_row(
            &f.name,
            format!("{:.1} KB", f.kb),
            theme::TEXT_SECONDARY,
            w,
        ));
    }
    lines
}

/// Top tools, full-width, with proportional bars scaled to the whole panel.
fn top_tools_lines(a: &McAnalytics, w: usize) -> Vec<Line<'static>> {
    if a.top_tools.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![header("Top tools")];
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
    let bar_w = w.saturating_sub(name_w + count_w + 6).clamp(4, 60);
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
