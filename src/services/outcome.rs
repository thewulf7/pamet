//! Attach after-the-fact usage feedback to a retrieval event (T032).
//! Never creates a disconnected new retrieval record (FR-011).

use crate::error::{Error, Result};
use crate::storage::{Backend, Store};
use chrono::Utc;
use rusqlite::params;

pub fn report_outcome(store: &Store, event_id: i64, item_id: &str, used: bool) -> Result<()> {
    let returned_json: Option<String> = match &store.backend {
        Backend::Sqlite(conn) => conn
            .query_row(
                "SELECT returned_item_ids_json FROM retrieval_events WHERE event_id = ?1",
                params![event_id],
                |row| row.get(0),
            )
            .ok(),
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.query_one(
                "SELECT returned_item_ids_json FROM retrieval_events WHERE event_id = $1",
                &[&event_id],
            )
            .ok()
            .map(|row| row.get(0))
        }
    };
    let returned_json = returned_json.ok_or_else(|| Error::UnknownItemForEvent {
        event_id,
        item_id: item_id.to_string(),
    })?;
    let returned: Vec<String> = serde_json::from_str(&returned_json)?;
    if !returned.contains(&item_id.to_string()) {
        return Err(Error::UnknownItemForEvent {
            event_id,
            item_id: item_id.to_string(),
        });
    }

    let now = Utc::now().to_rfc3339();
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "INSERT INTO outcome_records (retrieval_event_id, item_id, used, reported_at) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![event_id, item_id, used as i64, now],
            )?;
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.execute(
                "INSERT INTO outcome_records (retrieval_event_id, item_id, used, reported_at) \
                 VALUES ($1, $2, $3, $4)",
                &[&event_id, &item_id, &(used as i32), &now],
            )?;
        }
    }

    let payload = serde_json::json!({
        "retrieval_event_id": event_id,
        "item_id": item_id,
        "used": used,
    });
    crate::services::events::append_event(store, "outcome_update", &payload)?;

    if used {
        match &store.backend {
            Backend::Sqlite(conn) => {
                conn.execute(
                    "UPDATE memory_items SET use_count = use_count + 1, last_used_at = ?1 WHERE id = ?2",
                    params![now, item_id],
                )?;
            }
            #[cfg(feature = "postgres-backend")]
            Backend::Postgres(pool) => {
                let mut conn = pool.get()?;
                conn.execute(
                    "UPDATE memory_items SET use_count = use_count + 1, last_used_at = $1 WHERE id = $2",
                    &[&now, &item_id],
                )?;
            }
        }
    }
    Ok(())
}
