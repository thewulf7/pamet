//! Review queue resolution (T017).

use crate::error::{Error, Result};
use crate::models::{Decision, ReviewStatus};
use crate::storage::{Backend, Store};
use chrono::Utc;
use rusqlite::params;

pub struct ReviewQueueRow {
    pub id: i64,
    pub write_decision_id: i64,
    pub reason: String,
    pub status: String,
    pub created_at: String,
}

pub fn list(store: &Store, status_filter: Option<&str>) -> Result<Vec<ReviewQueueRow>> {
    let mut out = Vec::new();
    match &store.backend {
        Backend::Sqlite(conn) => {
            let sql = match status_filter {
                Some(_) => {
                    "SELECT id, write_decision_id, reason, status, created_at FROM review_queue \
                     WHERE status = ?1 ORDER BY id ASC"
                }
                None => "SELECT id, write_decision_id, reason, status, created_at FROM review_queue ORDER BY id ASC",
            };
            let mut stmt = conn.prepare(sql)?;
            let mapper = |row: &rusqlite::Row| -> rusqlite::Result<ReviewQueueRow> {
                Ok(ReviewQueueRow {
                    id: row.get(0)?,
                    write_decision_id: row.get(1)?,
                    reason: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                })
            };
            let rows = match status_filter {
                Some(s) => stmt.query_map(params![s], mapper)?,
                None => stmt.query_map(params![], mapper)?,
            };
            for r in rows {
                out.push(r?);
            }
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            let rows = match status_filter {
                Some(s) => conn.query(
                    "SELECT id, write_decision_id, reason, status, created_at FROM review_queue \
                     WHERE status = $1 ORDER BY id ASC",
                    &[&s],
                )?,
                None => conn.query(
                    "SELECT id, write_decision_id, reason, status, created_at FROM review_queue ORDER BY id ASC",
                    &[],
                )?,
            };
            for row in rows {
                out.push(ReviewQueueRow {
                    id: row.get(0),
                    write_decision_id: row.get(1),
                    reason: row.get(2),
                    status: row.get(3),
                    created_at: row.get(4),
                });
            }
        }
    }
    Ok(out)
}

/// Resolve a pending review entry (T017). Returns Err(NotPending) if it was
/// already resolved.
pub fn resolve(store: &Store, review_id: i64, decision: Decision) -> Result<()> {
    let status: String = match &store.backend {
        Backend::Sqlite(conn) => conn
            .query_row(
                "SELECT status FROM review_queue WHERE id = ?1",
                params![review_id],
                |row| row.get(0),
            )
            .map_err(|_| Error::NotPending(review_id))?,
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get().map_err(|_| Error::NotPending(review_id))?;
            conn.query_one(
                "SELECT status FROM review_queue WHERE id = $1",
                &[&review_id],
            )
            .map(|row| row.get(0))
            .map_err(|_| Error::NotPending(review_id))?
        }
    };
    if status != "pending" {
        return Err(Error::NotPending(review_id));
    }
    let new_status = ReviewStatus::from_decision(decision);
    let now = Utc::now().to_rfc3339();
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "UPDATE review_queue SET status = ?1, resolved_at = ?2 WHERE id = ?3",
                params![new_status.as_str(), now, review_id],
            )?;
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.execute(
                "UPDATE review_queue SET status = $1, resolved_at = $2 WHERE id = $3",
                &[&new_status.as_str(), &now, &review_id],
            )?;
        }
    }

    let payload = serde_json::json!({
        "review_id": review_id,
        "resolved_decision": decision.as_str(),
    });
    crate::services::events::append_event(store, "review_resolution", &payload)?;
    Ok(())
}
