//! Params-file plumbing — tool invocations travel as JSON files.
//!
//! Complex filters (dates, thread scopes, raw params) do not belong on a
//! command line: they fight quoting, leak into shell history, and cap out
//! at ARG_MAX. A tool invocation is data: write it to a file, load it
//! here, dispatch on it. Same discipline as `curl -d @file`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::commands::ToolCommand;

/// One tool invocation: the command plus its params, as written to disk.
///
/// ```json
/// { "tool": "read_chat", "chat": "-1004427473737", "limit": 50 }
/// ```
///
/// (`tool` is the serde tag on [`ToolCommand`]; the envelope exists so
/// the file format can evolve — version, metadata — without touching the
/// command data.)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ToolInvocation {
    /// Format version. Bump on breaking file-format change.
    #[serde(default = "default_version")]
    pub version: u8,
    #[serde(flatten)]
    pub command: ToolCommand,
}

const CURRENT_VERSION: u8 = 1;

const fn default_version() -> u8 {
    CURRENT_VERSION
}

impl ToolInvocation {
    /// Test-only constructor: production always arrives via `parse`/`load`.
    #[cfg(test)]
    pub(crate) fn new(command: ToolCommand) -> Self {
        Self {
            version: CURRENT_VERSION,
            command,
        }
    }

    /// Serialize to pretty JSON (test-only: what would be written to disk).
    #[cfg(test)]
    pub(crate) fn to_json(&self) -> anyhow::Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("serializing tool invocation: {e}"))
    }

    /// Save an invocation to a params file (test-only: creates parents).
    #[cfg(test)]
    pub(crate) fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("creating {}: {e}", parent.display()))?;
        }
        std::fs::write(path, self.to_json()?)
            .map_err(|e| anyhow::anyhow!("writing params file {}: {e}", path.display()))?;
        Ok(())
    }

    /// Parse an invocation from raw JSON (file contents).
    pub(crate) fn parse(json: &str) -> anyhow::Result<Self> {
        let inv: ToolInvocation = serde_json::from_str(json)
            .map_err(|e| anyhow::anyhow!("invalid tool invocation: {e}"))?;
        if inv.version > CURRENT_VERSION {
            anyhow::bail!(
                "tool invocation version {} is newer than supported {}",
                inv.version,
                CURRENT_VERSION
            );
        }
        Ok(inv)
    }

    /// Load an invocation from a params file.
    pub(crate) fn load(path: &Path) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading params file {}: {e}", path.display()))?;
        Self::parse(&json)
    }
}
