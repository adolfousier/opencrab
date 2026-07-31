//! Tests for the Mission Control analytics panel renderer (#900).
//!
//! Renders the real `analytics_panel::render` into a ratatui `TestBackend`
//! buffer and asserts on the produced cells, plus drives the pure `decide`
//! fn to verify the D/W/M/All filter switching and scroll behaviour that
//! feed the renderer. Covers: the pinned filter tabs (active tab bold),
//! the recovery / verify counters in Totals, the per-model reliability
//! rows, the phantom panel, the window label tracking the active filter,
//! empty-section hiding, body scrolling, and no layout regression at
//! 80-col and 120-col.

use crate::brain::mission_control::types::{
    McAnalytics, McBrainFile, McBrainVerifyStats, McModelToolStat, McPhantomStats,
    McStreamingStats, McToolStat, TimeWindow,
};
use crate::tui::app::mission_control::McPanel;
use crate::tui::app::mission_control::McState;
use crate::tui::app::mission_control::input::{KeyOutcome, decide};
use crate::tui::render::mission_control::render_analytics;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::empty())
}

/// Render the analytics panel into a fresh buffer and return it.
fn render_buf(a: &McAnalytics, window: TimeWindow, scroll: u16, w: u16, h: u16) -> Buffer {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    let area = Rect::new(0, 0, w, h);
    terminal
        .draw(|f| render_analytics(f, a, window, scroll, area, true))
        .unwrap();
    terminal.backend().buffer().clone()
}

