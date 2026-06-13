//! Mission Control panel layout — pure function, no rendering.
//!
//! The MC takes a single outer rect (full content area) and partitions
//! it into four panels. The left column stacks Inbox, Activity and Schedule;
//! Analytics takes the full-height right column so its dense bars and tables
//! have room:
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │ ┌ Inbox (40% × 22%) ─┐ ┌ Analytics ────────┐ │
//! │ ├────────────────────┤ │ (60% full height) │ │
//! │ │ Activity (× 48%)   │ │                   │ │
//! │ ├────────────────────┤ │                   │ │
//! │ │ Schedule (× 30%)   │ │                   │ │
//! │ └────────────────────┘ └───────────────────┘ │
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

    // Left column: Inbox (small, usually short) over the Activity feed and the
    // Schedule. Right column: Analytics, full height, so its bars and tables
    // get the room the dense content needs.
    let left_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(22),
            Constraint::Percentage(48),
            Constraint::Percentage(30),
        ])
        .split(cols[0]);

    McLayout {
        inbox: left_rows[0],
        activity: left_rows[1],
        schedule: left_rows[2],
        analytics: cols[1],
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
