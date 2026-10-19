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
//!
//! `rebuild_into` (T0xx, feature 003) reuses this exact replay machinery for
//! the `pamet migrate` subcommand — migrating to a new backend is "rebuild
//! into a store that happens to be the new one" rather than a bespoke copy
//! tool, a direct application of Principle I to the migration problem.

use crate::error::{Error, Result};
use crate::services::events::{export_log, Event};
use crate::storage::{Backend, Store};
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

/// Replay `source`'s event log into an already-open `target` store (which
/// may be a different backend — e.g. Postgres) — the core of `pamet
/// migrate`. Returns the number of events replayed.
pub fn rebuild_into(source: &Store, target: &Store) -> Result<usize> {
    let events = export_log(source, None)?;
    for event in &events {
        apply_event(target, event)?;
    }
    Ok(events.len())
}

fn apply_event(store: &Store, event: &Event) -> Result<()> {
    // Re-insert the event itself with its original id so every FK reference
    // recorded in later events (event_id columns) still resolves correctly.
    crate::services::events::append_event_with_id(
        store,
        event.id,
        &event.occurred_at,
        &event.event_type,
        &event.payload_json,
        event.schema_version,
    )?;
    let payload: serde_json::Value = serde_json::from_str(&event.payload_json)?;

    match event.event_type.as_str() {
        "write_decision" => apply_write_decision(store, event, &payload)?,
        "retrieval" => apply_retrieval(store, event, &payload)?,
        "outcome_update" => apply_outcome_update(store, event, &payload)?,
        "review_resolution" => apply_review_resolution(store, event, &payload)?,
        "tombstone" => apply_tombstone(store, event, &payload)?,
        "consolidation_run_started" => apply_consolidation_run_started(store, &payload)?,
        "consolidation_run_finished" => apply_consolidation_run_finished(store, &payload)?,
        "cluster_formed" => {} // informational only — no row of its own; summary_created does the writes
        "summary_created" => apply_summary_created(store, event, &payload)?,
        "eval_run_completed" => apply_eval_run_completed(store, &payload)?,
        "item_decayed" => {} // T047: decay is a pure read-time computation, not stored state —
        // the event itself (already re-inserted above) is the full audit trail
        other => {
            return Err(Error::MalformedEvent(format!(
                "unknown event_type '{other}' during replay"
            )))
        }
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
            let id = resulting_item_id
                .as_deref()
                .expect("store decision must record resulting_item_id");
            insert_item_from_replay(
                store,
                id,
                candidate_content,
                event.id,
                None,
                &event.occurred_at,
            )?;
        }
        "supersede" => {
            let id = resulting_item_id
                .as_deref()
                .expect("supersede decision must record resulting_item_id");
            insert_item_from_replay(
                store,
                id,
                candidate_content,
                event.id,
                target_item_id.clone(),
                &event.occurred_at,
            )?;
        }
        "merge" => {
            let target = target_item_id
                .as_deref()
                .expect("merge decision must record target_item_id");
            match &store.backend {
                Backend::Sqlite(conn) => {
                    conn.execute(
                        "UPDATE memory_items SET content = ?1 WHERE id = ?2",
                        params![candidate_content, target],
                    )?;
                    conn.execute(
                        "UPDATE memory_items_fts SET content = ?1 WHERE id = ?2",
                        params![candidate_content, target],
                    )?;
                }
                #[cfg(feature = "postgres-backend")]
                Backend::Postgres(pool) => {
                    let mut conn = pool.get()?;
                    conn.execute(
                        "UPDATE memory_items SET content = $1 WHERE id = $2",
                        &[&candidate_content, &target],
                    )?;
                }
            }
        }
        "skip" | "review" => {}
        other => {
            return Err(Error::MalformedEvent(format!(
                "unknown decision '{other}' during replay"
            )))
        }
    }

    let write_decision_id = match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "INSERT INTO write_decisions (event_id, candidate_content, decision, rule_name, \
                    rule_inputs_json, resulting_item_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![event.id, candidate_content, decision, rule_name, rule_inputs, resulting_item_id, event.occurred_at],
            )?;
            conn.last_insert_rowid()
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            let row = conn.query_one(
                "INSERT INTO write_decisions (event_id, candidate_content, decision, rule_name, \
                    rule_inputs_json, resulting_item_id, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
                &[&event.id, &candidate_content, &decision, &rule_name, &rule_inputs, &resulting_item_id, &event.occurred_at],
            )?;
            row.get::<_, i64>(0)
        }
    };

    if decision == "review" {
        let reason = payload["review_reason"]
            .as_str()
            .unwrap_or("low_confidence");
        match &store.backend {
            Backend::Sqlite(conn) => {
                conn.execute(
                    "INSERT INTO review_queue (write_decision_id, reason, status, created_at) \
                     VALUES (?1, ?2, 'pending', ?3)",
                    params![write_decision_id, reason, event.occurred_at],
                )?;
            }
            #[cfg(feature = "postgres-backend")]
            Backend::Postgres(pool) => {
                let mut conn = pool.get()?;
                conn.execute(
                    "INSERT INTO review_queue (write_decision_id, reason, status, created_at) \
                     VALUES ($1, $2, 'pending', $3)",
                    &[&write_decision_id, &reason, &event.occurred_at],
                )?;
            }
        }
    }
    Ok(())
}

