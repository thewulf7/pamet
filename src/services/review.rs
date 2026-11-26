//! Review queue resolution (T017). SQLite-only — postgres-backend feature removed.

use crate::error::{Error, Result};
use crate::models::{Decision, ReviewStatus};
use crate::storage::{Backend, Store};
use chrono::Utc;
use rusqlite::params;

/// T008 [US3]: Enriched review row that includes summary text and cluster source count —
/// avoids the need to run separate queries to understand what a pending review item is about.
/// T027 [Convergence]: also carries cluster_id per FR-005.
pub struct EnrichedReviewRow {
    pub id: i64,
    pub reason: String,
    pub status: String,
    pub created_at: String,
    pub summary_item_id: Option<String>,
    pub summary_text: Option<String>,
    pub source_count: i64,
    pub cluster_id: Option<String>,
}

pub struct ReviewQueueRow {
    pub id: i64,
    pub write_decision_id: i64,
    pub reason: String,
    pub status: String,
    pub created_at: String,
}

pub fn list(store: &Store, status_filter: Option<&str>) -> Result<Vec<ReviewQueueRow>> {
    let mut out = Vec::new();
    let Backend::Sqlite(conn) = &store.backend;
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
    Ok(out)
}

/// Resolve a pending review entry (T017). Returns Err(NotPending) if it was
/// already resolved.
pub fn resolve(store: &Store, review_id: i64, decision: Decision) -> Result<()> {
    let Backend::Sqlite(conn) = &store.backend;
    let status: String = conn
        .query_row(
            "SELECT status FROM review_queue WHERE id = ?1",
            params![review_id],
            |row| row.get(0),
        )
        .map_err(|_| Error::NotPending(review_id))?;
    if status != "pending" {
        return Err(Error::NotPending(review_id));
    }
    let new_status = ReviewStatus::from_decision(decision);
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE review_queue SET status = ?1, resolved_at = ?2 WHERE id = ?3",
        params![new_status.as_str(), now, review_id],
    )?;
    let payload = serde_json::json!({
        "review_id": review_id,
        "resolved_decision": decision.as_str(),
    });
    crate::services::events::append_event(store, "review_resolution", &payload)?;
    Ok(())
}

/// T009 [US3]: Enriched list — joins review_queue → write_decisions → memory_items + cluster_members
/// to surface summary text and source count without additional round-trips.
pub fn list_enriched(
    store: &Store,
    status_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<EnrichedReviewRow>> {
    let status = status_filter.unwrap_or("pending");
    let mut out = Vec::new();
    let Backend::Sqlite(conn) = &store.backend;
    // LEFT JOINs: a review item may have no summary (single-item low-confidence) → NULL source_count.
    // T027: also SELECT cluster_id via a second LEFT JOIN on cluster_members (cm2).
    let sql = "
        SELECT rq.id, rq.reason, rq.status, rq.created_at,
               wd.resulting_item_id,
               mi.content,
               COALESCE(cm.source_count, 0),
               cm2.cluster_id
        FROM review_queue rq
        JOIN write_decisions wd ON wd.id = rq.write_decision_id
        LEFT JOIN memory_items mi ON mi.id = wd.resulting_item_id
        LEFT JOIN (
            SELECT summary_item_id, COUNT(*) AS source_count
            FROM cluster_members
            GROUP BY summary_item_id
        ) cm ON cm.summary_item_id = wd.resulting_item_id
        LEFT JOIN cluster_members cm2 ON cm2.summary_item_id = wd.resulting_item_id
        WHERE rq.status = ?1
        GROUP BY rq.id
        ORDER BY rq.id ASC
        LIMIT ?2";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![status, limit as i64], |row| {
        Ok(EnrichedReviewRow {
            id: row.get(0)?,
            reason: row.get(1)?,
            status: row.get(2)?,
            created_at: row.get(3)?,
            summary_item_id: row.get(4)?,
            summary_text: row.get(5)?,
            source_count: row.get(6)?,
            cluster_id: row.get(7)?,
        })
    })?;
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// T010 [US3]: Approve all pending review items for a cluster — promotes summary to active,
/// SC-005 suppression in recall() hides source items once the review is resolved_store.
/// Returns the number of review items resolved.
pub fn approve_cluster(store: &Store, cluster_id: &str) -> Result<usize> {
    resolve_cluster_items(store, cluster_id, Decision::Store)
}

/// T011 [US3]: Skip all pending review items for a cluster — leaves source items active,
/// summary stays inactive (tombstoned/never promoted).
pub fn skip_cluster(store: &Store, cluster_id: &str) -> Result<usize> {
    resolve_cluster_items(store, cluster_id, Decision::Skip)
}

