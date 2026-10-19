//! Store: backend-selecting connection + migration runner (T003 per
//! plan.md). `Store::open()` dispatches on the `db_url`'s scheme:
//! `postgres://`/`postgresql://` selects the production Postgres backend
//! (feature-gated behind `postgres-backend`); anything else (a file path,
//! or `:memory:`) keeps the original embedded-SQLite behavior completely
//! unchanged (FR-006, SC-004).

use crate::error::Result;
use rusqlite::Connection;

#[cfg(feature = "postgres-backend")]
use super::postgres_pool::{self, PgPool};

pub enum Backend {
    Sqlite(Connection),
    #[cfg(feature = "postgres-backend")]
    Postgres(PgPool),
}

pub struct Store {
    pub backend: Backend,
}

impl Store {
    /// Backward-compatible accessor for the many existing call sites
    /// written against the embedded store's `store.conn` field. Panics if
    /// called against a `Postgres`-backed `Store` — every Postgres-aware
    /// call site must match on `store.backend` directly instead.
    pub fn conn(&self) -> &Connection {
        match &self.backend {
            Backend::Sqlite(conn) => conn,
            #[cfg(feature = "postgres-backend")]
            Backend::Postgres(_) => panic!(
                "Store::conn() called on a Postgres-backed Store — this call site needs a \
                 Postgres-dialect branch (see src/storage/store.rs doc comment)"
            ),
        }
    }

    /// Open a store at `db_url`. A `postgres://`/`postgresql://` URL opens
    /// (and, if unreachable, fails fast on) the Postgres backend; anything
    /// else opens the embedded SQLite file/`:memory:` path exactly as
    /// before this feature.
    pub fn open(db_url: &str) -> Result<Self> {
        if db_url.starts_with("postgres://") || db_url.starts_with("postgresql://") {
            return Self::open_postgres(db_url);
        }
        let conn = Connection::open(db_url)?;
        Self::init_sqlite(conn)
    }

    /// In-memory SQLite store, for tests — unchanged.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_sqlite(conn)
    }

    fn init_sqlite(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(super::schema::SQLITE_SCHEMA_SQL)?;
        Ok(Self {
            backend: Backend::Sqlite(conn),
        })
    }

    #[cfg(feature = "postgres-backend")]
    fn open_postgres(db_url: &str) -> Result<Self> {
        let pool = postgres_pool::build_pool(db_url)?;
        {
            let mut conn = pool.get()?;
            conn.batch_execute(super::schema::POSTGRES_SCHEMA_SQL)?;
        }
        Ok(Self {
            backend: Backend::Postgres(pool),
        })
    }

    #[cfg(not(feature = "postgres-backend"))]
    fn open_postgres(db_url: &str) -> Result<Self> {
        Err(crate::error::Error::Validation(format!(
            "'{db_url}' looks like a Postgres connection string, but this build of pamet was \
             compiled without the 'postgres-backend' feature — rebuild with \
             `--features postgres-backend` to use it"
        )))
    }
}
