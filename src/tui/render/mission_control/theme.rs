//! Mission Control theme — panel-specific shims over the runtime theme
//! system. Every accessor resolves through `theme::role()` so Mission
//! Control follows `/theme set`. Style helpers (`dim`, `muted`,
//! `title_style`) are re-exported from the palette module, whose bodies
//! are themselves theme-aware.

pub use crate::tui::render::palette::{dim, muted, title_style};

use ratatui::style::{Color, Style};

use crate::tui::render::theme::{self, Role};

// ── Text ────────────────────────────────────────────────────────────────────

pub fn text_primary() -> Color {
    theme::role(Role::TextPrimary)
}

pub fn text_secondary() -> Color {
    theme::role(Role::TextSecondary)
}

pub fn text_dim() -> Color {
    theme::role(Role::TextDim)
}

// ── Accents ─────────────────────────────────────────────────────────────────

pub fn orange() -> Color {
    theme::role(Role::Accent)
}

pub fn teal() -> Color {
    theme::role(Role::AccentTeal)
}

pub fn white() -> Color {
    theme::role(Role::AccentSoft)
}

pub fn green() -> Color {
    theme::role(Role::AnalyticsGreen)
}

// ── Panel chrome ────────────────────────────────────────────────────────────

/// Panel border when not focused — neutral grey, same as `sessions.rs`.
pub fn border_idle() -> Color {
    theme::role(Role::Gray)
}

/// Per-panel focus accents.
pub fn border_inbox_focus() -> Color {
    theme::role(Role::AccentTeal)
}

pub fn border_activity_focus() -> Color {
    theme::role(Role::Accent)
}

pub fn border_schedule_focus() -> Color {
    theme::role(Role::AccentSoft)
}

/// Analytics panel focus accent (green, from the analytics dashboard palette).
pub fn border_analytics_focus() -> Color {
    theme::role(Role::AnalyticsGreen)
}

// ── Help bar ────────────────────────────────────────────────────────────────

pub fn help_bar_style() -> Style {
    Style::default().fg(theme::role(Role::Gray))
}