/// Shared implementation for approve_cluster / skip_cluster: finds all pending review_queue
/// rows whose write_decision.resulting_item_id is a cluster member's summary_item_id for
/// the given cluster_id, then resolves each one.
fn resolve_cluster_items(store: &Store, cluster_id: &str, decision: Decision) -> Result<usize> {
    // T030 [Convergence/SC-004]: use DISTINCT so a cluster with N source_items (all sharing
    // the same summary_item_id) produces exactly 1 review_id per unique review_queue row,
    // not N. Without DISTINCT the JOIN multiplies rows by cluster size, causing N-1 spurious
    // NotPending errors that abort the entire approve/skip operation.
    let Backend::Sqlite(conn) = &store.backend;
    let sql = "SELECT DISTINCT rq.id FROM review_queue rq
         JOIN write_decisions wd ON wd.id = rq.write_decision_id
         JOIN cluster_members cm ON cm.summary_item_id = wd.resulting_item_id
         WHERE cm.cluster_id = ?1 AND rq.status = 'pending'";
    let mut stmt = conn.prepare(sql)?;
    let ids: rusqlite::Result<Vec<i64>> =
        stmt.query_map(params![cluster_id], |row| row.get(0))?.collect();
    let review_ids = ids?;
    for id in &review_ids {
        resolve(store, *id, decision.clone())?;
    }
    Ok(review_ids.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Store;

    fn open_test_store() -> Store {
        Store::open(":memory:").expect("in-memory store")
    }

    /// T016 [US3]: approve_cluster resolves review items to resolved_store.
    #[test]
    fn approve_cluster_resolves_pending_items() {
        let store = open_test_store();
        let item_id = "test-summary-item";
        let cluster_id = "test-cluster-001";
        seed_cluster(&store, item_id, cluster_id, "i-001", "cm-001", "source-001");

        let approved = approve_cluster(&store, cluster_id).expect("approve_cluster failed");
        assert_eq!(approved, 1, "expected 1 item approved");

        let Backend::Sqlite(conn) = &store.backend;
        let status: String = conn.query_row(
            "SELECT status FROM review_queue WHERE write_decision_id = \
             (SELECT wd.id FROM write_decisions wd WHERE wd.resulting_item_id = ?1)",
            params![item_id],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(status, "resolved_store");
    }

    /// T016 [US3]: skip_cluster resolves review items to resolved_skip.
    #[test]
    fn skip_cluster_resolves_pending_items() {
        let store = open_test_store();
        let item_id = "test-skip-item";
        let cluster_id = "test-cluster-002";
        seed_cluster(&store, item_id, cluster_id, "i-002", "cm-002", "source-002");

        let skipped = skip_cluster(&store, cluster_id).expect("skip_cluster failed");
        assert_eq!(skipped, 1);

        let Backend::Sqlite(conn) = &store.backend;
        let status: String = conn.query_row(
            "SELECT status FROM review_queue WHERE write_decision_id = \
             (SELECT wd.id FROM write_decisions wd WHERE wd.resulting_item_id = ?1)",
            params![item_id],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(status, "resolved_skip");
    }

    /// Seed a cluster + review_queue row into the in-memory store for testing.
    fn seed_cluster(
        store: &Store,
        item_id: &str,
        cluster_id: &str,
        interaction_id: &str,
        cm_id: &str,
        source_item_id: &str,
    ) {
        let Backend::Sqlite(conn) = &store.backend;

        // 1. Insert a provenance event (required FK on memory_items and write_decisions).
        conn.execute(
            "INSERT INTO events (occurred_at, event_type, payload_json, schema_version) \
             VALUES (datetime('now'), 'write_decision', '{}', 1)",
            [],
        ).unwrap();
        let event_id: i64 = conn.last_insert_rowid();

        // 2. Insert the summary memory_item.
        conn.execute(
            "INSERT INTO memory_items (id, content, item_type, embedding_json, confidence, \
             provenance_event_id, created_at, use_count) \
             VALUES (?1, 'summary text', 'summary', NULL, 1.0, ?2, datetime('now'), 0)",
            params![item_id, event_id],
        ).unwrap();

        // 3. Insert the write_decision pointing at this item.
        conn.execute(
            "INSERT INTO write_decisions (event_id, candidate_content, decision, rule_name, \
             rule_inputs_json, resulting_item_id, created_at) \
             VALUES (?1, 'cluster summary', 'review', 'consolidation_cluster_review', '{}', ?2, datetime('now'))",
            params![event_id, item_id],
        ).unwrap();
        let wd_id: i64 = conn.last_insert_rowid();

        // 4. Insert a source item so cluster_members has a real source_item_id FK.
        conn.execute(
            "INSERT INTO events (occurred_at, event_type, payload_json, schema_version) \
             VALUES (datetime('now'), 'write_decision', '{}', 1)",
            [],
        ).unwrap();
        let src_event_id: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO memory_items (id, content, item_type, embedding_json, confidence, \
             provenance_event_id, created_at, use_count) \
             VALUES (?1, 'source text', 'fact', NULL, 1.0, ?2, datetime('now'), 0)",
            params![source_item_id, src_event_id],
        ).unwrap();

        // 5. Seed a consolidation_run (required FK from cluster_members.consolidation_run_id).
        conn.execute(
            "INSERT INTO consolidation_runs (id, started_at, status) \
             VALUES ('run-001', datetime('now'), 'completed')",
            [],
        ).unwrap();

        // 6. Insert cluster_member row.
        conn.execute(
            "INSERT INTO cluster_members (id, cluster_id, summary_item_id, source_item_id, \
             consolidation_run_id, created_at) \
             VALUES (?1, ?2, ?3, ?4, 'run-001', datetime('now'))",
            params![cm_id, cluster_id, item_id, source_item_id],
        ).unwrap();

        // 7. Insert the review_queue row.
        conn.execute(
            "INSERT INTO review_queue (write_decision_id, reason, status, created_at) \
             VALUES (?1, 'consolidation_cluster_promotion', 'pending', datetime('now'))",
            params![wd_id],
        ).unwrap();

        // suppress unused var warning for interaction_id (kept in signature for readability)
        let _ = interaction_id;
    }
}
