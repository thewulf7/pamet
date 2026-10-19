//! Integration tests for consolidation (T009). spec.md Acceptance Scenarios 1-2.
//!
//! "Near duplicate" here means similar-but-not-identical phrasing stored via
//! `explicit_remember=true`, landing in write_policy's ambiguous band
//! (0.5-0.85 similarity) which is stored verbatim rather than merged.
//! Verified empirically against the real `embed()`/`cosine_similarity()`
//! before being hardcoded here (sim ≈ 0.68, see consolidation.rs docs on why
//! true write-time duplicates — >= 0.85 — never coexist as two active rows).

use pamet::{Interaction, Store};
use rusqlite::params;

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

#[test]
fn summary_is_review_gated_until_resolved_t041() {
    // Constitution II: every cluster is formed in the ambiguous 0.5-0.85
    // band, so the new summary must be flagged for review and stay
    // invisible to recall until a human resolves it.
    let store = Store::open_in_memory().unwrap();
    remember(&store, FACT_A, true);
    remember(&store, FACT_B, true);
    pamet::consolidate(&store).unwrap();

    let pending = pamet::services::review::list(&store, Some("pending")).unwrap();
    let cluster_reviews: Vec<_> =
        pending.iter().filter(|r| r.reason == "consolidation_cluster_promotion").collect();
    assert_eq!(cluster_reviews.len(), 1, "exactly one pending review for the new summary");

    // The summary content must NOT be the top/only thing recall surfaces
    // while pending — find the summary item id directly and confirm recall
    // excludes it.
    let summary_id: String = store
        .conn
        .query_row("SELECT id FROM memory_items WHERE item_type = 'summary'", [], |r| r.get(0))
        .unwrap();
    let result = pamet::recall(&store, "quarterly report deadline", None).unwrap();
    assert!(
        !result.items.iter().any(|i| i.id == summary_id),
        "pending-review summary must not appear in recall results"
    );

    // Resolve it as accepted — it should become visible.
    pamet::resolve_review(&store, cluster_reviews[0].id, pamet::Decision::Store).unwrap();
    let result_after = pamet::recall(&store, "quarterly report deadline", None).unwrap();
    assert!(
        result_after.items.iter().any(|i| i.id == summary_id),
        "approved summary must become visible to recall"
    );
}

