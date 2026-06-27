//! Profiles dialog app-side state.

use crate::config::profile::ProfileEntry;

/// Runtime state for the `/profiles` dialog. Single struct so `AppState`
/// only carries one field (`pub profiles_dialog: ProfilesDialogState`); all
/// dialog-specific behaviour lives in this module.
#[derive(Debug, Clone, Default)]
pub struct ProfilesDialogState {
    /// Live filter string. Type-to-narrow against profile name.
    pub filter: String,
    /// Index into the *filtered* list (not the unfiltered loaded set).
    pub selected_index: usize,
    /// Vertical scroll offset for the profile list.
    pub scroll_offset: u16,
    /// Cached profile list (loaded on open).
    pub profiles: Vec<ProfileEntry>,
    /// Name of the currently active profile.
    pub active_profile: String,
    /// Current action state (create, delete confirm, etc.)
    pub action: ProfileAction,
    /// Text input buffer for create/migrate flows.
    pub input_buffer: String,
    /// Secondary input buffer (e.g. description for create, destination for migrate).
    pub input_buffer_2: String,
}

/// Action state for the profiles dialog.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProfileAction {
    /// Normal browsing mode.
    #[default]
    None,
    /// Confirming deletion of a profile (name being deleted).
    ConfirmDelete(String),
    /// Creating a new profile (typing the name).
    CreateName,
    /// Creating a new profile (typing the description).
    CreateDesc,
    /// Migrating: typing source profile.
    MigrateFrom,
    /// Migrating: typing destination profile.
    MigrateTo,
}

impl ProfilesDialogState {
    /// Reset to a clean slate (called by `actions::open`).
    pub fn reset(&mut self) {
        self.filter.clear();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.action = ProfileAction::None;
        self.input_buffer.clear();
        self.input_buffer_2.clear();
    }
}

/// Filter `profiles` by `query` (case-insensitive substring on name).
/// Empty query passes everything through.
pub fn matching<'a>(profiles: &'a [ProfileEntry], query: &str) -> Vec<&'a ProfileEntry> {
    if query.is_empty() {
        return profiles.iter().collect();
    }
    let q = query.to_lowercase();
    profiles
        .iter()
        .filter(|p| p.name.to_lowercase().contains(&q))
        .collect()
}
