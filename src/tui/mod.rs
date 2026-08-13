//! Terminal User Interface
//!
//! Provides an interactive terminal interface for the AI orchestration agent using Ratatui.

pub mod app;
pub mod error;
pub mod events;
pub mod onboarding;
pub mod onboarding_render;
pub mod pane;
pub mod plan;
pub mod provider_selector;
pub mod render;
pub mod runner;

// Enhanced rendering modules
pub mod highlight;
pub(crate) mod hyperlink;
pub mod markdown;

pub mod components;

// Re-exports
pub use app::{App, DisplayMessage};
pub use events::{AppMode, EventHandler, TuiEvent};
pub use runner::run;
