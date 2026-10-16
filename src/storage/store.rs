//! Store: SQLite connection + migration runner (T005, T007 per plan.md — merged here).

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;

pub struct Store {
    pub conn: Connection,
}

impl Store {
    /// Open (or create) the SQLite file at `path` and run migrations.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// In-memory store, for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(super::schema::SCHEMA_SQL)?;
        Ok(Self { conn })
    }
}
