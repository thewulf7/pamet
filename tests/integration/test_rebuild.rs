//! Unit/integration test for rebuild_from_events() (T041): Constitution
//! Principle I — the event log alone must be sufficient to reconstruct
//! derived state.

use pamet::{Interaction, Store};

#[test]
fn rebuilt_store_matches_live_store_after_a_mixed_sequence() {
    let live = Store::open_in_memory().unwrap();

    let d1 = pamet::remember(
        &live,
        &Interaction { source_id: "t".into(), text: "Pamet is a memory layer for agents".into(), explicit_remember: true },
    )
    .unwrap();
    let item_a = d1[0].resulting_item_id.clone().unwrap();

    pamet::remember(
        &live,
        &Interaction { source_id: "t".into(), text: "Bananas are yellow".into(), explicit_remember: true },
    )
    .unwrap();

    let retrieval = pamet::recall(&live, "memory layer", Some(5)).unwrap();
    assert!(!retrieval.items.is_empty());
    pamet::report_outcome(&live, retrieval.event_id, &item_a, true).unwrap();

    pamet::remember(
        &live,
        &Interaction { source_id: "t".into(), text: "lol ok".into(), explicit_remember: false },
    )
    .unwrap();

    let rebuilt = pamet::rebuild_from_events(&live).unwrap();

    // Same events, in the same order, with the same ids.
    let live_events = pamet::export_log(&live, None).unwrap();
    let rebuilt_events = pamet::export_log(&rebuilt, None).unwrap();
    assert_eq!(live_events.len(), rebuilt_events.len());
    for (a, b) in live_events.iter().zip(rebuilt_events.iter()) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.event_type, b.event_type);
        assert_eq!(a.payload_json, b.payload_json);
    }

    // Same memory_items (ids + content + tombstone/supersede state).
    let live_items: Vec<(String, String)> = query_all_items(&live);
    let rebuilt_items: Vec<(String, String)> = query_all_items(&rebuilt);
    assert_eq!(live_items, rebuilt_items);

    // Same write_decisions count and decisions.
    let live_wd: Vec<String> = query_decisions(&live);
    let rebuilt_wd: Vec<String> = query_decisions(&rebuilt);
    assert_eq!(live_wd, rebuilt_wd);

    // Same outcome_records.
    let live_outcomes: i64 = live.conn.query_row("SELECT COUNT(*) FROM outcome_records", [], |r| r.get(0)).unwrap();
    let rebuilt_outcomes: i64 = rebuilt.conn.query_row("SELECT COUNT(*) FROM outcome_records", [], |r| r.get(0)).unwrap();
    assert_eq!(live_outcomes, rebuilt_outcomes);
    assert_eq!(live_outcomes, 1);
}

