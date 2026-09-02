//! Trello Integration
//!
//! Polls Trello board(s) for new card comments, routes them to the AI,
//! and replies by posting comments back on the card.
//!
//! Layout: [`state`] holds the shared `TrelloState`; `agent` polls,
//! `handler` routes comments, `client` wraps the REST API and `models`
//! holds its types. This file is declarations only — no function
//! definitions live here (CONTRIBUTING.md).

mod agent;
pub mod client;
pub(crate) mod handler;
pub mod models;
mod state;

pub use agent::TrelloAgent;
pub use client::TrelloClient;
pub use state::TrelloState;
