//! Analytics panel (full-height right column). Brain sizes plus tool usage /
//! reliability, RSI application counts, and the #897 provider/model-agnostic
//! stats (phantom detection, streaming recoveries, brain-verify gate, per-model
//! reliability), read from the cached `app.mc.analytics` snapshot. Read-only:
//! the renderer never hits disk or the DB itself.
//!
//! This is the native home for what the external `opencrabs-analytics` HTML
//! tool produced (discussion #178).
//!
//! Layout is a single full-width vertical stack so it reads at any terminal
//! width (80-col or 120-col): the D/W/M/All filter tabs are pinned on the top
//! line (#900) and the sections scroll beneath them. Section order: Totals
//! (+ recovery/verify counters), Model reliability, Flakiest, Phantoms, RSI
//! applied, Brain files, Top tools.

use super::theme;
use crate::brain::mission_control::{McAnalytics, McToolStat, TimeWindow};
use crate::tui::app::App;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn draw(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    render(
        frame,
        &app.mc.analytics,
        app.mc.analytics_window,
        app.mc.scroll_offset,
        area,
        focused,
    );
}

/// Render the analytics view (block + pinned filter tabs + scrolling section
/// stack) into any rect. Shared by the Mission Control panel and the Enter
/// detail popup, so the popup is the same rich view, just larger.
pub(crate) fn render(
    frame: &mut Frame,
    a: &McAnalytics,
    window: TimeWindow,
    scroll: u16,
    area: Rect,
    focused: bool,
) {
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

    // Pin the D/W/M/All filter tabs on the top line; the sections scroll
    // beneath them (#900). A single full-width stack works at any width —
    // sections truncate to fit and overflow scrolls rather than collapsing
    // into unreadable slivers on narrow terminals.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    frame.render_widget(Paragraph::new(tabs_line(window)), rows[0]);

    let w = rows[1].width as usize;
    let sections = [
        summary_lines(a),
        model_comparison_lines(a, w),
        flakiest_lines(a, w, window),
        phantom_lines(a, w),
        rsi_lines(a, w),
        brain_lines(a, w),
        top_tools_lines(a, w),
    ];
    let mut body: Vec<Line<'static>> = Vec::new();
    for section in sections {
        if section.is_empty() {
            continue;
        }
        if !body.is_empty() {
            body.push(blank());
        }
        body.extend(section);
    }

    frame.render_widget(Paragraph::new(body).scroll((scroll, 0)), rows[1]);
}

/// D/W/M/All filter tabs, active tab highlighted in the accent color (#900).
/// A trailing `1-4` hint keeps the hotkeys discoverable.
fn tabs_line(active: TimeWindow) -> Line<'static> {
    let tabs = [
        (TimeWindow::Day, "D"),
        (TimeWindow::Week, "W"),
        (TimeWindow::Month, "M"),
        (TimeWindow::All, "All"),
    ];
    let mut spans = vec![Span::raw(" ")];
    for (i, (window, label)) in tabs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        let style = if *window == active {
            Style::default()
                .fg(theme::BORDER_ANALYTICS_FOCUS)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_DIM)
        };
        spans.push(Span::styled(format!("[{label}]"), style));
    }
    spans.push(Span::styled(
        "  1-4 to switch",
        Style::default().fg(theme::TEXT_DIM),
    ));
    Line::from(spans)
}

/// Totals quadrant: calls / fails / recovery + verify counters / RSI / brain.
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
        summary_row("Recov", format!("{} recovered", a.streaming.total)),
        summary_row(
            "Verify",
            format!(
                "{} ok / {} rollbk",
                a.brain_verify.passes, a.brain_verify.rollbacks
            ),
        ),
        summary_row("RSI", format!("{} applied", a.rsi_applied_total)),
        summary_row(
            "RSI live",
            crate::brain::mission_control::staleness::rsi_staleness_line(
                chrono::Utc::now().timestamp(),
                a.rsi_last_call_ts,
                a.tool_events_since_rsi,
            ),
        ),
        summary_row(
            "Brain",
            format!("{:.1} KB / {} files", a.brain_total_kb, a.brain_files.len()),
        ),
    ]
}

/// Model comparison: per-model fail rate + phantom rate, most calls first
/// (#900). Provider-agnostic — shows the model name.
fn model_comparison_lines(a: &McAnalytics, w: usize) -> Vec<Line<'static>> {
    if a.model_tools.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![header("Model reliability")];
    for m in a.model_tools.iter().take(8) {
        // Join the per-model phantom count onto the model's call volume; a
        // model with no recorded phantoms reads as 0%.
        let phantoms = a
            .phantom
            .by_model
            .iter()
            .find(|(name, _, _)| name == &m.model)
            .map(|(_, total, _)| *total)
            .unwrap_or(0);
        let phantom_rate = if m.total > 0 {
            (phantoms as f64 / m.total as f64) * 100.0
        } else {
            0.0
        };
        lines.push(model_row(&m.model, m.fail_rate, phantom_rate, w));
    }
    lines
}

/// Flakiest quadrant: highest failure rates, values right-aligned to `w`.
/// The window label tracks the active D/W/M/All filter (#900).
fn flakiest_lines(a: &McAnalytics, w: usize, window: TimeWindow) -> Vec<Line<'static>> {
    if a.flakiest_tools.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![header(&format!(
        "Flakiest (≥5 calls, {})",
        window_label(window)
    ))];
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

/// Phantom panel: detected / resolved / ratio plus a per-model breakdown
/// (#900). Sits below Flakiest in the stack.
fn phantom_lines(a: &McAnalytics, w: usize) -> Vec<Line<'static>> {
    if a.phantom.total == 0 {
        return Vec::new();
    }
    let mut lines = vec![header("Phantoms")];
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!(
                "{} detected, {} resolved ({:.1}%)",
                a.phantom.total, a.phantom.resolved, a.phantom.resolved_pct
            ),
            Style::default().fg(theme::GREEN),
        ),
    ]));
    for (model, total, resolved) in a.phantom.by_model.iter().take(6) {
        lines.push(value_row(
            model,
            format!("{total} ({resolved} resolved)"),
            theme::TEXT_SECONDARY,
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

/// `model<spaces>fail X%  ph Y%` with the values flush against the right edge.
fn model_row(model: &str, fail_rate: f64, phantom_rate: f64, w: usize) -> Line<'static> {
    let fail = format!("fail {:.1}%", fail_rate);
    let ph = format!("  ph {:.1}%", phantom_rate);
    let value_len = fail.chars().count() + ph.chars().count();
    let name_room = w.saturating_sub(value_len + 3);
    let name = trunc(model, name_room.max(4));
    let gap = w
        .saturating_sub(name.chars().count() + value_len + 2)
        .max(1);
    Line::from(vec![
        Span::raw("  "),
        Span::styled(name, Style::default().fg(theme::TEXT_PRIMARY)),
        Span::raw(" ".repeat(gap)),
        Span::styled(fail, Style::default().fg(fail_color(fail_rate))),
        Span::styled(ph, Style::default().fg(theme::TEXT_SECONDARY)),
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

fn window_label(window: TimeWindow) -> &'static str {
    match window {
        TimeWindow::Day => "24h",
        TimeWindow::Week => "7d",
        TimeWindow::Month => "30d",
        TimeWindow::All => "all-time",
    }
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
