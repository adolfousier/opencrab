//! Mission Control theme — panel-specific aliases over the shared
//! brand palette. Brand-level colours (orange / teal / white / text
//! shades) live in `tui/render/palette` so other dialogs can reuse
//! them without going through MC's namespace.

pub use crate::tui::render::palette::{
    ORANGE, TEAL, TEXT_DIM, TEXT_PRIMARY, TEXT_SECONDARY, WHITE, dim, muted, title_style,
};

use ratatui::style::{Color, Style};

// ── Panel chrome ────────────────────────────────────────────────────────────

/// Panel border when not focused — neutral grey, same as `sessions.rs`.
pub const BORDER_IDLE: Color = Color::Rgb(120, 120, 120);
/// Per-panel focus accents.
pub const BORDER_INBOX_FOCUS: Color = TEAL;
pub const BORDER_ACTIVITY_FOCUS: Color = ORANGE;
pub const BORDER_SCHEDULE_FOCUS: Color = WHITE;
/// Analytics panel focus accent (green, from the analytics dashboard palette).
pub const BORDER_ANALYTICS_FOCUS: Color = Color::Rgb(46, 204, 113);
/// Bar/value highlight for the analytics panel.
pub const GREEN: Color = Color::Rgb(46, 204, 113);

// ── Help bar ────────────────────────────────────────────────────────────────

pub const HELP_BAR: Color = Color::Rgb(120, 120, 120);

pub fn help_bar_style() -> Style {
    Style::default().fg(HELP_BAR)
}