fn insert_item_from_replay(
    store: &Store,
    id: &str,
    content: &str,
    provenance_event_id: i64,
    supersedes_id: Option<String>,
    #[allow(unused_variables)] occurred_at: &str,
) -> Result<()> {
    let embedding = crate::services::embedding::embed(content);
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "INSERT INTO memory_items (id, item_type, content, embedding_json, entities_json, \
                    confidence, provenance_event_id, supersedes_id, created_at, use_count) \
                 VALUES (?1, 'fact', ?2, ?3, '[]', 0.9, ?4, ?5, (SELECT occurred_at FROM events WHERE id = ?4), 0)",
                params![id, content, serde_json::to_string(&embedding)?, provenance_event_id, supersedes_id],
            )?;
            conn.execute(
                "INSERT INTO memory_items_fts (id, content) VALUES (?1, ?2)",
                params![id, content],
            )?;
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            let vector = pgvector::Vector::from(embedding);
            conn.execute(
                "INSERT INTO memory_items (id, item_type, content, embedding, entities_json, \
                    confidence, provenance_event_id, supersedes_id, created_at, use_count) \
                 VALUES ($1, 'fact', $2, $3, '[]', 0.9, $4, $5, $6, 0)",
                &[
                    &id,
                    &content,
                    &vector,
                    &provenance_event_id,
                    &supersedes_id,
                    &occurred_at,
                ],
            )?;
        }
    }
    Ok(())
}

fn apply_retrieval(store: &Store, event: &Event, payload: &serde_json::Value) -> Result<()> {
    let query_text = payload["query_text"].as_str().unwrap_or_default();
    let vector_candidates = payload["vector_candidates"].to_string();
    let keyword_candidates = payload["keyword_candidates"].to_string();
    let fused_ranking = payload["fused_ranking"].to_string();
    let result_budget = payload["result_budget"].as_i64().unwrap_or(0);
    let returned_item_ids = payload["returned_item_ids"].to_string();
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "INSERT INTO retrieval_events (event_id, query_text, vector_candidates_json, \
                    keyword_candidates_json, fused_ranking_json, result_budget, returned_item_ids_json, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![event.id, query_text, vector_candidates, keyword_candidates, fused_ranking, result_budget, returned_item_ids, event.occurred_at],
            )?;
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.execute(
                "INSERT INTO retrieval_events (event_id, query_text, vector_candidates_json, \
                    keyword_candidates_json, fused_ranking_json, result_budget, returned_item_ids_json, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                &[&event.id, &query_text, &vector_candidates, &keyword_candidates, &fused_ranking, &(result_budget as i32), &returned_item_ids, &event.occurred_at],
            )?;
        }
    }
    Ok(())
}

