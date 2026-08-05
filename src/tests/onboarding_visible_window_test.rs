//! Regression (#948): the provider list is windowed, so the fields below it
//! stay on screen.
//!
//! The provider step rendered every entry — built-ins, every configured custom
//! provider, and "+ New Custom Provider". On an 80x24 terminal, which is the
//! default and what a zoomed-in display gives you, that filled the box and
//! pushed the model and API-key fields off screen where they could not be seen
//! or reached. The list only grows: each custom provider adds a row.
//!
//! The model list on the same step was already windowed (#916). The arithmetic
//! now lives in one place so a new list cannot be added without it.

use crate::tui::onboarding_render::visible_window;

/// Rows the provider list is allowed to occupy, mirroring the render constant.
const MAX: usize = 8;

#[test]
fn a_list_that_fits_is_shown_whole() {
    // No affordances, no truncation — a short list must look exactly as it did
    // before windowing existed.
    assert_eq!(visible_window(5, 0, MAX), (0, 5));
    assert_eq!(visible_window(5, 4, MAX), (0, 5));
    assert_eq!(visible_window(MAX, 3, MAX), (0, MAX));
}

#[test]
fn an_empty_list_is_an_empty_window() {
    assert_eq!(visible_window(0, 0, MAX), (0, 0));
}

#[test]
fn a_long_list_is_capped_at_the_window() {
    let (start, end) = visible_window(20, 0, MAX);
    assert_eq!(end - start, MAX, "never more rows than the cap");
}

#[test]
fn the_selection_is_always_inside_the_window() {
    // The property that matters: arrowing through must never scroll the cursor
    // out of view, at any position in any list length.
    for total in [1usize, 7, 8, 9, 18, 50] {
        for selected in 0..total {
            let (start, end) = visible_window(total, selected, MAX);
            assert!(
                selected >= start && selected < end,
                "selection {selected} fell outside [{start},{end}) for total {total}"
            );
            assert!(end <= total, "window ran past the end for total {total}");
            assert!(end - start <= MAX, "window exceeded the cap");
        }
    }
}

#[test]
fn the_last_item_does_not_leave_a_window_of_blanks() {
    // Centring alone would put the window past the end and render empty rows.
    let (start, end) = visible_window(18, 17, MAX);
    assert_eq!(end, 18, "the window must stop at the end of the list");
    assert_eq!(end - start, MAX, "and still be full");
}

#[test]
fn the_first_item_keeps_the_window_at_the_top() {
    let (start, end) = visible_window(18, 0, MAX);
    assert_eq!((start, end), (0, MAX));
}

#[test]
fn the_selection_is_centred_in_the_middle_of_a_long_list() {
    // Context on both sides, so the user can see where they are.
    let (start, end) = visible_window(18, 9, MAX);
    assert!(start > 0 && end < 18, "expected truncation both ways");
    assert_eq!(start, 9 - MAX / 2);
}

#[test]
fn a_zero_width_window_asks_for_nothing() {
    // Guards the `total - max` clamp against underflow if a caller ever passes 0.
    assert_eq!(visible_window(10, 5, 0), (0, 10));
}

#[test]
fn a_stock_provider_list_leaves_room_for_the_rest_of_the_form() {
    // The reported case: ~18 providers on an 80x24 terminal. The window must
    // leave the model and key fields somewhere to go.
    const PROVIDERS: usize = 18;
    // Worst case for height: a selection with truncation on both sides, so both
    // scroll hints render alongside a full window.
    let (start, end) = visible_window(PROVIDERS, PROVIDERS / 2, MAX);
    let hints = usize::from(start > 0) + usize::from(end < PROVIDERS);
    let rows = (end - start) + hints;
    assert_eq!(hints, 2, "expected truncation both ways at the midpoint");
    assert!(
        rows <= 10,
        "the list plus its scroll hints took {rows} of ~20 usable rows in an \
         80x24 box, leaving nothing for the model and key fields below"
    );
}
