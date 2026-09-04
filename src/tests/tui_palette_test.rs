//! Palette invariants for the S1 consolidation (#1363).
//!
//! S1's contract is byte-identical rendering: one named const per distinct
//! value, zero merging. These tests guard the structural contracts other
//! code relies on so a future "tidy-up" can't silently change rendering.

use crate::tui::render::palette;
use ratatui::style::Color;

/// The badge rotation is indexed by `project_id % len`; shrinking it or
/// reordering it changes which projects get which colour. Lock both down.
#[test]
fn project_badge_colors_rotation_is_locked() {
    assert_eq!(palette::PROJECT_BADGE_COLORS.len(), 8);
    assert_eq!(palette::PROJECT_BADGE_COLORS[0], palette::ORANGE);
    assert_eq!(palette::PROJECT_BADGE_COLORS[6], palette::WHITE);
}

/// Every entry in the rotation must be distinct, otherwise two projects
/// share a badge colour and the "distinguishable badges" purpose dies.
#[test]
fn project_badge_colors_are_distinct() {
    let n = palette::PROJECT_BADGE_COLORS.len();
    for i in 0..n {
        for j in (i + 1)..n {
            assert_ne!(
                palette::PROJECT_BADGE_COLORS[i],
                palette::PROJECT_BADGE_COLORS[j],
                "entries {i} and {j} collide"
            );
        }
    }
}

/// The near-neighbour pairs that S1 deliberately did NOT merge. If someone
/// unifies these values that's an S2 theme decision and must be a conscious
/// change, not a refactor side effect.
#[test]
fn near_neighbour_values_stay_distinct() {
    assert_ne!(palette::ERROR, palette::ERROR_SOFT);
    assert_ne!(palette::SURFACE_CODE, palette::SURFACE_CODE_ALT);
    assert_ne!(palette::GRAY, palette::GRAY_MID);
    assert_ne!(palette::TEAL_VIVID, palette::TEAL_BRIGHT);
}

/// Severity colours must keep resolving to the historical values that
/// `tui_error_test` asserts end-to-end.
#[test]
fn brand_and_status_anchors_are_stable() {
    assert_eq!(palette::ORANGE, Color::Rgb(215, 100, 20));
    assert_eq!(palette::GRAY, Color::Rgb(120, 120, 120));
    assert_eq!(palette::SUCCESS, Color::Rgb(80, 200, 120));
}