#[test]
fn confirmed_summary_replaces_originals_in_recall_sc005() {
    let store = Store::open_in_memory().unwrap();
    remember(&store, FACT_A, true);
    remember(&store, FACT_B, true);

    let before = pamet::recall(&store, "quarterly report deadline", None).unwrap();
    let before_count = before.items.len();
    assert_eq!(before_count, 2, "both near-duplicates surface before consolidation");

    pamet::consolidate(&store).unwrap();
    let review_id: i64 = store
        .conn
        .query_row(
            "SELECT id FROM review_queue WHERE reason = 'consolidation_cluster_promotion' AND status = 'pending'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    pamet::resolve_review(&store, review_id, pamet::Decision::Store).unwrap();

    let after = pamet::recall(&store, "quarterly report deadline", None).unwrap();
    // SC-005: at least 50% fewer surfaced items for the same query once the
    // cluster's summary is confirmed (2 originals -> 1 summary = 50% exactly).
    assert!(
        (after.items.len() as f64) <= (before_count as f64) * 0.5,
        "expected >= 50% fewer surfaced items after confirmation, before={before_count} after={}",
        after.items.len()
    );
    // The originals themselves are gone from recall (superseded), but the
    // summary — carrying the same topical content — is present.
    assert!(after.items.iter().any(|i| i.item_type == pamet::ItemType::Summary));
}

/// T048/FR-003/US1-AC2: two directly-contradicting facts must be flagged to
/// the review queue during consolidation, never auto-resolved. Items are
/// inserted directly (bypassing `remember()`'s single-best-match write-time
/// check, which would otherwise intercept this exact pair as a write-time
/// `unresolved_contradiction` review before either could become an active,
/// consolidation-visible item) — simulating two facts that entered the
/// store from different sources/times without ever being write-time-compared
/// against each other, which is exactly the gap consolidation's own
/// exhaustive pairwise scan (`flag_contradictions`) exists to catch.
/// Wording verified against the real `embed()`/`cosine_similarity()` to land
/// in `write_policy`'s contradiction-eligible band (CONTRADICTION_SIMILARITY_FLOOR
/// 0.35 <= sim < LOW_CONFIDENCE_THRESHOLD 0.5, so they are NOT also clustered
/// by `cluster_items()`, which only unions pairs >= 0.5): sim ≈ 0.42, 4 shared
/// tokens, candidate B contains contradiction markers ("actually"/"not"/"now").
#[test]
fn contradicting_facts_are_flagged_for_review_not_auto_resolved_t048() {
    use pamet::services::embedding;
    use pamet::services::events;

    const FACT_A: &str = "the quarterly review meeting happens tuesday afternoon in building two";
    const FACT_B: &str = "actually the quarterly review is not happening tuesday now";

    let store = Store::open_in_memory().unwrap();

    // Sanity: confirm the wording actually lands in the intended band before
    // relying on it (CONTRADICTION_SIMILARITY_FLOOR <= sim < LOW_CONFIDENCE_THRESHOLD).
    let sim = embedding::cosine_similarity(&embedding::embed(FACT_A), &embedding::embed(FACT_B));
    assert!(
        (0.35..0.5).contains(&sim),
        "fixture wording must land in [0.35, 0.5), got {sim}"
    );

    for (id, text) in [("item-a", FACT_A), ("item-b", FACT_B)] {
        let event_id = events::append_event(
            &store,
            "write_decision",
            &serde_json::json!({"source_id": "direct-insert", "candidate_content": text}),
        )
        .unwrap();
        let embedding = embedding::embed(text);
        store
            .conn
            .execute(
                "INSERT INTO memory_items (id, item_type, content, embedding_json, entities_json, \
                    confidence, provenance_event_id, created_at, use_count) \
                 VALUES (?1, 'fact', ?2, ?3, '[]', 0.9, ?4, '2026-01-01T00:00:00Z', 0)",
                params![id, text, serde_json::to_string(&embedding).unwrap(), event_id],
            )
            .unwrap();
        store
            .conn
            .execute("INSERT INTO memory_items_fts (id, content) VALUES (?1, ?2)", params![id, text])
            .unwrap();
        store
            .conn
            .execute(
                "INSERT INTO write_decisions (event_id, candidate_content, decision, rule_name, \
                    rule_inputs_json, resulting_item_id, created_at) \
                 VALUES (?1, ?2, 'store', 'direct_insert_fixture', '{}', ?3, '2026-01-01T00:00:00Z')",
                params![event_id, text, id],
            )
            .unwrap();
    }

    let summary = pamet::consolidate(&store).unwrap();
    assert_eq!(summary.clusters_formed, 0, "sim < 0.5 must not cluster A and B");
    assert_eq!(
        summary.contradictions_flagged, 1,
        "exactly one contradiction must be flagged"
    );

    let pending = pamet::services::review::list(&store, Some("pending")).unwrap();
    let contradiction_reviews: Vec<_> =
        pending.iter().filter(|r| r.reason == "consolidation_contradiction").collect();
    assert_eq!(
        contradiction_reviews.len(),
        1,
        "the contradiction must land in the review queue, not be auto-resolved"
    );

    // Neither item was tombstoned/deleted/auto-merged — both remain
    // inspectable (Constitution II: non-destructive, human decides).
    let active_count: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM memory_items WHERE tombstoned_at IS NULL", [], |r| r.get(0))
        .unwrap();
    assert_eq!(active_count, 2, "neither contradicting item was deleted or merged");

    let log = pamet::export_log(&store, None).unwrap();
    assert!(
        log.iter().any(|e| e.event_type == "consolidation_run_finished"
            && e.payload_json.contains("\"contradictions_flagged\":1")),
        "the flagged contradiction must be traceable via export-log"
    );
}
