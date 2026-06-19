//! Configuration Module
//!
//! Handles application configuration loading, validation, and management.

pub mod crabrace;
mod current;
pub mod health;
pub mod profile;
pub mod repair;
pub mod secrets;
mod types;
pub mod update;

pub use crabrace::{CrabraceConfig, CrabraceIntegration};
pub use secrets::SecretString;
pub use types::*;
pub use update::{ProviderUpdater, UpdateResult};

// `merge_provider_keys` is internal to the crate but must be reachable
// from the regression tests in `src/tests/merge_provider_keys_test.rs`.
#[cfg(test)]
pub(crate) use types::merge_provider_keys;