fn apply_outcome_update(store: &Store, event: &Event, payload: &serde_json::Value) -> Result<()> {
    let retrieval_event_id = payload["retrieval_event_id"].as_i64().unwrap_or_default();
    let item_id = payload["item_id"].as_str().unwrap_or_default();
    let used = payload["used"].as_bool().unwrap_or(false);
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "INSERT INTO outcome_records (retrieval_event_id, item_id, used, reported_at) VALUES (?1, ?2, ?3, ?4)",
                params![retrieval_event_id, item_id, used as i64, event.occurred_at],
            )?;
            if used {
                conn.execute(
                    "UPDATE memory_items SET use_count = use_count + 1 WHERE id = ?1",
                    params![item_id],
                )?;
            }
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.execute(
                "INSERT INTO outcome_records (retrieval_event_id, item_id, used, reported_at) VALUES ($1, $2, $3, $4)",
                &[&retrieval_event_id, &item_id, &(used as i32), &event.occurred_at],
            )?;
            if used {
                conn.execute(
                    "UPDATE memory_items SET use_count = use_count + 1 WHERE id = $1",
                    &[&item_id],
                )?;
            }
        }
    }
    Ok(())
}

fn apply_review_resolution(
    store: &Store,
    event: &Event,
    payload: &serde_json::Value,
) -> Result<()> {
    let review_id = payload["review_id"].as_i64().unwrap_or_default();
    let resolved_decision = payload["resolved_decision"].as_str().unwrap_or_default();
    let status = match resolved_decision {
        "store" => "resolved_store",
        "merge" => "resolved_merge",
        "supersede" => "resolved_supersede",
        "skip" => "resolved_skip",
        other => {
            return Err(Error::MalformedEvent(format!(
                "unknown resolved_decision '{other}'"
            )))
        }
    };
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "UPDATE review_queue SET status = ?1, resolved_at = ?2 WHERE id = ?3",
                params![status, event.occurred_at, review_id],
            )?;
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.execute(
                "UPDATE review_queue SET status = $1, resolved_at = $2 WHERE id = $3",
                &[&status, &event.occurred_at, &review_id],
            )?;
        }
    }
    Ok(())
}

fn apply_tombstone(store: &Store, event: &Event, payload: &serde_json::Value) -> Result<()> {
    let item_id = payload["item_id"].as_str().unwrap_or_default();
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "UPDATE memory_items SET tombstoned_at = ?1 WHERE id = ?2",
                params![event.occurred_at, item_id],
            )?;
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.execute(
                "UPDATE memory_items SET tombstoned_at = $1 WHERE id = $2",
                &[&event.occurred_at, &item_id],
            )?;
        }
    }
    Ok(())
}

fn apply_consolidation_run_started(store: &Store, payload: &serde_json::Value) -> Result<()> {
    let run_id = payload["run_id"].as_str().unwrap_or_default();
    let started_at = payload["started_at"].as_str().unwrap_or_default();
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute("INSERT INTO consolidation_runs (id, started_at, status) VALUES (?1, ?2, 'running')", params![run_id, started_at])?;
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.execute("INSERT INTO consolidation_runs (id, started_at, status) VALUES ($1, $2, 'running')", &[&run_id, &started_at])?;
        }
    }
    Ok(())
}

