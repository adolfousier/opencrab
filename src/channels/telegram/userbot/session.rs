//! JSON-file session storage for the userbot.
//!
//! Replaces grammers' `SqliteSession`, whose libsql dependency asserts a
//! one-shot process-global `sqlite3_config(SQLITE_CONFIG_SERIALIZED)` that
//! fails (SQLITE_MISUSE → panic) once the platform's rusqlite pool has
//! initialized SQLite. `MemorySession` has no export API, so this is our own
//! [`Session`] impl over the same `SessionData`, persisted as serde_json
//! (tmp + rename, 0600) whenever dirty.
//!
//! The file IS the logged-in account. Treat it like keys.toml.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use grammers_session::types::{
    ChannelState, DcOption, PeerId, PeerInfo, UpdateState, UpdatesState,
};
use grammers_session::{BoxFuture, Session, SessionData};
use serde::{Deserialize, Serialize};

/// Serde mirror of [`SessionData`]. serde_json cannot key maps with integers,
/// so the maps become `Vec`s and are re-keyed by `id` on load.
#[derive(Serialize, Deserialize)]
struct PersistedSession {
    home_dc: i32,
    dc_options: Vec<DcOption>,
    peer_infos: Vec<PeerInfo>,
    updates_state: UpdatesState,
}

impl From<&SessionData> for PersistedSession {
    fn from(d: &SessionData) -> Self {
        Self {
            home_dc: d.home_dc,
            dc_options: d.dc_options.values().cloned().collect(),
            peer_infos: d.peer_infos.values().cloned().collect(),
            updates_state: d.updates_state.clone(),
        }
    }
}

impl From<PersistedSession> for SessionData {
    fn from(p: PersistedSession) -> Self {
        Self {
            home_dc: p.home_dc,
            dc_options: p.dc_options.into_iter().map(|d| (d.id, d)).collect(),
            peer_infos: p.peer_infos.into_iter().map(|i| (i.id(), i)).collect(),
            updates_state: p.updates_state,
        }
    }
}

#[derive(Debug)]
pub(crate) enum FileSessionError {
    Poisoned,
}

impl std::error::Error for FileSessionError {}

impl fmt::Display for FileSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileSessionError::Poisoned => write!(f, "session lock is poisoned"),
        }
    }
}

/// In-memory session state persisted to a JSON file when dirty.
pub(crate) struct FileSession {
    data: Mutex<SessionData>,
    path: PathBuf,
    dirty: AtomicBool,
}

impl FileSession {
    /// Load the session from `path`, or start from defaults if absent/corrupt.
    /// A corrupt file is a hard error: silently proceeding would mint a new
    /// auth key and strand the logged-in device session.
    pub(crate) fn load(path: &Path) -> anyhow::Result<Self> {
        let data = if path.is_file() {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
            let persisted: PersistedSession = serde_json::from_str(&raw)
                .map_err(|e| anyhow::anyhow!("parsing {}: {e}", path.display()))?;
            persisted.into()
        } else {
            SessionData::default()
        };
        Ok(Self {
            data: Mutex::new(data),
            path: path.to_path_buf(),
            dirty: AtomicBool::new(false),
        })
    }

    /// Persist unconditionally with an owner-only temp file and atomic replace.
    pub(crate) fn save(&self) -> anyhow::Result<()> {
        use std::io::Write as _;

        let snapshot = {
            let guard = self
                .data()
                .map_err(|error| anyhow::anyhow!("saving session: {error}"))?;
            PersistedSession::from(&*guard)
        };
        let json = serde_json::to_vec_pretty(&snapshot)?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("session path has no parent"))?;
        std::fs::create_dir_all(parent)?;

        let mut builder = tempfile::Builder::new();
        builder.prefix(".telegram-userbot-").suffix(".tmp");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            builder.permissions(std::fs::Permissions::from_mode(0o600));
        }
        let mut file = builder.tempfile_in(parent)?;
        file.write_all(&json)?;
        file.as_file().sync_all()?;
        file.persist(&self.path).map_err(|error| error.error)?;
        Ok(())
    }

    /// Persist only when the MTProto session mutated since the previous tick.
    pub(crate) fn save_if_dirty(&self) -> anyhow::Result<()> {
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return Ok(());
        }
        if let Err(error) = self.save() {
            self.mark_dirty();
            return Err(error);
        }
        Ok(())
    }

    fn data(&self) -> Result<MutexGuard<'_, SessionData>, FileSessionError> {
        self.data.lock().map_err(|_| FileSessionError::Poisoned)
    }

    fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }
}

impl Session for FileSession {
    type Error = FileSessionError;

    fn home_dc_id(&self) -> Result<i32, Self::Error> {
        Ok(self.data()?.home_dc)
    }

    fn set_home_dc_id(&self, dc_id: i32) -> BoxFuture<'_, Result<(), Self::Error>> {
        Box::pin(async move {
            self.data()?.home_dc = dc_id;
            self.mark_dirty();
            Ok(())
        })
    }

    fn dc_option(&self, dc_id: i32) -> Result<Option<DcOption>, Self::Error> {
        Ok(self.data()?.dc_options.get(&dc_id).cloned())
    }

    fn set_dc_option(&self, dc_option: &DcOption) -> BoxFuture<'_, Result<(), Self::Error>> {
        let dc_option = dc_option.clone();
        Box::pin(async move {
            self.data()?
                .dc_options
                .insert(dc_option.id, dc_option.clone());
            self.mark_dirty();
            Ok(())
        })
    }

    fn peer(&self, peer: PeerId) -> BoxFuture<'_, Result<Option<PeerInfo>, Self::Error>> {
        Box::pin(async move { Ok(self.data()?.peer_infos.get(&peer).cloned()) })
    }

    fn cache_peer(&self, peer: &PeerInfo) -> BoxFuture<'_, Result<(), Self::Error>> {
        let peer = peer.clone();
        Box::pin(async move {
            self.data()?
                .peer_infos
                .entry(peer.id())
                .or_insert_with(|| peer.clone())
                .extend_info(&peer);
            self.mark_dirty();
            Ok(())
        })
    }

    fn updates_state(&self) -> BoxFuture<'_, Result<UpdatesState, Self::Error>> {
        Box::pin(async move { Ok(self.data()?.updates_state.clone()) })
    }

    fn set_update_state(&self, update: UpdateState) -> BoxFuture<'_, Result<(), Self::Error>> {
        Box::pin(async move {
            let mut data = self.data()?;
            match update {
                UpdateState::All(updates_state) => {
                    data.updates_state = updates_state;
                }
                UpdateState::Primary { pts, date, seq } => {
                    data.updates_state.pts = pts;
                    data.updates_state.date = date;
                    data.updates_state.seq = seq;
                }
                UpdateState::Secondary { qts } => {
                    data.updates_state.qts = qts;
                }
                UpdateState::Channel { id, pts } => {
                    data.updates_state.channels.retain(|c| c.id != id);
                    data.updates_state.channels.push(ChannelState { id, pts });
                }
            }
            drop(data);
            self.mark_dirty();
            Ok(())
        })
    }
}
