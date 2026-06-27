//! App Module — TUI application state and logic.

pub(crate) mod background_session;
mod dialogs;
pub(crate) mod input;
mod messaging;
pub mod mission_control;
pub mod profiles_dialog;
pub mod skills_dialog;
mod state;

pub use background_session::{BackgroundSessionState, SessionStateMut};
pub use state::*;

// Re-export sibling modules so sub-modules can use `super::events`, etc.
pub(crate) use super::events;
pub(crate) use super::onboarding;
pub(crate) use super::prompt_analyzer;
