//! The [`Store`] handle: a single-connection deadpool over the dedicated
//! WhatsApp session database, opened and migrated by [`Store::new`].

use deadpool_sqlite::{Config, Hook, Pool, Runtime};

use wacore::store::error::{Result, StoreError};

/// Rusqlite-backed storage for `whatsapp-rust`.
///
/// Uses a dedicated SQLite file at `~/.opencrabs/whatsapp/session.db`,
/// completely separate from the main OpenCrabs database.
#[derive(Clone)]
pub struct Store {
    pub(super) pool: Pool,
    pub(super) device_id: i32,
}

impl Store {
    /// Open (or create) the store at the given path.
    pub async fn new(path: &str) -> Result<Self> {
        let pool = Config::new(path)
            // Single connection: the Signal session store is read-modify-write
            // (load_session -> ratchet/establish -> put_session), and
            // whatsapp-rust encrypts for a message's recipient devices in
            // PARALLEL. With multiple pooled connections, a session written for
            // a device on one connection isn't always visible to the encrypt's
            // read on another connection in time, so that device is skipped
            // ("session ... not found"), the participant list comes out
            // incomplete, and the server rejects the whole message with error
            // 400 (intermittently — whichever device loses the race). Serializing
            // all store access removes that race. A personal bot's WhatsApp
            // throughput is tiny, so one connection costs nothing.
            .builder(Runtime::Tokio1)
            .map_err(|e| StoreError::Connection(e.to_string().into()))?
            .max_size(1)
            .post_create(Hook::async_fn(|conn, _| {
                Box::pin(async move {
                    conn.interact(|conn| {
                        conn.execute_batch(
                            "PRAGMA journal_mode = WAL;
                             PRAGMA busy_timeout = 10000;",
                        )
                    })
                    .await
                    .map_err(|e| deadpool_sqlite::HookError::Message(e.to_string().into()))?
                    .map_err(|e| deadpool_sqlite::HookError::Message(e.to_string().into()))?;
                    Ok(())
                })
            }))
            .build()
            .map_err(|e| StoreError::Connection(e.to_string().into()))?;

        let store = Self { pool, device_id: 1 };
        store.run_migrations().await?;
        Ok(store)
    }
}