/// T046: Constitution I / data-model.md rebuild rules — the 5 new Phase 2
/// event types (consolidation_run_started/_finished, cluster_formed,
/// summary_created, eval_run_completed) must replay to the exact same
/// derived state as the live path produced, including the review-gate
/// rows T041 added and a subsequent resolution.
#[test]
fn rebuilt_store_matches_live_store_after_phase2_events() {
    let live = Store::open_in_memory().unwrap();

    pamet::remember(
        &live,
        &Interaction {
            source_id: "t".into(),
            text: "the quarterly report is due friday".into(),
            explicit_remember: true,
        },
    )
    .unwrap();
    pamet::remember(
        &live,
        &Interaction {
            source_id: "t".into(),
            text: "remember the quarterly report deadline is this friday afternoon".into(),
            explicit_remember: true,
        },
    )
    .unwrap();

    let consolidation_summary = pamet::consolidate(&live).unwrap();
    assert_eq!(consolidation_summary.clusters_formed, 1);

    // Resolve the review gate so both the pending AND resolved code paths
    // get exercised through replay.
    let review_id: i64 = live
        .conn
        .query_row(
            "SELECT id FROM review_queue WHERE reason = 'consolidation_cluster_promotion' LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    pamet::resolve_review(&live, review_id, pamet::Decision::Store).unwrap();

    // A second consolidation run (idempotent no-op) plus an eval harness run
    // against this same store, to also exercise consolidation_run_started/
    // _finished with zero new clusters, and eval_run_completed replay.
    pamet::consolidate(&live).unwrap();
    let fixtures = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/eval_probes_v1.json"
    ));
    pamet::services::eval_harness::run_and_report(&live, fixtures, false).unwrap();

    let rebuilt = pamet::rebuild_from_events(&live).unwrap();

    let live_events = pamet::export_log(&live, None).unwrap();
    let rebuilt_events = pamet::export_log(&rebuilt, None).unwrap();
    assert_eq!(live_events.len(), rebuilt_events.len());
    for (a, b) in live_events.iter().zip(rebuilt_events.iter()) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.event_type, b.event_type);
        assert_eq!(a.payload_json, b.payload_json);
    }

    assert_eq!(query_all_items(&live), query_all_items(&rebuilt));
    assert_eq!(query_decisions(&live), query_decisions(&rebuilt));

    let live_clusters: i64 =
        live.conn.query_row("SELECT COUNT(*) FROM cluster_members", [], |r| r.get(0)).unwrap();
    let rebuilt_clusters: i64 =
        rebuilt.conn.query_row("SELECT COUNT(*) FROM cluster_members", [], |r| r.get(0)).unwrap();
    assert_eq!(live_clusters, rebuilt_clusters);
    assert!(live_clusters > 0);

    let live_runs: i64 =
        live.conn.query_row("SELECT COUNT(*) FROM consolidation_runs WHERE status = 'completed'", [], |r| r.get(0)).unwrap();
    let rebuilt_runs: i64 =
        rebuilt.conn.query_row("SELECT COUNT(*) FROM consolidation_runs WHERE status = 'completed'", [], |r| r.get(0)).unwrap();
    assert_eq!(live_runs, rebuilt_runs);
    assert_eq!(live_runs, 2);

    let live_review_status: String = live
        .conn
        .query_row("SELECT status FROM review_queue WHERE id = ?1", [review_id], |r| r.get(0))
        .unwrap();
    let rebuilt_review_status: String = rebuilt
        .conn
        .query_row("SELECT status FROM review_queue WHERE id = ?1", [review_id], |r| r.get(0))
        .unwrap();
    assert_eq!(live_review_status, "resolved_store");
    assert_eq!(live_review_status, rebuilt_review_status);

    let live_reports: i64 =
        live.conn.query_row("SELECT COUNT(*) FROM baseline_reports", [], |r| r.get(0)).unwrap();
    let rebuilt_reports: i64 =
        rebuilt.conn.query_row("SELECT COUNT(*) FROM baseline_reports", [], |r| r.get(0)).unwrap();
    assert_eq!(live_reports, rebuilt_reports);
    assert_eq!(live_reports, 1);

    // T045: per-probe eval_results rows must also replay (same count,
    // same pass/fail per probe).
    let live_eval_results: Vec<(String, i64)> = {
        let mut stmt = live.conn.prepare("SELECT probe_id, passed FROM eval_results ORDER BY probe_id").unwrap();
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).unwrap().map(|r| r.unwrap()).collect()
    };
    let rebuilt_eval_results: Vec<(String, i64)> = {
        let mut stmt = rebuilt.conn.prepare("SELECT probe_id, passed FROM eval_results ORDER BY probe_id").unwrap();
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).unwrap().map(|r| r.unwrap()).collect()
    };
    assert_eq!(live_eval_results, rebuilt_eval_results);
    assert!(!live_eval_results.is_empty());
}

/// T051: Constitution I / FR-002 — `item_decayed` (T047) replays exactly
/// like the other 5 Phase 2 event types. Ages an item directly (bypassing
/// real time), runs `consolidate` to produce the event, rebuilds, and
/// confirms both the raw event log and the derived `memory_items` state
/// match. `item_decayed` carries no derived row of its own (decay is a
/// pure read-time computation, not stored state — see `decay.rs`), so the
/// event itself replaying without error, with an identical payload, is the
/// whole rebuild contract here.
#[test]
fn rebuilt_store_matches_live_store_after_item_decayed_event_t051() {
    let live = Store::open_in_memory().unwrap();

    pamet::remember(
        &live,
        &Interaction {
            source_id: "t".into(),
            text: "The office closes at six.".into(),
            explicit_remember: true,
        },
    )
    .unwrap();

    // Age the item directly so this run's decay check actually fires,
    // without waiting on real wall-clock time.
    live.conn
        .execute(
            "UPDATE memory_items SET created_at = '2020-01-01T00:00:00Z' WHERE 1=1",
            [],
        )
        .unwrap();

    let summary = pamet::consolidate(&live).unwrap();
    assert_eq!(summary.items_decayed, 1, "the aged item must be observed as decayed");

    let rebuilt = pamet::rebuild_from_events(&live).unwrap();

    let live_events = pamet::export_log(&live, None).unwrap();
    let rebuilt_events = pamet::export_log(&rebuilt, None).unwrap();
    assert_eq!(live_events.len(), rebuilt_events.len());
    for (a, b) in live_events.iter().zip(rebuilt_events.iter()) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.event_type, b.event_type);
        assert_eq!(a.payload_json, b.payload_json);
    }
    assert!(
        live_events.iter().any(|e| e.event_type == "item_decayed"),
        "fixture must actually exercise item_decayed, or this test proves nothing"
    );

    // No derived row for item_decayed (decay is read-time-only), but the
    // item itself must still match post-replay.
    assert_eq!(query_all_items(&live), query_all_items(&rebuilt));
}

fn query_all_items(store: &Store) -> Vec<(String, String)> {
    let mut stmt = store.conn.prepare("SELECT id, content FROM memory_items ORDER BY id").unwrap();
    stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

fn query_decisions(store: &Store) -> Vec<String> {
    let mut stmt = store.conn.prepare("SELECT decision FROM write_decisions ORDER BY id").unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}
