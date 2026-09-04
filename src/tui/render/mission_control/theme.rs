//! Mission Control theme — panel-specific aliases over the shared
//! brand palette. Brand-level colours (orange / teal / white / text
//! shades) live in `tui/render/palette` so other dialogs can reuse
//! them without going through MC's namespace.

pub use crate::tui::render::palette::{
    ANALYTICS_GREEN, GRAY, ORANGE, TEAL, TEXT_DIM, TEXT_PRIMARY, TEXT_SECONDARY, WHITE, dim, muted,
    title_style,
};

use ratatui::style::{Color, Style};

// ── Panel chrome ────────────────────────────────────────────────────────────

/// Panel border when not focused — neutral grey, same as `sessions.rs`.
pub const BORDER_IDLE: Color = GRAY;
/// Per-panel focus accents.
pub const BORDER_INBOX_FOCUS: Color = TEAL;
pub const BORDER_ACTIVITY_FOCUS: Color = ORANGE;
pub const BORDER_SCHEDULE_FOCUS: Color = WHITE;
/// Analytics panel focus accent (green, from the analytics dashboard palette).
pub const BORDER_ANALYTICS_FOCUS: Color = ANALYTICS_GREEN;
/// Bar/value highlight for the analytics panel.
pub const GREEN: Color = ANALYTICS_GREEN;

// ── Help bar ────────────────────────────────────────────────────────────────

pub const HELP_BAR: Color = GRAY;

pub fn help_bar_style() -> Style {
    Style::default().fg(HELP_BAR)
}
