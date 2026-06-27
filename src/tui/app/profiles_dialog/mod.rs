//! Profiles dialog — app-side state, input, actions.
//!
//! Pairs with `src/tui/render/profiles_dialog/`. Single responsibility
//! per file: state lives in `state.rs`, key handling in `input.rs`,
//! side-effecting actions (open / switch / create / delete) in `actions.rs`.

pub mod actions;
pub mod input;
pub mod state;

pub use state::ProfilesDialogState;
