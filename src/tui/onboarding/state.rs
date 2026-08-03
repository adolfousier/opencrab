//! Persisted onboarding progress.
//!
//! Whether the user finished setup is a fact about the user, not something to
//! be inferred from the config. `is_first_time()` used to answer "could some
//! provider serve a request right now?", which is a different question: a CLI
//! provider needs no API key, so an `enabled = true` section left by a partial
//! run or a hand edit was enough to make onboarding disappear while nothing
//! else had been set. The user landed in a chat with no usable pair and no
//! explanation (#919).
//!
//! Kept in its own small file rather than in config.toml: this is bookkeeping,
//! not something a user should have to read past or edit, and writing it does
//! not disturb the config the process is running on.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where the user got to in the wizard, and whether they ever reached the end.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnboardingState {
    /// The user reached the final step. Only ever set by finishing the wizard,
    /// or once by the migration below for installs that predate this file.
    #[serde(default)]
    pub completed: bool,
    /// The step the user was last on, so an interrupted run resumes there
    /// instead of restarting from step one.
    #[serde(default)]
    pub last_step: Option<String>,
    /// "quick" or "advanced". Which steps are still outstanding depends on it,
    /// so resuming without it would list the wrong ones.
    #[serde(default)]
    pub mode: Option<String>,
}

impl OnboardingState {
    pub fn path() -> PathBuf {
        crate::config::opencrabs_home().join("onboarding.json")
    }

    /// Read the recorded progress. A missing or unreadable file means "no
    /// progress recorded", which is the safe answer: it shows the wizard
    /// rather than skipping setup the user never did.
    pub fn load() -> Self {
        let path = Self::path();
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                tracing::warn!(
                    "Onboarding progress at {} could not be read ({e}) — treating setup as unfinished",
                    path.display()
                );
                return Self::default();
            }
        };
        match serde_json::from_str(&raw) {
            Ok(state) => state,
            Err(e) => {
                tracing::warn!(
                    "Onboarding progress at {} is not valid JSON ({e}) — treating setup as unfinished",
                    path.display()
                );
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!(
                "Could not create {} for onboarding progress: {e}",
                parent.display()
            );
            return;
        }
        let json = match serde_json::to_string_pretty(self) {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!("Could not serialize onboarding progress: {e}");
                return;
            }
        };
        if let Err(e) = std::fs::write(&path, json) {
            tracing::warn!(
                "Could not write onboarding progress to {}: {e}",
                path.display()
            );
        }
    }

    /// Record the step the user is on, and the flow they are in. Called as the
    /// wizard advances so an exit halfway through is resumable.
    pub fn record_step(step: &str, mode: &str) {
        let mut state = Self::load();
        if state.last_step.as_deref() == Some(step) && state.mode.as_deref() == Some(mode) {
            return;
        }
        state.last_step = Some(step.to_string());
        state.mode = Some(mode.to_string());
        state.save();
    }

    /// Record that the user reached the end of the wizard.
    pub fn mark_completed() {
        let mut state = Self::load();
        state.completed = true;
        state.last_step = None;
        state.save();
    }
}