/// Whole buffer flattened to one string (rows joined by '\n') for `.contains`.
fn buf_text(buf: &Buffer) -> String {
    let area = buf.area;
    (0..area.height)
        .map(|row| row_text(buf, row, area.width))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract the text of a single buffer row.
fn row_text(buf: &Buffer, row: u16, width: u16) -> String {
    (0..width)
        .map(|col| {
            buf.cell((col, row))
                .map(|c| c.symbol().to_string())
                .unwrap_or_default()
        })
        .collect::<String>()
}

/// A fixture with every section populated so render exercises all builders.
fn full_fixture() -> McAnalytics {
    McAnalytics {
        tool_total_calls: 1000,
        tool_total_fails: 50,
        top_tools: vec![McToolStat {
            name: "bash".into(),
            total: 500,
            failures: 10,
            fail_rate: 2.0,
        }],
        flakiest_tools: vec![McToolStat {
            name: "flaky_tool".into(),
            total: 10,
            failures: 5,
            fail_rate: 50.0,
        }],
        rsi_applied_total: 12,
        rsi_top_dimensions: vec![("voice".into(), 4)],
        brain_files: vec![McBrainFile {
            name: "MEMORY.md".into(),
            kb: 8.0,
        }],
        brain_total_kb: 8.0,
        phantom: McPhantomStats {
            total: 20,
            resolved: 15,
            resolved_pct: 75.0,
            by_model: vec![("claude-opus-4".into(), 10, 8)],
        },
        streaming: McStreamingStats {
            total: 7,
            ..Default::default()
        },
        brain_verify: McBrainVerifyStats {
            passes: 42,
            rollbacks: 3,
            ..Default::default()
        },
        model_tools: vec![McModelToolStat {
            model: "claude-opus-4".into(),
            total: 100,
            failures: 5,
            fail_rate: 5.0,
        }],
        ..Default::default()
    }
}

// ── Filter tabs ─────────────────────────────────────────────────────────────

#[test]
fn tabs_render_all_four_windows_with_hotkey_hint() {
    let buf = render_buf(&McAnalytics::default(), TimeWindow::Month, 0, 80, 24);
    let text = buf_text(&buf);
    assert!(text.contains("[D]"), "missing Day tab");
    assert!(text.contains("[W]"), "missing Week tab");
    assert!(text.contains("[M]"), "missing Month tab");
    assert!(text.contains("[All]"), "missing All tab");
    assert!(text.contains("1-4 to switch"), "missing hotkey hint");
}

#[test]
fn active_tab_is_bold_others_are_not() {
    // Month active: the [M] cells carry BOLD, the [D]/[W]/[All] cells do not.
    let buf = render_buf(&McAnalytics::default(), TimeWindow::Month, 0, 80, 24);
    // Tabs render on the first inner row (y=1, below the top border).
    let mut d_bold = false;
    let mut m_bold = false;
    for col in 0..80u16 {
        if let Some(cell) = buf.cell((col, 1)) {
            let bold = cell.style().add_modifier.contains(Modifier::BOLD);
            match cell.symbol() {
                "D" => d_bold = bold,
                "M" => m_bold = bold,
                _ => {}
            }
        }
    }
    assert!(m_bold, "active Month tab should be bold");
    assert!(!d_bold, "inactive Day tab should not be bold");
}

// ── Totals: recovery + verify counters ──────────────────────────────────────

#[test]
fn totals_show_recovery_and_verify_counters() {
    let a = McAnalytics {
        tool_total_calls: 100,
        tool_total_fails: 5,
        streaming: McStreamingStats {
            total: 7,
            ..Default::default()
        },
        brain_verify: McBrainVerifyStats {
            passes: 42,
            rollbacks: 3,
            ..Default::default()
        },
        ..Default::default()
    };
    let text = buf_text(&render_buf(&a, TimeWindow::Month, 0, 80, 30));
    assert!(text.contains("Totals"), "missing Totals header");
    assert!(text.contains("100 calls"), "missing tool call total");
    assert!(text.contains("5 (5.0%)"), "missing fail count/rate");
    assert!(text.contains("7 recovered"), "missing recovery counter");
    assert!(text.contains("42 ok / 3 rollbk"), "missing verify counter");
}

// ── Model comparison ────────────────────────────────────────────────────────

#[test]
fn model_comparison_row_shows_fail_and_phantom_rate() {
    let a = McAnalytics {
        model_tools: vec![McModelToolStat {
            model: "claude-opus-4".into(),
            total: 100,
            failures: 5,
            fail_rate: 5.0,
        }],
        phantom: McPhantomStats {
            total: 10,
            resolved: 8,
            resolved_pct: 80.0,
            by_model: vec![("claude-opus-4".into(), 10, 8)],
        },
        ..Default::default()
    };
    let text = buf_text(&render_buf(&a, TimeWindow::Month, 0, 80, 30));
    assert!(text.contains("Model reliability"), "missing model header");
    assert!(text.contains("claude-opus-4"), "missing model name");
    assert!(text.contains("fail 5.0%"), "missing fail rate");
    // phantom_rate = 10 phantoms / 100 calls = 10.0%
    assert!(text.contains("ph 10.0%"), "missing phantom rate");
}

// ── Phantom panel ───────────────────────────────────────────────────────────

#[test]
fn phantom_panel_renders_detected_resolved_and_per_model() {
    let a = McAnalytics {
        phantom: McPhantomStats {
            total: 20,
            resolved: 15,
            resolved_pct: 75.0,
            by_model: vec![("gpt-5".into(), 12, 9)],
        },
        ..Default::default()
    };
    let text = buf_text(&render_buf(&a, TimeWindow::Month, 0, 80, 30));
    assert!(text.contains("Phantoms"), "missing Phantoms header");
    assert!(
        text.contains("20 detected, 15 resolved (75.0%)"),
        "missing detected/resolved line"
    );
    assert!(text.contains("gpt-5"), "missing per-model name");
    assert!(text.contains("12 (9 resolved)"), "missing per-model counts");
}

// ── D/W/M label tracks the active window ────────────────────────────────────

#[test]
fn flakiest_header_label_tracks_active_window() {
    let a = McAnalytics {
        flakiest_tools: vec![McToolStat {
            name: "flaky_tool".into(),
            total: 10,
            failures: 5,
            fail_rate: 50.0,
        }],
        ..Default::default()
    };
    let cases = [
        (TimeWindow::Day, "24h"),
        (TimeWindow::Week, "7d"),
        (TimeWindow::Month, "30d"),
        (TimeWindow::All, "all-time"),
    ];
    for (window, label) in cases {
        let text = buf_text(&render_buf(&a, window, 0, 80, 30));
        assert!(text.contains("Flakiest"), "missing Flakiest header");
        assert!(
            text.contains(label),
            "window {window:?} should label flakiest as {label}"
        );
    }
}

// ── Empty sections are hidden ───────────────────────────────────────────────

#[test]
fn empty_sections_are_hidden_but_totals_and_tabs_remain() {
    let text = buf_text(&render_buf(
        &McAnalytics::default(),
        TimeWindow::Month,
        0,
        80,
        40,
    ));
    // Totals + tabs always render.
    assert!(text.contains("Totals"), "Totals should always render");
    assert!(text.contains("[M]"), "tabs should always render");
    // Empty sections must not render their headers.
    for absent in [
        "Model reliability",
        "Phantoms",
        "Flakiest",
        "Top tools",
        "Brain files",
        "RSI applied by dimension",
    ] {
        assert!(
            !text.contains(absent),
            "{absent} should be hidden when empty"
        );
    }
}

// ── Body scrolling ──────────────────────────────────────────────────────────

#[test]
fn scroll_offset_shifts_body_up() {
    // Default fixture: only the Totals section renders, so the body is a
    // predictable stack (Totals, Tools, Fails, Recov, Verify, RSI, ...).
    let a = McAnalytics::default();
    // rows[0] is the pinned tabs (y=1); the body starts at y=2.
    let at0 = row_text(&render_buf(&a, TimeWindow::Month, 0, 80, 30), 2, 80);
    assert!(
        at0.contains("Totals"),
        "body top at scroll=0 should be Totals"
    );

    let at2 = row_text(&render_buf(&a, TimeWindow::Month, 2, 80, 30), 2, 80);
    assert!(
        !at2.contains("Totals"),
        "scrolling by 2 should move Totals off the top body row"
    );
    assert!(
        at2.contains("Fails"),
        "scrolling by 2 should bring Fails to the top body row"
    );
}

// ── No layout regression at 80-col and 120-col ──────────────────────────────

#[test]
fn renders_at_80_and_120_col_without_panic() {
    let a = full_fixture();
    for width in [80u16, 120] {
        let buf = render_buf(&a, TimeWindow::Month, 0, width, 24);
        let text = buf_text(&buf);
        assert!(text.contains("[All]"), "tabs should render at {width}-col");
        assert!(
            text.contains("Totals"),
            "Totals should render at {width}-col"
        );
    }
}

// ── decide(): D/W/M/All filter switching + scroll ───────────────────────────

#[test]
fn digit_keys_switch_window_when_analytics_focused() {
    let mut s = McState {
        focused_panel: McPanel::Analytics,
        ..Default::default()
    };
    // Default window is Month.
    assert_eq!(s.analytics_window, TimeWindow::Month);

    let cases = [
        (KeyCode::Char('1'), TimeWindow::Day),
        (KeyCode::Char('2'), TimeWindow::Week),
        (KeyCode::Char('3'), TimeWindow::Month),
        (KeyCode::Char('4'), TimeWindow::All),
    ];
    for (code, want) in cases {
        let out = decide(&mut s, 5, key(code));
        // Each target window differs from the current one in this sequence,
        // so every press is a genuine change (the no-op case is covered by
        // `same_window_key_is_consumed_not_changed`).
        assert_eq!(out, KeyOutcome::AnalyticsWindowChanged, "key {code:?}");
        assert_eq!(s.analytics_window, want, "key {code:?}");
    }
}

#[test]
fn same_window_key_is_consumed_not_changed() {
    let mut s = McState {
        focused_panel: McPanel::Analytics,
        analytics_window: TimeWindow::Day,
        ..Default::default()
    };
    let out = decide(&mut s, 5, key(KeyCode::Char('1')));
    assert_eq!(out, KeyOutcome::Consumed);
    assert_eq!(s.analytics_window, TimeWindow::Day);
}

#[test]
fn digit_keys_are_ignored_outside_analytics() {
    let mut s = McState {
        focused_panel: McPanel::Inbox,
        ..Default::default()
    };
    let out = decide(&mut s, 5, key(KeyCode::Char('1')));
    assert_eq!(out, KeyOutcome::NotConsumed);
    assert_eq!(
        s.analytics_window,
        TimeWindow::Month,
        "window must not change"
    );
}

#[test]
fn scroll_keys_move_offset_in_analytics() {
    let mut s = McState {
        focused_panel: McPanel::Analytics,
        ..Default::default()
    };
    assert_eq!(s.scroll_offset, 0);
    assert_eq!(
        decide(&mut s, 5, key(KeyCode::Char('j'))),
        KeyOutcome::Consumed
    );
    assert_eq!(s.scroll_offset, 1, "j should scroll down");
    assert_eq!(
        decide(&mut s, 5, key(KeyCode::Char('k'))),
        KeyOutcome::Consumed
    );
    assert_eq!(s.scroll_offset, 0, "k should scroll back up");
    // k at zero saturates, never underflows.
    assert_eq!(
        decide(&mut s, 5, key(KeyCode::Char('k'))),
        KeyOutcome::Consumed
    );
    assert_eq!(s.scroll_offset, 0);
}

#[test]
fn home_resets_scroll_in_analytics() {
    let mut s = McState {
        focused_panel: McPanel::Analytics,
        scroll_offset: 10,
        ..Default::default()
    };
    decide(&mut s, 5, key(KeyCode::Home));
    assert_eq!(s.scroll_offset, 0, "Home should reset scroll");
}
