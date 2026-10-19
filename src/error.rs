//! Shared error type for the pamet-core library (T008).

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("storage error: {0}")]
    Storage(#[from] rusqlite::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("unknown item {item_id} for retrieval event {event_id}")]
    UnknownItemForEvent { event_id: i64, item_id: String },

    #[error("review entry {0} is not pending")]
    NotPending(i64),

    #[error("malformed event payload: {0}")]
    MalformedEvent(String),

    #[cfg(feature = "postgres-backend")]
    #[error("postgres storage error: {0}")]
    Postgres(#[from] postgres::Error),

    #[cfg(feature = "postgres-backend")]
    #[error("postgres connection pool error: {0}")]
    PostgresPool(#[from] r2d2::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
