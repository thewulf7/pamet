//! Schema migration for databases created before this feature shipped
//! (feature 008, discovered during implementation — see spec.md Results).
//!
//! `events.event_type`'s `CHECK` constraint is baked in at table-creation
//! time; `CREATE TABLE IF NOT EXISTS` (store.rs) is a no-op against an
//! already-existing table, so any pre-existing Pamet database (including,
//! at discovery time, this project's own live `~/.hermes/pamet/memory.db`
//! backing the Hermes plugin) still rejects the new `cluster_gram_model_built`
//! event type with a `CHECK constraint failed` error the instant
//! consolidation tries to persist a gram model. This is a pre-existing gap
//! (no schema-migration framework existed before this), newly surfaced
//! because this is the first event type added since real databases
//! accumulated real data — not a recurring bug in prior features.
//!
//! Idempotent and runs on every `Store::open()`: a no-op if the live
//! constraint already accepts the new type (true for any brand-new
//! database, since `SQLITE_SCHEMA_SQL`/`POSTGRES_SCHEMA_SQL` already
//! include it).

use crate::error::Result;
use rusqlite::Connection;

/// Rewrites the `events.event_type` `CHECK` constraint by directly editing
/// its stored `CREATE TABLE` text in `sqlite_master` via `writable_schema`
/// (SQLite's own documented technique for constraint changes it has no
/// `ALTER TABLE` syntax for) — no table rename, no row copy, no `DROP
/// TABLE`. Matters in practice, not just in theory: an earlier version of
/// this migration used the textbook rename→recreate→copy→drop approach,
/// which is correct but was measured to take minutes against this
/// project's own real, already-populated `~/.hermes/pamet/memory.db`
/// (bundled SQLite 3.45's `DROP TABLE` on a tens-of-thousands-of-rows table
/// was dramatically slower there than the system `sqlite3` CLI's SQLite
/// 3.54 on the identical data — a library-version/build difference, not a
/// logic bug). This version is a single near-instant metadata edit
/// regardless of table size.
pub fn ensure_events_check_constraint_current_sqlite(conn: &Connection) -> Result<()> {
    let sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='events'",
            [],
            |r| r.get(0),
        )
        .ok();
    let Some(sql) = sql else {
        return Ok(()); // events table doesn't exist yet — nothing to migrate.
    };
    if sql.contains("embedding_backfilled") {
        return Ok(()); // already current.
    }

    const NEW_EVENTS_TABLE_SQL: &str = "CREATE TABLE events (
             id            INTEGER PRIMARY KEY AUTOINCREMENT,
             occurred_at   TEXT    NOT NULL,
             event_type    TEXT    NOT NULL CHECK (event_type IN
                               ('write_decision','retrieval','outcome_update','review_resolution','tombstone',
                                'cluster_formed','summary_created','consolidation_run_started',
                                'consolidation_run_finished','eval_run_completed','item_decayed',
                                'hindsight_memory_migrated','cluster_gram_model_built','embedding_backfilled')),
             payload_json  TEXT    NOT NULL,
             schema_version INTEGER NOT NULL DEFAULT 1
         )";

    // The `writable_schema` edit alone only affects *future* connections —
    // this one's in-memory schema cache (parsed at open time) stays stale
    // until something tells it the schema changed. Bumping
    // `PRAGMA schema_version` is SQLite's own signal for that: it forces a
    // reparse on next access, so the new CHECK constraint takes effect on
    // *this* connection immediately, not just the next `Store::open()`.
    let schema_version: i64 = conn.query_row("PRAGMA schema_version;", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA writable_schema = ON;")?;
    let result = conn
        .execute(
            "UPDATE sqlite_master SET sql = ?1 WHERE type='table' AND name='events'",
            [NEW_EVENTS_TABLE_SQL],
        )
        .and_then(|_| {
            conn.execute_batch(&format!("PRAGMA schema_version = {};", schema_version + 1))
        });
    // Always try to turn writable_schema back off, even on error — leaving
    // it on is a correctness hazard for every later statement on this
    // connection, worse than the original migration failure.
    conn.execute_batch("PRAGMA writable_schema = OFF;")?;
    result?;
    Ok(())
}

/// Adds `memory_items.embedding_model_id` to databases created before
/// feature 009 — same class of gap as the events `CHECK` constraint above:
/// `CREATE TABLE IF NOT EXISTS` (schema.rs) only affects brand-new databases.
/// `ALTER TABLE ... ADD COLUMN` has no `IF NOT EXISTS` form in SQLite, so the
/// idempotence check is a `PRAGMA table_info` existence probe instead.
pub fn ensure_memory_items_embedding_model_id_column_sqlite(conn: &Connection) -> Result<()> {
    let has_table: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='memory_items'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|_| true)
        .unwrap_or(false);
    if !has_table {
        return Ok(()); // memory_items doesn't exist yet — nothing to migrate.
    }
    let already_present: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('memory_items') WHERE name = 'embedding_model_id'")?
        .query_row([], |r| r.get::<_, i64>(0))
        .map(|_| true)
        .unwrap_or(false);
    if already_present {
        return Ok(()); // already current.
    }
    conn.execute_batch(
        "ALTER TABLE memory_items ADD COLUMN embedding_model_id TEXT REFERENCES embedding_models(id);",
    )?;
    Ok(())
}

/// Postgres equivalent — `ADD COLUMN IF NOT EXISTS` makes this idempotent
/// natively, no probe needed.
#[cfg(feature = "postgres-backend")]
pub fn ensure_memory_items_embedding_model_id_column_postgres(
    pool: &crate::storage::postgres_pool::PgPool,
) -> Result<()> {
    let mut conn = pool.get()?;
    conn.batch_execute(
        "ALTER TABLE memory_items ADD COLUMN IF NOT EXISTS embedding_model_id TEXT;",
    )?;
    Ok(())
}

/// Postgres supports `ALTER TABLE ... DROP/ADD CONSTRAINT` directly, so no
/// table rebuild is needed — just replace the constraint if it's stale.
#[cfg(feature = "postgres-backend")]
pub fn ensure_events_check_constraint_current_postgres(
    pool: &crate::storage::postgres_pool::PgPool,
) -> Result<()> {
    let mut conn = pool.get()?;
    let row = conn.query_opt(
        "SELECT 1 FROM information_schema.check_constraints \
         WHERE constraint_schema = current_schema() \
           AND check_clause LIKE '%embedding_backfilled%'",
        &[],
    )?;
    if row.is_some() {
        return Ok(()); // already current.
    }
    conn.batch_execute(
        "ALTER TABLE events DROP CONSTRAINT IF EXISTS events_event_type_check; \
         ALTER TABLE events ADD CONSTRAINT events_event_type_check CHECK (event_type IN \
             ('write_decision','retrieval','outcome_update','review_resolution','tombstone', \
              'cluster_formed','summary_created','consolidation_run_started', \
              'consolidation_run_finished','eval_run_completed','item_decayed', \
              'hindsight_memory_migrated','cluster_gram_model_built','embedding_backfilled'));",
    )?;
    Ok(())
}
