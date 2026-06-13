//! Mission Control panel layout — pure function, no rendering.
//!
//! The MC takes a single outer rect (full content area) and partitions
//! it into four panels. The left column splits into a small Inbox (often
//! empty) over an Analytics panel that reclaims the formerly idle space:
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ ┌── Inbox (40% × 30%) ┐ ┌ Activity (top) ───┐ │
//! │ ├─────────────────────┤ │ (60% × 50%)       │ │
//! │ │ Analytics           │ ├───────────────────┤ │
//! │ │ (40% × 70%)         │ │ Schedule (bottom) │ │
//! │ │                     │ │ (60% × 50%)       │ │
//! │ └─────────────────────┘ └───────────────────┘ │
//! │ help bar (1 row)                             │
//! └──────────────────────────────────────────────┘
//! ```
//!
//! `compute` is a pure fn so the geometry is unit-testable without a
//! live `Frame` — the contract is "every panel rect stays inside the
//! outer area, panels don't overlap, and the help bar takes 1 row at
//! the bottom whenever the area is tall enough".

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Partition of the MC area into named panel rects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McLayout {
    pub inbox: Rect,
    pub analytics: Rect,
    pub activity: Rect,
    pub schedule: Rect,
    pub help_bar: Rect,
}

/// Compute the panel rectangles for a Mission Control draw.
///
/// Returns zero-sized rects for the help bar when the outer area is too
/// short to spare a row. Callers should treat any zero-height rect as a
/// "skip rendering" signal.
pub fn compute(area: Rect) -> McLayout {
    // Reserve the bottom row for the help bar when there's room.
    let (panels_area, help_bar) = split_help_bar(area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(panels_area);

    // Left column: a small Inbox (usually empty) on top, Analytics below it
    // claiming the space that used to sit idle.
    let left_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(cols[0]);

    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(cols[1]);

    McLayout {
        inbox: left_rows[0],
        analytics: left_rows[1],
        activity: right_rows[0],
        schedule: right_rows[1],
        help_bar,
    }
}

/// Split the bottom row off as the help bar. Returns `(panels, help_bar)`.
/// When the area is < 2 rows, no help bar is reserved.
fn split_help_bar(area: Rect) -> (Rect, Rect) {
    if area.height < 2 {
        let empty = Rect { height: 0, ..area };
        return (area, empty);
    }
    let panels = Rect {
        height: area.height - 1,
        ..area
    };
    let help_bar = Rect {
        x: area.x,
        y: area.y + area.height - 1,
        width: area.width,
        height: 1,
    };
    (panels, help_bar)
}
