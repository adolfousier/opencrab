//! Regression tests for the slash-autocomplete dropdown sizing math.
//!
//! Background: the dropdown is anchored at `input_area.x + 1`, so its
//! right edge (`x + width`) must stay strictly within the frame's right
//! edge. A pre-existing off-by-one in the width clamp meant a long
//! enough description (introduced when skills started shipping with
//! trigger-rich descriptions for LLM auto-invoke) inflated `width` to
//! `input_area.width` and ratatui rejected the cell write at column
//! `input_area.width` with an "index outside of buffer" panic. These
//! tests pin the contract: `width <= input_area.width - 1` for any input.

use crate::tui::render::{dropdown_dimensions, truncate_to_chars};

#[test]
fn never_overflows_when_anchored_at_x_plus_one() {
    // Frame width 154 was the value that surfaced the original panic.
    // Description char count of 200 is in the same range as the original
    // (overlong) skill descriptions.
    let lengths = vec![200, 50, 30];
    let (width, _inner, _budget) = dropdown_dimensions(15, &lengths, 154, 1);
    assert!(
        width < 154,
        "width must not push past frame right edge: width={width}"
    );
}

#[test]
fn respects_terminal_width_for_narrow_terminals() {
    // 30-col terminal: dropdown must fit in 29 cells (width - 1).
    let lengths = vec![1000];
    let (width, _, _) = dropdown_dimensions(10, &lengths, 30, 1);
    assert!(
        width <= 29,
        "narrow terminal must clamp to width - 1: width={width}"
    );
}

#[test]
fn handles_zero_input_area_width_without_panic() {
    // Pathological case during a resize: input_area.width could be 0.
    // Saturating arithmetic should produce a sane minimum without panic.
    let lengths = vec![80];
    let (width, _, _) = dropdown_dimensions(10, &lengths, 0, 1);
    assert!(width >= 1);
}

#[test]
fn responsive_grows_to_fit_short_content_without_capping_at_80() {
    // 100-char description on a wide 200-col terminal — responsive
    // sizing should let the dropdown grow past the old 80-col soft cap
    // to fit the description in full (no truncation).
    let lengths = vec![100];
    let (width, _, budget) = dropdown_dimensions(15, &lengths, 200, 1);
    // Expected: 2 (leading) + 15 (name col) + 1 + 100 (desc) + 1 (trailing) + 4 (chrome) = 123
    assert!(
        width > 80,
        "responsive sizing must allow growth past 80 on a wide terminal: width={width}"
    );
    assert!(
        budget >= 100,
        "desc budget must accommodate full 100-char description: budget={budget}"
    );
}

#[test]
fn truncates_when_content_exceeds_terminal() {
    // 500-char description on a 100-col terminal — must truncate.
    let lengths = vec![500];
    let (width, _, budget) = dropdown_dimensions(15, &lengths, 100, 1);
    assert!(width <= 99, "must clamp at terminal-1: width={width}");
    assert!(
        budget < 500,
        "desc budget must shrink below full description length when terminal is too narrow: budget={budget}"
    );
}

#[test]
fn floor_at_40_when_terminal_allows() {
    // Empty descriptions and a wide terminal — width should be at least
    // 40 cols (the minimum-usable floor).
    let lengths: Vec<usize> = vec![];
    let (width, _, _) = dropdown_dimensions(10, &lengths, 200, 1);
    assert!(width >= 40, "floor at 40 expected: width={width}");
}

#[test]
fn name_col_chars_drives_layout_alignment() {
    // Two equal descriptions but different name_col_chars → wider
    // name col → wider dropdown.
    let lengths = vec![20];
    let (width_short, _, budget_short) = dropdown_dimensions(8, &lengths, 200, 1);
    let (width_long, _, budget_long) = dropdown_dimensions(20, &lengths, 200, 1);
    assert!(
        width_long >= width_short,
        "longer name column should produce >= dropdown width"
    );
    // Budget stays the same because terminal allows it.
    assert!(budget_short >= 20);
    assert!(budget_long >= 20);
}

#[test]
fn truncate_passes_through_short_strings() {
    let s = truncate_to_chars("short", 100);
    assert_eq!(s, "short");
}

#[test]
fn truncate_appends_ellipsis_when_clipped() {
    let s = truncate_to_chars("0123456789abcdef", 10);
    assert_eq!(s.chars().count(), 10);
    assert!(s.ends_with('…'));
}

#[test]
fn truncate_zero_budget_returns_empty() {
    let s = truncate_to_chars("anything", 0);
    assert_eq!(s, "");
}

