//! Integration tests for consolidation (T009). spec.md Acceptance Scenarios 1-2.
//!
//! "Near duplicate" here means similar-but-not-identical phrasing stored via
//! `explicit_remember=true`, landing in write_policy's ambiguous band
//! (0.5-0.85 similarity) which is stored verbatim rather than merged.
//! Verified empirically against the real `embed()`/`cosine_similarity()`
//! before being hardcoded here (sim ≈ 0.68, see consolidation.rs docs on why
//! true write-time duplicates — >= 0.85 — never coexist as two active rows).

use pamet::{Interaction, Store};

fn remember(store: &Store, text: &str, explicit: bool) {
    pamet::remember(store, &Interaction {
        source_id: "test".into(), text: text.into(), explicit_remember: explicit,
    }).unwrap();
}

const FACT_A: &str = "the quarterly report is due friday";
const FACT_B: &str = "remember the quarterly report deadline is this friday afternoon";

#[test]
fn clusters_near_duplicates_non_destructively() {
    let store = Store::open_in_memory().unwrap();
    remember(&store, FACT_A, true);
    remember(&store, FACT_B, true);

    // Sanity: write-time policy kept them as two separate active items
    // (neither merged nor tombstoned) — otherwise this test isn't exercising
    // consolidation's clustering at all.
    let active_count: i64 = store.conn.query_row(
        "SELECT COUNT(*) FROM memory_items WHERE tombstoned_at IS NULL", [], |r| r.get(0),
    ).unwrap();
    assert_eq!(active_count, 2, "fixture wording must land both facts as separate active items");

    let summary = pamet::consolidate(&store).unwrap();
    assert_eq!(summary.clusters_formed, 1, "2 near-duplicate facts should form exactly one cluster");

    // Originals are not tombstoned (non-destructive).
    let tombstoned: i64 = store.conn.query_row(
        "SELECT COUNT(*) FROM memory_items WHERE tombstoned_at IS NOT NULL", [], |r| r.get(0),
    ).unwrap();
    assert_eq!(tombstoned, 0);

    // The event log has the consolidation events.
    let log = pamet::export_log(&store, None).unwrap();
    assert!(log.iter().any(|e| e.event_type == "consolidation_run_started"));
    assert!(log.iter().any(|e| e.event_type == "cluster_formed"));
    assert!(log.iter().any(|e| e.event_type == "summary_created"));
    assert!(log.iter().any(|e| e.event_type == "consolidation_run_finished"));

    // recall still finds something for the topic (summary or original).
    let result = pamet::recall(&store, "quarterly report deadline", None).unwrap();
    assert!(!result.items.is_empty());
}

#[test]
fn idempotent_rerun_forms_no_new_clusters() {
    let store = Store::open_in_memory().unwrap();
    remember(&store, FACT_A, true);
    remember(&store, FACT_B, true);

    let first = pamet::consolidate(&store).unwrap();
    assert_eq!(first.clusters_formed, 1);

    let second = pamet::consolidate(&store).unwrap();
    assert_eq!(second.clusters_formed, 0, "re-running over an unchanged store must be a no-op");
}

#[test]
fn empty_store_consolidation_is_a_no_op() {
    let store = Store::open_in_memory().unwrap();
    let summary = pamet::consolidate(&store).unwrap();
    assert_eq!(summary.items_processed, 0);
    assert_eq!(summary.clusters_formed, 0);
}

#[test]
fn interrupted_run_is_marked_and_new_run_proceeds() {
    let store = Store::open_in_memory().unwrap();
    remember(&store, FACT_A, true);

    // Simulate a crashed run: insert a 'running' row directly (no finished event).
    store.conn.execute(
        "INSERT INTO consolidation_runs (id, started_at, status) VALUES ('stale-run', '2020-01-01T00:00:00Z', 'running')",
        [],
    ).unwrap();

    let summary = pamet::consolidate(&store).unwrap();
    assert!(summary.items_processed >= 0); // new run proceeded without erroring

    let stale_status: String = store.conn.query_row(
        "SELECT status FROM consolidation_runs WHERE id = 'stale-run'", [], |r| r.get(0),
    ).unwrap();
    assert_eq!(stale_status, "interrupted");

    let log = pamet::export_log(&store, None).unwrap();
    assert!(log.iter().any(|e| e.event_type == "consolidation_run_finished"
        && e.payload_json.contains("\"status\":\"interrupted\"")
        && e.payload_json.contains("stale-run")));
}
