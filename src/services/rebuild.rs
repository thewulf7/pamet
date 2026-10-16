//! Rebuild derived state purely by replaying the `events` log (T041,
//! Constitution Principle I — NON-NEGOTIABLE: any derived store/index MUST
//! be rebuildable from the event log alone).
//!
//! This module never recomputes a write decision or regenerates a random id:
//! every fact it needs (resulting_item_id, target_item_id, rule_name, ...)
//! was already resolved and recorded in the event payload at write time
//! (see `lib.rs::remember_one`). Replay only re-derives embeddings, which
//! are a pure, deterministic function of content (`services::embedding::embed`),
//! so recomputing them here always yields the same vectors as the live path.

use crate::error::{Error, Result};
use crate::services::events::{export_log, Event};
use crate::storage::Store;
use rusqlite::params;

/// Rebuild a brand-new in-memory `Store` by replaying every event from
/// `source` in order. The result is expected to be equivalent (same
/// memory_items / write_decisions / review_queue / retrieval_events /
/// outcome_records rows, modulo embedding/content-derived columns which are
/// recomputed deterministically) to the store that produced the log.
pub fn rebuild_from_events(source: &Store) -> Result<Store> {
    let events = export_log(source, None)?;
    let rebuilt = Store::open_in_memory()?;
    for event in &events {
        apply_event(&rebuilt, event)?;
    }
    Ok(rebuilt)
}

fn apply_event(store: &Store, event: &Event) -> Result<()> {
    // Re-insert the event itself with its original id so every FK reference
    // recorded in later events (event_id columns) still resolves correctly.
    store.conn.execute(
        "INSERT INTO events (id, occurred_at, event_type, payload_json, schema_version) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![event.id, event.occurred_at, event.event_type, event.payload_json, event.schema_version],
    )?;
    let payload: serde_json::Value = serde_json::from_str(&event.payload_json)?;

    match event.event_type.as_str() {
        "write_decision" => apply_write_decision(store, event, &payload)?,
        "retrieval" => apply_retrieval(store, event, &payload)?,
        "outcome_update" => apply_outcome_update(store, event, &payload)?,
        "review_resolution" => apply_review_resolution(store, event, &payload)?,
        "tombstone" => apply_tombstone(store, event, &payload)?,
        other => return Err(Error::MalformedEvent(format!("unknown event_type '{other}' during replay"))),
    }
    Ok(())
}

fn apply_write_decision(store: &Store, event: &Event, payload: &serde_json::Value) -> Result<()> {
    let candidate_content = payload["candidate_content"].as_str().unwrap_or_default();
    let decision = payload["decision"].as_str().unwrap_or_default();
    let rule_name = payload["rule_name"].as_str().unwrap_or_default();
    let rule_inputs = payload["rule_inputs"].to_string();
    let resulting_item_id = payload["resulting_item_id"].as_str().map(|s| s.to_string());
    let target_item_id = payload["target_item_id"].as_str().map(|s| s.to_string());

    match decision {
        "store" => {
            let id = resulting_item_id.as_deref().expect("store decision must record resulting_item_id");
            insert_item_from_replay(store, id, candidate_content, event.id, None)?;
        }
        "supersede" => {
            let id = resulting_item_id.as_deref().expect("supersede decision must record resulting_item_id");
            insert_item_from_replay(store, id, candidate_content, event.id, target_item_id.clone())?;
        }
        "merge" => {
            let target = target_item_id.as_deref().expect("merge decision must record target_item_id");
            store.conn.execute(
                "UPDATE memory_items SET content = ?1 WHERE id = ?2",
                params![candidate_content, target],
            )?;
            store.conn.execute(
                "UPDATE memory_items_fts SET content = ?1 WHERE id = ?2",
                params![candidate_content, target],
            )?;
        }
        "skip" | "review" => {}
        other => return Err(Error::MalformedEvent(format!("unknown decision '{other}' during replay"))),
    }

    store.conn.execute(
        "INSERT INTO write_decisions (event_id, candidate_content, decision, rule_name, \
            rule_inputs_json, resulting_item_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![event.id, candidate_content, decision, rule_name, rule_inputs, resulting_item_id, event.occurred_at],
    )?;
    let write_decision_id = store.conn.last_insert_rowid();

    if decision == "review" {
        let reason = payload["review_reason"].as_str().unwrap_or("low_confidence");
        store.conn.execute(
            "INSERT INTO review_queue (write_decision_id, reason, status, created_at) \
             VALUES (?1, ?2, 'pending', ?3)",
            params![write_decision_id, reason, event.occurred_at],
        )?;
    }
    Ok(())
}

