//! Rusqlite-backed WhatsApp session store
//!
//! Implements `wacore::store::Backend` using deadpool-sqlite + rusqlite,
//! matching the rest of the OpenCrabs database layer.

mod appsync;
mod device;
mod protocol;
mod signal;

use deadpool_sqlite::{Config, Hook, Pool, Runtime};

use wacore::store::error::{Result, StoreError};

/// Map a deadpool InteractError to StoreError
fn interact_to_store_err(e: deadpool_sqlite::InteractError) -> StoreError {
    StoreError::Database(format!("interact error: {e}").into())
}

/// Map a deadpool PoolError to StoreError
fn pool_err(e: deadpool_sqlite::PoolError) -> StoreError {
    StoreError::Connection(format!("pool error: {e}").into())
}

/// Map a rusqlite error to a `StoreError::Database`, preserving the typed source.
fn db_err(e: rusqlite::Error) -> StoreError {
    StoreError::Database(Box::new(e))
}

/// Rusqlite-backed storage for `whatsapp-rust`.
///
/// Uses a dedicated SQLite file at `~/.opencrabs/whatsapp/session.db`,
/// completely separate from the main OpenCrabs database.
#[derive(Clone)]
pub struct Store {
    pool: Pool,
    device_id: i32,
}

impl Store {
    /// Open (or create) the store at the given path.
    pub async fn new(path: &str) -> Result<Self> {
        let pool = Config::new(path)
            .builder(Runtime::Tokio1)
            .map_err(|e| StoreError::Connection(e.to_string().into()))?
            .max_size(4)
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

    async fn run_migrations(&self) -> Result<()> {
        let sql = r#"
            CREATE TABLE IF NOT EXISTS wa_device (
                id          INTEGER PRIMARY KEY,
                data        BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS wa_identities (
                address     TEXT NOT NULL,
                device_id   INTEGER NOT NULL,
                key         BLOB NOT NULL,
                PRIMARY KEY (address, device_id)
            );
            CREATE TABLE IF NOT EXISTS wa_sessions (
                address     TEXT NOT NULL,
                device_id   INTEGER NOT NULL,
                record      BLOB NOT NULL,
                PRIMARY KEY (address, device_id)
            );
            CREATE TABLE IF NOT EXISTS wa_prekeys (
                id          INTEGER NOT NULL,
                device_id   INTEGER NOT NULL,
                record      BLOB NOT NULL,
                uploaded    INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (id, device_id)
            );
            CREATE TABLE IF NOT EXISTS wa_signed_prekeys (
                id          INTEGER NOT NULL,
                device_id   INTEGER NOT NULL,
                record      BLOB NOT NULL,
                PRIMARY KEY (id, device_id)
            );
            CREATE TABLE IF NOT EXISTS wa_sender_keys (
                address     TEXT NOT NULL,
                device_id   INTEGER NOT NULL,
                record      BLOB NOT NULL,
                PRIMARY KEY (address, device_id)
            );
            CREATE TABLE IF NOT EXISTS wa_app_state_keys (
                key_id      BLOB NOT NULL,
                device_id   INTEGER NOT NULL,
                data        TEXT NOT NULL,
                PRIMARY KEY (key_id, device_id)
            );
            CREATE TABLE IF NOT EXISTS wa_app_state_versions (
                name        TEXT NOT NULL,
                device_id   INTEGER NOT NULL,
                data        TEXT NOT NULL,
                PRIMARY KEY (name, device_id)
            );
            CREATE TABLE IF NOT EXISTS wa_app_state_mutation_macs (
                name        TEXT NOT NULL,
                version     INTEGER NOT NULL,
                index_mac   BLOB NOT NULL,
                value_mac   BLOB NOT NULL,
                device_id   INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_mutation_macs_lookup
                ON wa_app_state_mutation_macs (name, index_mac, device_id);
            CREATE TABLE IF NOT EXISTS wa_sender_key_devices (
                group_jid   TEXT NOT NULL,
                device_jid  TEXT NOT NULL,
                has_key     INTEGER NOT NULL DEFAULT 0,
                device_id   INTEGER NOT NULL,
                PRIMARY KEY (group_jid, device_jid, device_id)
            );
            CREATE TABLE IF NOT EXISTS wa_lid_pn_mapping (
                lid             TEXT NOT NULL,
                phone_number    TEXT NOT NULL,
                created_at      INTEGER NOT NULL,
                updated_at      INTEGER NOT NULL,
                learning_source TEXT NOT NULL DEFAULT '',
                device_id       INTEGER NOT NULL,
                PRIMARY KEY (lid, device_id)
            );
            CREATE INDEX IF NOT EXISTS idx_lid_pn_phone
                ON wa_lid_pn_mapping (phone_number, device_id);
            CREATE TABLE IF NOT EXISTS wa_base_keys (
                address     TEXT NOT NULL,
                message_id  TEXT NOT NULL,
                base_key    BLOB NOT NULL,
                device_id   INTEGER NOT NULL,
                PRIMARY KEY (address, message_id, device_id)
            );
            CREATE TABLE IF NOT EXISTS wa_device_registry (
                user        TEXT NOT NULL,
                device_id   INTEGER NOT NULL,
                data        TEXT NOT NULL,
                PRIMARY KEY (user, device_id)
            );
            CREATE TABLE IF NOT EXISTS wa_tc_tokens (
                jid              TEXT NOT NULL,
                token            BLOB NOT NULL,
                token_timestamp  INTEGER NOT NULL,
                sender_timestamp INTEGER,
                device_id        INTEGER NOT NULL,
                PRIMARY KEY (jid, device_id)
            );
            CREATE TABLE IF NOT EXISTS wa_sent_messages (
                chat_jid    TEXT NOT NULL,
                message_id  TEXT NOT NULL,
                payload     BLOB NOT NULL,
                created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
                device_id   INTEGER NOT NULL,
                PRIMARY KEY (chat_jid, message_id, device_id)
            );
        "#;

        self.pool
            .get()
            .await
            .map_err(pool_err)?
            .interact(move |conn| conn.execute_batch(sql))
            .await
            .map_err(interact_to_store_err)?
            .map_err(db_err)?;
        Ok(())
    }
}

/// Extension trait for rusqlite optional queries
trait OptionalExt<T> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for std::result::Result<T, rusqlite::Error> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
