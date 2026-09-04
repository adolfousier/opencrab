//! TUI Rendering
//!
//! Main rendering logic for the terminal interface. [`frame`] is the
//! per-frame entry point, [`title`] the shared app title bar, and each
//! screen or widget has its own module. This file is declarations only —
//! no function definitions live here (CONTRIBUTING.md).

pub(crate) mod background;
pub(crate) mod chat;
mod dialogs;
mod frame;

mod help;
mod input;
pub(crate) mod mission_control;
pub(crate) mod palette;
mod panes;
mod plan_overlay;
mod plan_widget;
pub(crate) mod plan_window;
pub(crate) mod profiles_dialog;
mod projects;
mod session_files;
mod sessions;
pub(crate) mod skills_dialog;
// TEMP(#1364): allow(dead_code) until the S2 codemod lands role() consumers;
// removed with the codemod commit.
#[allow(dead_code)]
pub(crate) mod theme;
// TEMP(#1364): allow(dead_code) until the /theme command and boot apply land.
#[allow(dead_code)]
pub(crate) mod presets;
#[cfg(test)]
mod presets_test;
mod title;
mod tools;
pub(crate) mod utils;

// Re-export for sibling modules (e.g. onboarding_render)
pub(in crate::tui) use utils::char_boundary_at_width;

// Re-export for tests
#[cfg(test)]
pub(crate) use chat::reasoning_to_lines;
#[cfg(test)]
pub(crate) use input::{DropdownFit, dropdown_dimensions, fit_dropdown, truncate_to_chars};
#[cfg(test)]
pub(crate) use tools::collapse_build_output;
#[cfg(test)]
pub(crate) use tools::unescape_display_string;

pub use frame::render;