fn apply_consolidation_run_finished(store: &Store, payload: &serde_json::Value) -> Result<()> {
    let run_id = payload["run_id"].as_str().unwrap_or_default();
    let finished_at = payload["finished_at"].as_str().unwrap_or_default();
    let items_processed = payload["items_processed"].as_i64().unwrap_or(0);
    let clusters_formed = payload["clusters_formed"].as_i64().unwrap_or(0);
    let contradictions_flagged = payload["contradictions_flagged"].as_i64().unwrap_or(0);
    let status = payload["status"].as_str().unwrap_or("completed");
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "UPDATE consolidation_runs SET finished_at = ?1, items_processed = ?2, clusters_formed = ?3, \
                    contradictions_flagged = ?4, status = ?5 WHERE id = ?6",
                params![finished_at, items_processed, clusters_formed, contradictions_flagged, status, run_id],
            )?;
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.execute(
                "UPDATE consolidation_runs SET finished_at = $1, items_processed = $2, clusters_formed = $3, \
                    contradictions_flagged = $4, status = $5 WHERE id = $6",
                &[&finished_at, &(items_processed as i32), &(clusters_formed as i32), &(contradictions_flagged as i32), &status, &run_id],
            )?;
        }
    }
    Ok(())
}

/// Recreates the summary `memory_items` row and every `cluster_members`
/// link — the `cluster_formed` event that preceded this one in the log
/// carries no row of its own (research.md §1-2: everything a cluster needs
/// to be rebuilt is captured in this one event's payload).
fn apply_summary_created(store: &Store, event: &Event, payload: &serde_json::Value) -> Result<()> {
    let run_id = payload["run_id"].as_str().unwrap_or_default();
    let cluster_id = payload["cluster_id"].as_str().unwrap_or_default();
    let summary_item_id = payload["summary_item_id"].as_str().unwrap_or_default();
    let summary_content = payload["summary_content"].as_str().unwrap_or_default();
    let member_item_ids: Vec<String> = payload["member_item_ids"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let embedding = crate::services::embedding::embed(summary_content);
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "INSERT INTO memory_items (id, item_type, content, embedding_json, entities_json, confidence, \
                    provenance_event_id, supersedes_id, created_at, use_count) \
                 VALUES (?1, 'summary', ?2, ?3, '[]', 0.9, ?4, NULL, ?5, 0)",
                params![summary_item_id, summary_content, serde_json::to_string(&embedding)?, event.id, event.occurred_at],
            )?;
            conn.execute(
                "INSERT INTO memory_items_fts (id, content) VALUES (?1, ?2)",
                params![summary_item_id, summary_content],
            )?;
            for member_id in &member_item_ids {
                conn.execute(
                    "INSERT INTO cluster_members (id, cluster_id, summary_item_id, source_item_id, consolidation_run_id, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![uuid::Uuid::new_v4().to_string(), cluster_id, summary_item_id, member_id, run_id, event.occurred_at],
                )?;
            }
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            let vector = pgvector::Vector::from(embedding);
            conn.execute(
                "INSERT INTO memory_items (id, item_type, content, embedding, entities_json, confidence, \
                    provenance_event_id, supersedes_id, created_at, use_count) \
                 VALUES ($1, 'summary', $2, $3, '[]', 0.9, $4, NULL, $5, 0)",
                &[&summary_item_id, &summary_content, &vector, &event.id, &event.occurred_at],
            )?;
            for member_id in &member_item_ids {
                conn.execute(
                    "INSERT INTO cluster_members (id, cluster_id, summary_item_id, source_item_id, consolidation_run_id, created_at) \
                     VALUES ($1, $2, $3, $4, $5, $6)",
                    &[&uuid::Uuid::new_v4().to_string(), &cluster_id, &summary_item_id, member_id, &run_id, &event.occurred_at],
                )?;
            }
        }
    }
    // Replay the review gate the same way the live path creates it (T041) —
    // same write_decisions + review_queue rows, so a rebuilt store's
    // pending/resolved review state matches the live one exactly.
    let write_decision_id =
        match &store.backend {
            Backend::Sqlite(conn) => {
                conn.execute(
                "INSERT INTO write_decisions (event_id, candidate_content, decision, rule_name, \
                    rule_inputs_json, resulting_item_id, created_at) VALUES (?1, ?2, 'review', \
                    'consolidation_cluster_review', '{}', ?3, ?4)",
                params![event.id, summary_content, summary_item_id, event.occurred_at],
            )?;
                conn.last_insert_rowid()
            }
            #[cfg(feature = "postgres-backend")]
            Backend::Postgres(pool) => {
                let mut conn = pool.get()?;
                let row = conn.query_one(
                "INSERT INTO write_decisions (event_id, candidate_content, decision, rule_name, \
                    rule_inputs_json, resulting_item_id, created_at) VALUES ($1, $2, 'review', \
                    'consolidation_cluster_review', '{}', $3, $4) RETURNING id",
                &[&event.id, &summary_content, &summary_item_id, &event.occurred_at],
            )?;
                row.get::<_, i64>(0)
            }
        };
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "INSERT INTO review_queue (write_decision_id, reason, status, created_at) VALUES (?1, 'consolidation_cluster_promotion', 'pending', ?2)",
                params![write_decision_id, event.occurred_at],
            )?;
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.execute(
                "INSERT INTO review_queue (write_decision_id, reason, status, created_at) VALUES ($1, 'consolidation_cluster_promotion', 'pending', $2)",
                &[&write_decision_id, &event.occurred_at],
            )?;
        }
    }
    Ok(())
}

