//! Parse-guards for the OpenCrabs config/secret files.
//!
//! A write that leaves `config.toml` or `keys.toml` unparseable takes the whole
//! agent down: no provider keys, no bot token, no way in. These helpers let
//! every write path (the config_manager tool, `edit_file`/`write_file`, and the
//! last-good snapshot) refuse a write that would break parsing, tell the caller
//! exactly why, and log it — so a bad edit is denied at the source instead of
//! corrupting the file and then poisoning the recovery snapshot too.

use std::path::Path;

use crate::config::Config;
use crate::config::opencrabs_home;
use crate::config::types::KeysFile;

/// Which protected file a path points at, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectedFile {
    Config,
    Keys,
}

impl ProtectedFile {
    pub fn file_name(self) -> &'static str {
        match self {
            ProtectedFile::Config => "config.toml",
            ProtectedFile::Keys => "keys.toml",
        }
    }
}

/// Best-effort absolute form for comparison — canonicalize when the file exists,
/// else fall back to the path as given.
fn normalize(path: &Path) -> std::path::PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Classify `path` as the OpenCrabs `config.toml` or `keys.toml` (profile-
/// resolved), or `None` for anything else. A PROJECT's own `config.toml` in a
/// working directory is NOT protected — only the active profile's real config
/// and secrets files are, matched by resolved absolute path.
pub fn protected_file(path: &Path) -> Option<ProtectedFile> {
    let home = opencrabs_home();
    let cfg = Config::system_config_path().unwrap_or_else(|| home.join("config.toml"));
    let keys = home.join("keys.toml");
    let target = normalize(path);
    if target == normalize(&cfg) {
        Some(ProtectedFile::Config)
    } else if target == normalize(&keys) {
        Some(ProtectedFile::Keys)
    } else {
        None
    }
}

/// Does `content` still parse for the given protected file? Returns the
/// human-readable parse error on failure. Uses the SAME deserialization the
/// loader uses (`Config` / `KeysFile`), so a pass here means the file will load.
pub fn parses(kind: ProtectedFile, content: &str) -> Result<(), String> {
    let res = match kind {
        ProtectedFile::Config => toml::from_str::<Config>(content).map(|_| ()),
        ProtectedFile::Keys => toml::from_str::<KeysFile>(content).map(|_| ()),
    };
    res.map_err(|e| e.to_string())
}

/// Guard a raw file write: if `path` is a protected config file and `content`
/// would break its parsing, log an error and return an explanatory message the
/// caller can surface to the agent. Returns `Ok(())` for non-protected paths and
/// for valid content, so callers can gate their write on `?`.
pub fn deny_if_would_break(path: &Path, content: &str) -> Result<(), String> {
    let Some(kind) = protected_file(path) else {
        return Ok(());
    };
    if let Err(e) = parses(kind, content) {
        let name = kind.file_name();
        tracing::error!(
            target: "config_guard",
            "Denied write to {name}: the new content does not parse ({e}) — file left unchanged"
        );
        return Err(format!(
            "Refused to write {name}: the new content is not valid ({name} must stay parseable) — \
             {e}. The file was NOT changed. Fix the TOML, or use the config_manager tool for config \
             edits instead of editing the file directly."
        ));
    }
    Ok(())
}
