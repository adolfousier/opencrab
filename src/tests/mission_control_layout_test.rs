//! Tests for `compute` — the pure layout function for Mission Control.
//!
//! Contract: the inbox / activity / schedule / help_bar rects must:
//!   1. Stay strictly inside the outer area.
//!   2. Not overlap each other.
//!   3. Reserve exactly 2 rows for the help bar when the area can spare it.
//!   4. Collapse the help bar to 0-height when the area is too short to
//!      spare those 2 rows.

use crate::tui::render::mission_control::{McLayout, compute};
use ratatui::layout::Rect;

fn rect_inside(child: Rect, parent: Rect) -> bool {
    child.x >= parent.x
        && child.y >= parent.y
        && child.x + child.width <= parent.x + parent.width
        && child.y + child.height <= parent.y + parent.height
}

fn rects_overlap(a: Rect, b: Rect) -> bool {
    let a_right = a.x + a.width;
    let b_right = b.x + b.width;
    let a_bottom = a.y + a.height;
    let b_bottom = b.y + b.height;
    !(a_right <= b.x || b_right <= a.x || a_bottom <= b.y || b_bottom <= a.y)
}

#[test]
fn every_panel_stays_inside_outer_area() {
    let outer = Rect::new(0, 0, 154, 50);
    let layout: McLayout = compute(outer);
    assert!(rect_inside(layout.inbox, outer), "inbox escaped outer");
    assert!(
        rect_inside(layout.analytics, outer),
        "analytics escaped outer"
    );
    assert!(
        rect_inside(layout.activity, outer),
        "activity escaped outer"
    );
    assert!(
        rect_inside(layout.schedule, outer),
        "schedule escaped outer"
    );
    assert!(
        rect_inside(layout.help_bar, outer),
        "help_bar escaped outer"
    );
}

#[test]
fn panels_do_not_overlap() {
    let outer = Rect::new(0, 0, 154, 50);
    let layout = compute(outer);
    assert!(
        !rects_overlap(layout.inbox, layout.activity),
        "inbox/activity overlap"
    );
    assert!(
        !rects_overlap(layout.inbox, layout.analytics),
        "inbox/analytics overlap"
    );
    assert!(
        !rects_overlap(layout.analytics, layout.activity),
        "analytics/activity overlap"
    );
    assert!(
        !rects_overlap(layout.analytics, layout.schedule),
        "analytics/schedule overlap"
    );
    assert!(
        !rects_overlap(layout.analytics, layout.help_bar),
        "analytics overlaps help bar"
    );
    assert!(
        !rects_overlap(layout.inbox, layout.schedule),
        "inbox/schedule overlap"
    );
    assert!(
        !rects_overlap(layout.activity, layout.schedule),
        "activity/schedule overlap"
    );
    // Panels must not overlap the help bar either.
    assert!(
        !rects_overlap(layout.inbox, layout.help_bar),
        "inbox overlaps help bar"
    );
    assert!(
        !rects_overlap(layout.activity, layout.help_bar),
        "activity overlaps help bar"
    );
    assert!(
        !rects_overlap(layout.schedule, layout.help_bar),
        "schedule overlaps help bar"
    );
}

#[test]
fn help_bar_takes_exactly_two_rows_when_area_is_tall_enough() {
    let outer = Rect::new(0, 0, 100, 30);
    let layout = compute(outer);
    assert_eq!(layout.help_bar.height, 2);
    // Help bar sits at the very bottom two rows.
    assert_eq!(layout.help_bar.y, outer.y + outer.height - 2);
    assert_eq!(layout.help_bar.x, outer.x);
    assert_eq!(layout.help_bar.width, outer.width);
}

#[test]
fn help_bar_collapses_to_zero_when_area_is_too_short() {
    // A 2-row area can't spare the 2-row bar — panels take the whole height.
    let outer = Rect::new(0, 0, 100, 2);
    let layout = compute(outer);
    assert_eq!(layout.help_bar.height, 0);
}

#[test]
fn inbox_takes_left_40_percent() {
    let outer = Rect::new(0, 0, 100, 30);
    let layout = compute(outer);
    assert_eq!(layout.inbox.x, 0);
    // Allow 1-cell rounding tolerance from ratatui's percentage split.
    assert!(
        (layout.inbox.width as i32 - 40).abs() <= 1,
        "inbox width was {}, expected ~40",
        layout.inbox.width
    );
}

#[test]
fn analytics_takes_full_right_column_left_stacks_three() {
    let outer = Rect::new(0, 0, 100, 30);
    let layout = compute(outer);
    let panel_height = outer.height - layout.help_bar.height;
    // Analytics spans the full panel height on the right.
    assert_eq!(
        layout.analytics.height, panel_height,
        "analytics should be full height"
    );
    // Inbox + Activity + Schedule stack to fill the left column.
    let left_height = layout.inbox.height + layout.activity.height + layout.schedule.height;
    assert_eq!(
        left_height, panel_height,
        "left column should fill the panel height"
    );
    // Analytics sits to the right of the left column.
    assert!(
        layout.analytics.x >= layout.inbox.x + layout.inbox.width,
        "analytics should be right of the left column"
    );
}

#[test]
fn handles_zero_area_without_panic() {
    // Pathological case: terminal mid-resize. compute must not panic.
    let outer = Rect::new(0, 0, 0, 0);
    // The assertion is simply that this call returns without panicking.
    let _ = compute(outer);
}