/// Eval runs are read-only probes, not replayed state (data-model.md); this
/// just mirrors the audit-trail row so `export-log` + replay stay
/// consistent with what a fresh `pamet eval` run would have written.
fn apply_eval_run_completed(store: &Store, payload: &serde_json::Value) -> Result<()> {
    let report_id = payload["report_id"].as_str().unwrap_or_default();
    let probe_set_version = payload["probe_set_version"].as_str().unwrap_or_default();
    let generated_at = payload["generated_at"].as_str().unwrap_or_default();
    let pamet_results_json = payload["pamet_results_json"].as_str().unwrap_or_default();
    let hindsight_results_json = payload["hindsight_results_json"].as_str();
    let hindsight_available = payload["hindsight_available"].as_bool().unwrap_or(false);
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "INSERT INTO baseline_reports (id, probe_set_version, generated_at, pamet_results_json, \
                    hindsight_results_json, hindsight_available) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![report_id, probe_set_version, generated_at, pamet_results_json, hindsight_results_json, hindsight_available as i64],
            )?;
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.execute(
                "INSERT INTO baseline_reports (id, probe_set_version, generated_at, pamet_results_json, \
                    hindsight_results_json, hindsight_available) VALUES ($1, $2, $3, $4, $5, $6)",
                &[&report_id, &probe_set_version, &generated_at, &pamet_results_json, &hindsight_results_json, &(hindsight_available as i32)],
            )?;
        }
    }
    // T045: replay the per-probe eval_results rows from the same event.
    if let Some(outcomes) = payload["probe_outcomes"].as_array() {
        for outcome in outcomes {
            let probe_id = outcome["probe_id"].as_str().unwrap_or_default();
            let category = outcome["category"].as_str().unwrap_or_default();
            let passed = outcome["passed"].as_bool().unwrap_or(false);
            match &store.backend {
                Backend::Sqlite(conn) => {
                    conn.execute(
                        "INSERT INTO eval_results (id, run_id, probe_id, category, probe_set_version, passed, ran_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![uuid::Uuid::new_v4().to_string(), report_id, probe_id, category, probe_set_version, passed as i64, generated_at],
                    )?;
                }
                #[cfg(feature = "postgres-backend")]
                Backend::Postgres(pool) => {
                    let mut conn = pool.get()?;
                    conn.execute(
                        "INSERT INTO eval_results (id, run_id, probe_id, category, probe_set_version, passed, ran_at) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7)",
                        &[&uuid::Uuid::new_v4().to_string(), &report_id, &probe_id, &category, &probe_set_version, &(passed as i32), &generated_at],
                    )?;
                }
            }
        }
    }
    Ok(())
}
