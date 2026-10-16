//! Append-only event log: source of truth (Constitution Principle I). T006, T033, T039.

use crate::error::{Error, Result};
use crate::storage::Store;
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub occurred_at: String,
    pub event_type: String,
    pub payload_json: String,
    pub schema_version: i64,
}

const VALID_EVENT_TYPES: [&str; 5] = [
    "write_decision",
    "retrieval",
    "outcome_update",
    "review_resolution",
    "tombstone",
];

/// Append one event to the log. Fails closed on an unknown event_type or
/// payload that is not valid JSON (T039 — never partially apply).
pub fn append_event(store: &Store, event_type: &str, payload: &serde_json::Value) -> Result<i64> {
    if !VALID_EVENT_TYPES.contains(&event_type) {
        return Err(Error::MalformedEvent(format!(
            "unknown event_type '{event_type}'"
        )));
    }
    let payload_json = serde_json::to_string(payload)?;
    let now = Utc::now().to_rfc3339();
    store.conn.execute(
        "INSERT INTO events (occurred_at, event_type, payload_json, schema_version) VALUES (?1, ?2, ?3, 1)",
        params![now, event_type, payload_json],
    )?;
    Ok(store.conn.last_insert_rowid())
}

/// Replay-ordered export of the event log, optionally filtered by `since` (T033).
pub fn export_log(store: &Store, since: Option<&str>) -> Result<Vec<Event>> {
    let mut rows = Vec::new();
    let sql = match since {
        Some(_) => {
            "SELECT id, occurred_at, event_type, payload_json, schema_version FROM events \
             WHERE occurred_at >= ?1 ORDER BY id ASC"
        }
        None => {
            "SELECT id, occurred_at, event_type, payload_json, schema_version FROM events \
             ORDER BY id ASC"
        }
    };
    let mut stmt = store.conn.prepare(sql)?;
    let mapper = |row: &rusqlite::Row| -> rusqlite::Result<Event> {
        Ok(Event {
            id: row.get(0)?,
            occurred_at: row.get(1)?,
            event_type: row.get(2)?,
            payload_json: row.get(3)?,
            schema_version: row.get(4)?,
        })
    };
    let iter = match since {
        Some(s) => stmt.query_map(params![s], mapper)?,
        None => stmt.query_map(params![], mapper)?,
    };
    for row in iter {
        rows.push(row?);
    }
    Ok(rows)
}
