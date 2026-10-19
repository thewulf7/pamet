//! Postgres connection pool construction (T004), feature-gated behind
//! `postgres-backend`. Uses `r2d2`/`r2d2_postgres` — a mature, synchronous
//! pooling layer (Constitution Principle VI: adopt, don't reinvent) — so
//! every `Store` method that needs a connection checks one out of the pool,
//! giving genuine concurrent connections (research.md Decision 6) instead
//! of the embedded SQLite backend's single shared connection.

use crate::error::{Error, Result};
use postgres::NoTls;
use r2d2_postgres::PostgresConnectionManager;

pub type PgPool = r2d2::Pool<PostgresConnectionManager<NoTls>>;

/// Build a pool against `url` (a `postgres://`/`postgresql://` connection
/// string) and fail fast if the server is unreachable (FR-007) rather than
/// lazily discovering it on the first real query.
pub fn build_pool(url: &str) -> Result<PgPool> {
    let config = url
        .parse::<postgres::Config>()
        .map_err(|e| Error::Validation(format!("invalid postgres connection string: {e}")))?;
    let manager = PostgresConnectionManager::new(config, NoTls);
    let pool = r2d2::Pool::builder().max_size(8).build(manager)?;
    // Fail fast: force one real connection now instead of on first query.
    let mut conn = pool.get()?;
    conn.simple_query("SELECT 1")?;
    Ok(pool)
}