#[test]
fn truncate_one_char_budget_is_just_the_ellipsis() {
    // With a budget of 1 char, we keep 0 source chars + 1 ellipsis.
    let s = truncate_to_chars("hello world", 1);
    assert_eq!(s.chars().count(), 1);
    assert_eq!(s, "…");
}

#[test]
fn truncate_handles_multi_byte_unicode() {
    // CJK / accented chars must be counted as chars, not bytes.
    let s = truncate_to_chars("日本語のテキスト", 4);
    assert_eq!(s.chars().count(), 4);
    assert!(s.ends_with('…'));
}

// --- fit_dropdown: height clamping for short terminals ---
// 2026-05-17: users on default macOS Terminal, Ghostty, and old Windows
// Terminal (all default to 80x24) reported the TUI completely locking
// with a blank screen after pressing `/`. Only Ctrl+C escaped. Root
// cause: the slash autocomplete dropdown was sized at `count + 4` rows
// (no height clamp), so the full SLASH_COMMANDS + skills list (30+
// entries) produced a Rect that overflowed the terminal buffer.
// Ratatui panicked writing past the buffer, catch_unwind cleared the
// screen, the next frame panicked again, and the user was stuck in an
// infinite blank-screen render loop. Alacritty users escaped because
// their default window dimensions were large enough to fit the popup.
//
// `fit_dropdown` returns None instead of an oversized Rect so the
// render path skips drawing rather than panicking.

use crate::tui::render::{DropdownFit, fit_dropdown};

#[test]
fn fit_dropdown_fits_when_room_available() {
    // 10 items, 20 rows above input, 4 chrome rows: easy fit.
    let fit = fit_dropdown(10, 0, 20, 4).expect("must fit");
    assert_eq!(
        fit,
        DropdownFit {
            height: 14, // 10 items + 4 chrome
            visible_items: 10,
            scroll_offset: 0,
        }
    );
}

#[test]
fn fit_dropdown_clamps_height_to_input_y() {
    // 30 items but only 12 rows available above input → height clamped
    // to 12, visible_items = 12 - 4 = 8. The 22 invisible items are
    // reachable by scrolling.
    let fit = fit_dropdown(30, 0, 12, 4).expect("must fit");
    assert_eq!(fit.height, 12);
    assert_eq!(fit.visible_items, 8);
    assert_eq!(fit.scroll_offset, 0);
}

#[test]
fn fit_dropdown_scrolls_to_keep_selected_visible() {
    // Same 30 items + 12 rows scenario, but selected=15. The visible
    // window (8 items) must include row 15, so scroll_offset = 15-7 = 8.
    let fit = fit_dropdown(30, 15, 12, 4).expect("must fit");
    assert_eq!(fit.visible_items, 8);
    assert_eq!(fit.scroll_offset, 8);
    assert!(
        (fit.scroll_offset..fit.scroll_offset + fit.visible_items).contains(&15),
        "selected row 15 must be inside the visible window"
    );
}

#[test]
fn fit_dropdown_clamps_scroll_at_list_end() {
    // selected=29 (last item) with 8 visible items → scroll_offset = 22
    // so the window ends exactly at row 29 without overshooting.
    let fit = fit_dropdown(30, 29, 12, 4).expect("must fit");
    assert_eq!(fit.scroll_offset, 22);
    assert_eq!(fit.visible_items, 8);
    assert_eq!(fit.scroll_offset + fit.visible_items, 30);
}

#[test]
fn fit_dropdown_returns_none_when_no_room() {
    // Need at least chrome + 1 rows for one item. 4 rows is just borders+padding.
    assert_eq!(fit_dropdown(10, 0, 4, 4), None);
    assert_eq!(fit_dropdown(10, 0, 0, 4), None);
    // Empty list never renders either.
    assert_eq!(fit_dropdown(0, 0, 20, 4), None);
}

#[test]
fn fit_dropdown_macos_terminal_default_dimensions() {
    // The exact scenario reported on 2026-05-17: macOS Terminal default
    // 80x24, input at y=20 (chat + plan + queue + status above), 30
    // total slash commands (SLASH_COMMANDS + skills). Pre-fix this
    // produced a height=34 Rect that overflowed the 24-row terminal
    // and panicked the render loop. Post-fix: height=20, scroll=0,
    // visible=16 — fits, no panic.
    let fit = fit_dropdown(30, 0, 20, 4).expect("must fit on macOS Terminal default");
    assert_eq!(fit.height, 20);
    assert_eq!(fit.visible_items, 16);
    assert_eq!(fit.scroll_offset, 0);
    assert!(fit.height <= 20, "height must not exceed rows above input");
}
