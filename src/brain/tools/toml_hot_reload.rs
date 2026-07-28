//! Reload-on-change cache for the safety TOMLs under `~/.opencrabs/safety/`.
//!
//! `bash_blocklist.toml` and `ralph_loop.toml` were each cached in a
//! `OnceLock`, which reads the file once per process. Both were described as
//! runtime-editable, and neither was: adding a blocklist pattern or changing an
//! iteration cap needed a restart, which is exactly the rebuild-free path these
//! files exist to provide (#851, #852).
//!
//! The cache is keyed on the file's modification time and length. Changing the
//! file changes at least one of those, so the next read picks it up. Keeping a
//! key at all matters because the blocklist is consulted on every bash call.
//!
//! A missing, unreadable or unparseable file yields `None` and is cached as
//! such, so a broken config does not re-read and re-log on every call. Fixing
//! the file changes its mtime, which re-arms the load.

use serde::de::DeserializeOwned;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

/// What the cache compares to decide whether a reload is needed. `None` means
/// the file was absent, which is itself a state worth caching: it must be
/// distinguishable from a file that exists so appearance triggers a load.
type Stamp = Option<(SystemTime, u64)>;

pub struct HotToml<T> {
    path: PathBuf,
    label: &'static str,
    cached: RwLock<Option<(Stamp, Option<Arc<T>>)>>,
}

impl<T: DeserializeOwned> HotToml<T> {
    pub fn new(path: PathBuf, label: &'static str) -> Self {
        Self {
            path,
            label,
            cached: RwLock::new(None),
        }
    }

    /// The parsed config, reloading first if the file changed on disk.
    ///
    /// `None` when the file is missing or does not parse. Never panics and
    /// never propagates an error: these configs add enforcement on top of the
    /// hardcoded floor, so an unreadable one must degrade rather than break the
    /// tool that consults it.
    pub fn get(&self) -> Option<Arc<T>> {
        let stamp = self.stamp();

        if let Ok(guard) = self.cached.read()
            && let Some((cached_stamp, value)) = guard.as_ref()
            && *cached_stamp == stamp
        {
            return value.clone();
        }

        let loaded = self.load();
        if let Ok(mut guard) = self.cached.write() {
            *guard = Some((stamp, loaded.clone()));
        }
        loaded
    }

    fn stamp(&self) -> Stamp {
        let meta = std::fs::metadata(&self.path).ok()?;
        Some((meta.modified().ok()?, meta.len()))
    }

    fn load(&self) -> Option<Arc<T>> {
        if !self.path.exists() {
            tracing::debug!("No {} at {}", self.label, self.path.display());
            return None;
        }
        let content = match std::fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("{} read error: {e}", self.label);
                return None;
            }
        };
        match toml::from_str::<T>(&content) {
            Ok(parsed) => Some(Arc::new(parsed)),
            Err(e) => {
                tracing::warn!("{} parse error: {e}", self.label);
                None
            }
        }
    }
}

/// Path of a safety config, or `None` when the home directory is undetectable.
pub fn safety_path(file: &str) -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".opencrabs/safety").join(file))
}
