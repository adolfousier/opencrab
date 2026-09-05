//! Side-effect actions invoked from the profiles dialog.

use crate::config::profile;
use crate::tui::app::App;
use crate::tui::events::AppMode;

use super::state::ProfileAction;

/// Open the profiles dialog. Loads the profile list and resets state.
pub fn open(app: &mut App) {
    if app.mode == AppMode::Profiles {
        return;
    }
    app.mode = AppMode::Profiles;
    app.profiles_dialog.reset();
    reload(app);
}

/// Reload the profile list from disk.
pub fn reload(app: &mut App) {
    let profiles = profile::list_profiles().unwrap_or_default();
    let active = profile::active_profile().unwrap_or("default").to_string();
    app.profiles_dialog.profiles = profiles;
    app.profiles_dialog.active_profile = active;
    // Clamp selection
    let visible =
        super::state::matching(&app.profiles_dialog.profiles, &app.profiles_dialog.filter);
    if app.profiles_dialog.selected_index >= visible.len() && !visible.is_empty() {
        app.profiles_dialog.selected_index = visible.len() - 1;
    }
}

/// Switch to the selected profile. Shows a message that restart is required.
pub fn switch_to(app: &mut App, name: &str) {
    if name == app.profiles_dialog.active_profile {
        return; // already active
    }
    match profile::set_active_profile(Some(name.to_string())) {
        Ok(()) => {
            app.push_system_message(format!(
                "Switched to profile '{}'. Restart OpenCrabs to apply: `opencrabs -p {}`",
                name, name
            ));
            app.mode = AppMode::Chat;
        }
        Err(e) => {
            app.push_system_message(format!("Failed to switch profile: {}", e));
        }
    }
}

/// Create a new profile with the given name and description.
pub fn create(app: &mut App, name: &str, description: Option<&str>) {
    match profile::create_profile(name, description) {
        Ok(_) => {
            app.push_system_message(format!(
                "Created profile '{}'. Switch to it with: `opencrabs -p {}`",
                name, name
            ));
            reload(app);
            app.profiles_dialog.action = ProfileAction::None;
            app.profiles_dialog.input_buffer.clear();
            app.profiles_dialog.input_buffer_2.clear();
        }
        Err(e) => {
            app.push_system_message(format!("Failed to create profile: {}", e));
            // Surface inline too: the full-screen dialog hides the chat log (#1381)
            app.profiles_dialog.error = Some(e.to_string());
        }
    }
}

/// Delete the named profile.
pub fn delete(app: &mut App, name: &str) {
    match profile::delete_profile(name) {
        Ok(()) => {
            app.push_system_message(format!("Deleted profile '{}'", name));
            reload(app);
            app.profiles_dialog.action = ProfileAction::None;
            app.profiles_dialog.filter.clear();
        }
        Err(e) => {
            app.push_system_message(format!("Failed to delete profile: {}", e));
        }
    }
}

/// Migrate files from one profile to another.
pub fn migrate(app: &mut App, from: &str, to: &str) {
    match profile::migrate_profile(from, to, false) {
        Ok(files) => {
            app.push_system_message(format!(
                "Migrated {} file(s) from '{}' to '{}'",
                files.len(),
                from,
                to
            ));
            app.profiles_dialog.action = ProfileAction::None;
            app.profiles_dialog.input_buffer.clear();
            app.profiles_dialog.input_buffer_2.clear();
        }
        Err(e) => {
            app.push_system_message(format!("Migration failed: {}", e));
        }
    }
}
