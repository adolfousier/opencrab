//! #728: expanding/collapsing a block via click must keep the clicked header on
//! the same screen row instead of jumping the viewport. `anchored_scroll_offset`
//! turns the anchored message's line position into the from-bottom scroll offset
//! that pins it.

use crate::tui::render::chat::anchored_scroll_offset;

#[test]
fn header_stays_on_its_row_after_growth() {
    // Header sits on screen row 3. After expanding, its first line is at index
    // 40 in a transcript whose max scroll is 100. To keep it on row 3 the top
    // visible line must be 37, i.e. 63 lines up from the bottom.
    let (offset, auto) = anchored_scroll_offset(40, 3, 100);
    assert_eq!(offset, 63, "offset pins the header to its row");
    assert!(!auto, "not at the bottom, so auto-scroll stays off");
}

#[test]
fn anchor_at_top_scrolls_all_the_way_up() {
    // Header is the very first line and should sit on row 0 -> top of buffer,
    // which is the maximum from-bottom offset.
    let (offset, auto) = anchored_scroll_offset(0, 0, 100);
    assert_eq!(offset, 100);
    assert!(!auto);
}

#[test]
fn anchor_resolving_to_bottom_rearms_auto_scroll() {
    // Collapsing near the bottom: the desired top (line_top - row) meets or
    // exceeds max_scroll, so we sit at the bottom and re-arm auto-scroll.
    let (offset, auto) = anchored_scroll_offset(100, 0, 40);
    assert_eq!(offset, 0, "clamped to the bottom");
    assert!(
        auto,
        "at the bottom, auto-scroll re-arms so streaming sticks"
    );
}

#[test]
fn row_below_line_top_saturates_without_panic() {
    // Defensive: an anchor row larger than the line index must not underflow.
    let (offset, auto) = anchored_scroll_offset(2, 10, 50);
    assert_eq!(offset, 50, "desired top saturates to 0 -> full scroll up");
    assert!(!auto);
}
