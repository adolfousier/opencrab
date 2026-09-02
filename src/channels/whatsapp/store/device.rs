use super::Store;
use super::errors::{OptionalExt, db_err, interact_to_store_err, pool_err};
use async_trait::async_trait;
use rusqlite::params;
use wacore::store::Device;
use wacore::store::error::{Result, StoreError};
use wacore::store::traits::DeviceStore;

#[async_trait]
impl DeviceStore for Store {
    async fn save(&self, device: &Device) -> Result<()> {
        let bytes =
            rmp_serde::to_vec(device).map_err(|e| StoreError::Serialization(Box::new(e)))?;
        let did = self.device_id;
        self.pool
            .get()
            .await
            .map_err(pool_err)?
            .interact(move |conn| {
                conn.execute(
                    "INSERT INTO wa_device (id, data) VALUES (?1, ?2)
                     ON CONFLICT(id) DO UPDATE SET data = excluded.data",
                    params![did, bytes],
                )
            })
            .await
            .map_err(interact_to_store_err)?
            .map_err(db_err)?;
        Ok(())
    }

    async fn load(&self) -> Result<Option<Device>> {
        let did = self.device_id;
        let data_opt = self
            .pool
            .get()
            .await
            .map_err(pool_err)?
            .interact(move |conn| {
                conn.prepare("SELECT data FROM wa_device WHERE id = ?1")?
                    .query_row(params![did], |row| row.get::<_, Vec<u8>>(0))
                    .optional()
            })
            .await
            .map_err(interact_to_store_err)?
            .map_err(db_err)?;
        match data_opt {
            Some(data) => {
                let device: Device = match rmp_serde::from_slice(&data) {
                    Ok(d) => d,
                    Err(_) => {
                        // Old JSON-serialized data can't roundtrip (byte array issue).
                        // Delete it so the client re-pairs cleanly.
                        tracing::warn!(
                            "WhatsApp: clearing incompatible legacy device data — re-pair required"
                        );
                        let did2 = self.device_id;
                        // Best-effort cleanup: if pool.get() fails, skip the delete.
                        if let Ok(conn) = self.pool.get().await {
                            tokio::spawn(async move {
                                if let Err(e) = conn
                                    .interact(move |conn| {
                                        conn.execute(
                                            "DELETE FROM wa_device WHERE id = ?1",
                                            params![did2],
                                        )
                                    })
                                    .await
                                {
                                    tracing::warn!(error = %e, "failed to delete legacy WhatsApp device data");
                                }
                            });
                        }
                        return Ok(None);
                    }
                };
                Ok(Some(device))
            }
            None => Ok(None),
        }
    }

    async fn exists(&self) -> Result<bool> {
        let did = self.device_id;
        self.pool
            .get()
            .await
            .map_err(pool_err)?
            .interact(move |conn| {
                conn.prepare("SELECT 1 FROM wa_device WHERE id = ?1")?
                    .query_row(params![did], |_| Ok(()))
                    .optional()
                    .map(|opt| opt.is_some())
            })
            .await
            .map_err(interact_to_store_err)?
            .map_err(db_err)
    }

    async fn create(&self) -> Result<i32> {
        Ok(self.device_id)
    }
}
