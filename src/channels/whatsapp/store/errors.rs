//! Error mapping from deadpool / rusqlite into `wacore`'s `StoreError`,
//! plus the `optional()` extension for "no rows" queries.

use wacore::store::error::StoreError;

/// Map a deadpool InteractError to StoreError
pub(crate) fn interact_to_store_err(e: deadpool_sqlite::InteractError) -> StoreError {
    StoreError::Database(format!("interact error: {e}").into())
}

/// Map a deadpool PoolError to StoreError
pub(crate) fn pool_err(e: deadpool_sqlite::PoolError) -> StoreError {
    StoreError::Connection(format!("pool error: {e}").into())
}

/// Map a rusqlite error to a `StoreError::Database`, preserving the typed source.
pub(crate) fn db_err(e: rusqlite::Error) -> StoreError {
    StoreError::Database(Box::new(e))
}

/// Extension trait for rusqlite optional queries
pub(crate) trait OptionalExt<T> {
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
