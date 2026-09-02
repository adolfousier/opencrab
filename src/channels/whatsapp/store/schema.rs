//! The `wa_*` schema, applied idempotently on every open.

use wacore::store::error::Result;

use super::Store;
use super::errors::{db_err, interact_to_store_err, pool_err};

impl Store {
    pub(super) async fn run_migrations(&self) -> Result<()> {
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
            CREATE TABLE IF NOT EXISTS wa_msg_secrets (
                chat        TEXT NOT NULL,
                sender      TEXT NOT NULL,
                msg_id      TEXT NOT NULL,
                secret      BLOB NOT NULL,
                expires_at  INTEGER NOT NULL DEFAULT 0,
                message_ts  INTEGER NOT NULL DEFAULT 0,
                device_id   INTEGER NOT NULL,
                PRIMARY KEY (chat, sender, msg_id, device_id)
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
