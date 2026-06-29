//! User-Defined Slash Commands
//!
//! Loads and saves user slash commands from TOML (with JSON fallback/migration).
//! Commands are merged with built-in slash commands for autocomplete.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A user-defined slash command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCommand {
    /// Command name including the leading slash, e.g. "/deploy"
    pub name: String,

    /// Short description shown in autocomplete
    pub description: String,

    /// Action type: "prompt" sends to LLM, "system" displays inline
    #[serde(default = "default_action")]
    pub action: String,

    /// The prompt text or system message content
    pub prompt: String,
}

fn default_action() -> String {
    "prompt".to_string()
}

/// TOML wrapper: `[[commands]]` array
#[derive(Debug, Serialize, Deserialize)]
struct CommandsFile {
    #[serde(default)]
    commands: Vec<UserCommand>,
}

/// Loads and saves user-defined slash commands (TOML primary, JSON fallback).
pub struct CommandLoader {
    /// Path to commands.toml
    toml_path: PathBuf,
    /// Path to legacy commands.json (for migration)
    json_path: PathBuf,
}

impl CommandLoader {
    /// Create a new CommandLoader pointing at a specific TOML file path.
    pub fn new(path: PathBuf) -> Self {
        let json_path = path.with_extension("json");
        Self {
            toml_path: path,
            json_path,
        }
    }

    /// Resolve the commands paths from the brain path.
    pub fn from_brain_path(brain_path: &std::path::Path) -> Self {
        Self {
            toml_path: brain_path.join("commands.toml"),
            json_path: brain_path.join("commands.json"),
        }
    }

    /// Load user commands. Priority: TOML → JSON (with auto-migration) → empty.
    pub fn load(&self) -> Vec<UserCommand> {
        // 1. Try TOML first
        if let Ok(content) = std::fs::read_to_string(&self.toml_path) {
            match toml::from_str::<CommandsFile>(&content) {
                Ok(file) => {
                    tracing::info!(
                        "Loaded {} user commands from {}",
                        file.commands.len(),
                        self.toml_path.display()
                    );
                    return file.commands;
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse commands.toml at {}: {}",
                        self.toml_path.display(),
                        e
                    );
                }
            }
        }

        // 2. Fall back to JSON and auto-migrate
        if let Ok(content) = std::fs::read_to_string(&self.json_path) {
            match serde_json::from_str::<Vec<UserCommand>>(&content) {
                Ok(commands) => {
                    tracing::info!(
                        "Loaded {} user commands from legacy {} — migrating to TOML",
                        commands.len(),
                        self.json_path.display()
                    );
                    // Auto-migrate: save as TOML
                    if let Err(e) = self.save(&commands) {
                        tracing::warn!("Failed to auto-migrate commands to TOML: {}", e);
                    } else {
                        tracing::info!("Migrated commands.json → {}", self.toml_path.display());
                    }
                    return commands;
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse commands.json at {}: {}",
                        self.json_path.display(),
                        e
                    );
                }
            }
        }

        tracing::debug!(
            "No commands file found at {} (this is normal)",
            self.toml_path.display()
        );
        Vec::new()
    }

    /// Save user commands to TOML file.
    pub fn save(&self, commands: &[UserCommand]) -> Result<()> {
        if let Some(parent) = self.toml_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        crate::config::daily_backup(&self.toml_path, 7);
        let file = CommandsFile {
            commands: commands.to_vec(),
        };
        let toml_str = toml::to_string_pretty(&file)?;
        std::fs::write(&self.toml_path, toml_str)?;
        tracing::info!(
            "Saved {} user commands to {}",
            commands.len(),
            self.toml_path.display()
        );
        Ok(())
    }

    /// Add a single command, preserving existing ones.
    pub fn add_command(&self, command: UserCommand) -> Result<()> {
        let mut commands = self.load();
        // Replace if same name exists
        if let Some(pos) = commands.iter().position(|c| c.name == command.name) {
            commands[pos] = command;
        } else {
            commands.push(command);
        }
        self.save(&commands)
    }

    /// Remove a command by name. Returns true if found and removed.
    pub fn remove_command(&self, name: &str) -> Result<bool> {
        let mut commands = self.load();
        let len_before = commands.len();
        commands.retain(|c| c.name != name);
        let removed = commands.len() < len_before;
        if removed {
            self.save(&commands)?;
        }
        Ok(removed)
    }

    /// Generate a slash commands section for the system brain.
    pub fn commands_section(builtin: &[(&str, &str)], user_commands: &[UserCommand]) -> String {
        let mut section = String::new();

        section.push_str("Built-in commands:\n");
        for (name, desc) in builtin {
            section.push_str(&format!("  {} — {}\n", name, desc));
        }

        if !user_commands.is_empty() {
            section.push_str("\nUser-defined commands:\n");
            for cmd in user_commands {
                section.push_str(&format!("  {} — {}\n", cmd.name, cmd.description));
            }
        }

        section
    }
}