fn insert_item_from_replay(
    store: &Store,
    id: &str,
    content: &str,
    provenance_event_id: i64,
    supersedes_id: Option<String>,
) -> Result<()> {
    let embedding = crate::services::embedding::embed(content);
    store.conn.execute(
        "INSERT INTO memory_items (id, item_type, content, embedding_json, entities_json, \
            confidence, provenance_event_id, supersedes_id, created_at, use_count) \
         VALUES (?1, 'fact', ?2, ?3, '[]', 0.9, ?4, ?5, (SELECT occurred_at FROM events WHERE id = ?4), 0)",
        params![id, content, serde_json::to_string(&embedding)?, provenance_event_id, supersedes_id],
    )?;
    store.conn.execute(
        "INSERT INTO memory_items_fts (id, content) VALUES (?1, ?2)",
        params![id, content],
    )?;
    Ok(())
}

fn apply_retrieval(store: &Store, event: &Event, payload: &serde_json::Value) -> Result<()> {
    store.conn.execute(
        "INSERT INTO retrieval_events (event_id, query_text, vector_candidates_json, \
            keyword_candidates_json, fused_ranking_json, result_budget, returned_item_ids_json, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            event.id,
            payload["query_text"].as_str().unwrap_or_default(),
            payload["vector_candidates"].to_string(),
            payload["keyword_candidates"].to_string(),
            payload["fused_ranking"].to_string(),
            payload["result_budget"].as_i64().unwrap_or(0),
            payload["returned_item_ids"].to_string(),
            event.occurred_at,
        ],
    )?;
    Ok(())
}

fn apply_outcome_update(store: &Store, event: &Event, payload: &serde_json::Value) -> Result<()> {
    let retrieval_event_id = payload["retrieval_event_id"].as_i64().unwrap_or_default();
    let item_id = payload["item_id"].as_str().unwrap_or_default();
    let used = payload["used"].as_bool().unwrap_or(false);
    store.conn.execute(
        "INSERT INTO outcome_records (retrieval_event_id, item_id, used, reported_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![retrieval_event_id, item_id, used as i64, event.occurred_at],
    )?;
    if used {
        store.conn.execute(
            "UPDATE memory_items SET use_count = use_count + 1 WHERE id = ?1",
            params![item_id],
        )?;
    }
    Ok(())
}

fn apply_review_resolution(store: &Store, event: &Event, payload: &serde_json::Value) -> Result<()> {
    let review_id = payload["review_id"].as_i64().unwrap_or_default();
    let resolved_decision = payload["resolved_decision"].as_str().unwrap_or_default();
    let status = match resolved_decision {
        "store" => "resolved_store",
        "merge" => "resolved_merge",
        "supersede" => "resolved_supersede",
        "skip" => "resolved_skip",
        other => return Err(Error::MalformedEvent(format!("unknown resolved_decision '{other}'"))),
    };
    store.conn.execute(
        "UPDATE review_queue SET status = ?1, resolved_at = ?2 WHERE id = ?3",
        params![status, event.occurred_at, review_id],
    )?;
    Ok(())
}

fn apply_tombstone(store: &Store, event: &Event, payload: &serde_json::Value) -> Result<()> {
    let item_id = payload["item_id"].as_str().unwrap_or_default();
    store.conn.execute(
        "UPDATE memory_items SET tombstoned_at = ?1 WHERE id = ?2",
        params![event.occurred_at, item_id],
    )?;
    Ok(())
}
