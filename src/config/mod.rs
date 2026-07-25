//! Configuration Module
//!
//! Handles application configuration loading, validation, and management.

pub mod crabrace;
mod current;
pub mod guard;
pub mod health;
pub mod owner;
pub mod profile;
pub mod registry_client;
pub mod repair;
pub mod secrets;
pub mod startup_checks;
pub(crate) mod types;
pub mod update;

pub use crabrace::{CrabraceConfig, CrabraceIntegration};
pub use registry_client::{Model, Provider, RegistryClient};
pub use secrets::SecretString;
pub use types::*;
pub use update::{ProviderUpdater, UpdateResult};

// `merge_provider_keys` is internal to the crate but must be reachable
// from the regression tests in `src/tests/merge_provider_keys_test.rs`.
#[cfg(test)]
pub(crate) use types::merge_provider_keys;
