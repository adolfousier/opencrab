use super::*;
use async_trait::async_trait;
use rusqlite::params;
use wacore::store::traits::{
    MsgSecretEntry, MsgSecretStore, merge_msg_secret_expiry, merge_msg_secret_message_ts,
};

#[async_trait]
impl MsgSecretStore for Store {
    async fn put_msg_secrets(&self, entries: Vec<MsgSecretEntry>) -> Result<usize> {
        let did = self.device_id;
        let stored = entries.len();
        self.pool
            .get()
            .await
            .map_err(pool_err)?
            .interact(move |conn| {
                for e in entries {
                    // On key conflict the window must never shrink: later
                    // deadline wins (0 = never), and a 0 message_ts never
                    // clobbers a known one.
                    let existing: Option<(i64, i64)> = conn
                        .prepare(
                            "SELECT expires_at, message_ts FROM wa_msg_secrets
                             WHERE chat = ?1 AND sender = ?2 AND msg_id = ?3 AND device_id = ?4",
                        )?
                        .query_row(params![e.chat, e.sender, e.msg_id, did], |row| {
                            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                        })
                        .optional()?;
                    let (expires_at, message_ts) = match existing {
                        Some((ex, ts)) => (
                            merge_msg_secret_expiry(ex, e.expires_at),
                            merge_msg_secret_message_ts(ts, e.message_ts),
                        ),
                        None => (e.expires_at, e.message_ts),
                    };
                    conn.execute(
                        "INSERT INTO wa_msg_secrets
                            (chat, sender, msg_id, secret, expires_at, message_ts, device_id)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                         ON CONFLICT(chat, sender, msg_id, device_id) DO UPDATE SET
                            secret = excluded.secret,
                            expires_at = excluded.expires_at,
                            message_ts = excluded.message_ts",
                        params![
                            e.chat, e.sender, e.msg_id, e.secret, expires_at, message_ts, did
                        ],
                    )?;
                }
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .map_err(interact_to_store_err)?
            .map_err(db_err)?;
        Ok(stored)
    }

    async fn get_msg_secret(
        &self,
        chat: &str,
        sender: &str,
        msg_id: &str,
    ) -> Result<Option<Vec<u8>>> {
        Ok(self
            .get_msg_secret_with_ts(chat, sender, msg_id)
            .await?
            .map(|(secret, _)| secret))
    }

    async fn get_msg_secret_with_ts(
        &self,
        chat: &str,
        sender: &str,
        msg_id: &str,
    ) -> Result<Option<(Vec<u8>, i64)>> {
        let did = self.device_id;
        let (c, s, m) = (chat.to_string(), sender.to_string(), msg_id.to_string());
        self.pool
            .get()
            .await
            .map_err(pool_err)?
            .interact(move |conn| {
                conn.prepare(
                    "SELECT secret, message_ts FROM wa_msg_secrets
                     WHERE chat = ?1 AND sender = ?2 AND msg_id = ?3 AND device_id = ?4",
                )?
                .query_row(params![c, s, m, did], |row| {
                    Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?))
                })
                .optional()
            })
            .await
            .map_err(interact_to_store_err)?
            .map_err(db_err)
    }

    async fn delete_expired_msg_secrets(&self, cutoff_timestamp: i64) -> Result<u32> {
        // Rows with expires_at = 0 (never) are kept.
        let did = self.device_id;
        let removed = self
            .pool
            .get()
            .await
            .map_err(pool_err)?
            .interact(move |conn| {
                conn.execute(
                    "DELETE FROM wa_msg_secrets
                     WHERE device_id = ?1 AND expires_at != 0 AND expires_at <= ?2",
                    params![did, cutoff_timestamp],
                )
            })
            .await
            .map_err(interact_to_store_err)?
            .map_err(db_err)?;
        Ok(removed as u32)
    }
}
