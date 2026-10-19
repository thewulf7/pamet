//! US2 (feature 003): `pamet migrate` / `rebuild_into` end-to-end — every
//! event from a SQLite source store reproduces equivalent live state in a
//! fresh Postgres target store. Requires a reachable Postgres+pgvector
//! instance at `PAMET_TEST_POSTGRES_URL`.

#![cfg(feature = "postgres-backend")]

use pamet::{Interaction, Store};
use std::sync::Mutex;

/// Both tests in this file hit the same shared Postgres database with no
/// per-test schema isolation, and `cargo test` runs tests in a binary
/// concurrently by default — serialize them so one test's truncate/migrate
/// can't race the other's.
static DB_LOCK: Mutex<()> = Mutex::new(());

fn test_db_url() -> Option<String> {
    std::env::var("PAMET_TEST_POSTGRES_URL").ok()
}

/// Both tests in this file share one Postgres database (there's no
/// per-test schema isolation here), and `rebuild_into` now deliberately
/// refuses a non-empty target (FR-004) — so each test must start from a
/// genuinely empty target, not rely on test-run ordering.
///
/// `Store::open` first (discarded) ensures the schema exists before
/// truncating — on a brand-new container, whichever test in this file
/// happens to run first has no tables yet, so a bare `TRUNCATE` fails with
/// "relation does not exist" rather than leaving an empty-but-present
/// schema.
fn truncate_target(url: &str) {
    let _ = Store::open(url);
    let mut client = postgres::Client::connect(url, postgres::NoTls).expect("truncate connection");
    client
        .batch_execute(
            "TRUNCATE memory_items, events, write_decisions, review_queue, retrieval_events, \
             consolidation_runs, cluster_members, outcome_records, eval_results, baseline_reports \
             RESTART IDENTITY CASCADE",
        )
        .expect("truncate target tables");
}

/// A poisoned `DB_LOCK` (from one test panicking mid-critical-section)
/// must not cascade into failing every other test in the file with an
/// unrelated `PoisonError` — recover the lock and continue; the shared
/// state it guards (the Postgres tables) is schema-safe to keep using
/// regardless of why the previous holder panicked.
fn lock_db() -> std::sync::MutexGuard<'static, ()> {
    DB_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn migrate_sqlite_source_into_postgres_target_preserves_recall() {
    let Some(url) = test_db_url() else {
        eprintln!("skipping: PAMET_TEST_POSTGRES_URL not set");
        return;
    };
    let _guard = lock_db();
    truncate_target(&url);

    let source = Store::open_in_memory().unwrap();
    pamet::remember(
        &source,
        &Interaction {
            source_id: "test".into(),
            text: "The migration preserves this fact across backends".into(),
            explicit_remember: true,
        },
    )
    .unwrap();
    pamet::recall(&source, "migration preserves fact", None).unwrap();

    let target = match Store::open(&url) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: Postgres unreachable: {e}");
            return;
        }
    };

    let source_log_len = pamet::export_log(&source, None).unwrap().len();
    let summary = pamet::services::rebuild::rebuild_into(&source, &target).unwrap();
    assert_eq!(summary.events as usize, source_log_len);
    assert_eq!(summary.items, 1);

    let result = pamet::recall(&target, "migration preserves fact", None).unwrap();
    assert!(
        result
            .items
            .iter()
            .any(|i| i.content.contains("migration preserves this fact")),
        "migrated item not found via recall on target: {:?}",
        result.items
    );
}

#[test]
fn migrate_refuses_to_run_against_a_non_empty_target() {
    // FR-004 (clarified 2026-10-19): a safe re-run means a fresh target, not
    // merging/resuming into one that already has data.
    let Some(url) = test_db_url() else {
        eprintln!("skipping: PAMET_TEST_POSTGRES_URL not set");
        return;
    };
    let _guard = lock_db();
    truncate_target(&url);

    let source = Store::open_in_memory().unwrap();
    pamet::remember(
        &source,
        &Interaction {
            source_id: "test".into(),
            text: "first migration attempt".into(),
            explicit_remember: true,
        },
    )
    .unwrap();

    let target = match Store::open(&url) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: Postgres unreachable: {e}");
            return;
        }
    };

    // First run against a fresh target succeeds...
    pamet::services::rebuild::rebuild_into(&source, &target).unwrap();
    // ...a second run against that now-non-empty target MUST be refused,
    // not silently re-applied or merged.
    let second = pamet::services::rebuild::rebuild_into(&source, &target);
    assert!(
        second.is_err(),
        "expected migrate to refuse a non-empty target, got Ok"
    );
}

fn pg_count(url: &str, sql: &str) -> i64 {
    let mut client = postgres::Client::connect(url, postgres::NoTls).expect("pg_count connection");
    client.query_one(sql, &[]).unwrap().get(0)
}

/// T031/T036: US2/AC1 ("a pending review, a resolved review, and at least
/// one consolidation run... same counts of each, with no duplicates") and
/// FR-009's eval-history ("baseline-report history with no loss") were
/// only ever manually verified once via quickstart.md — this gives them a
/// real regression test. Two separate near-duplicate clusters give one
/// pending and one resolved review; running `eval` gives a baseline report.
#[test]
fn migrate_carries_over_reviews_consolidation_runs_and_eval_history() {
    let Some(url) = test_db_url() else {
        eprintln!("skipping: PAMET_TEST_POSTGRES_URL not set");
        return;
    };
    let _guard = lock_db();
    truncate_target(&url);

    let source = Store::open_in_memory().unwrap();
    let remember = |text: &str| {
        pamet::remember(
            &source,
            &Interaction {
                source_id: "test".into(),
                text: text.into(),
                explicit_remember: true,
            },
        )
        .unwrap();
    };

    // Cluster 1 — resolved.
    remember("the quarterly report is due friday");
    remember("remember the quarterly report deadline is this friday afternoon");
    let summary1 = pamet::consolidate(&source).unwrap();
    assert_eq!(summary1.clusters_formed, 1, "cluster 1 must form");
    let review1_id: i64 = source
        .conn()
        .query_row(
            "SELECT id FROM review_queue WHERE reason = 'consolidation_cluster_promotion' \
             ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    pamet::resolve_review(&source, review1_id, pamet::Decision::Store).unwrap();

    // Cluster 2 — left pending (verified in-band empirically before being
    // hardcoded here, same approach as test_consolidation.rs's FACT_A/B).
    remember("the office wifi password changed last week");
    remember("the wifi password at the office got changed recently");
    let summary2 = pamet::consolidate(&source).unwrap();
    assert_eq!(summary2.clusters_formed, 1, "cluster 2 must form");

    // T036: run eval against the source before migrating, so baseline_reports
    // has at least one row to carry over (FR-009).
    let fixtures = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/eval_probes_v1.json"
    ));
    pamet::services::eval_harness::run_and_report(&source, fixtures, false).unwrap();

    let source_items: i64 = source
        .conn()
        .query_row("SELECT COUNT(*) FROM memory_items", [], |r| r.get(0))
        .unwrap();
    let source_pending = pamet::services::review::list(&source, Some("pending"))
        .unwrap()
        .len();
    let source_resolved: i64 = source
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM review_queue WHERE status LIKE 'resolved%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let source_runs: i64 = source
        .conn()
        .query_row("SELECT COUNT(*) FROM consolidation_runs", [], |r| r.get(0))
        .unwrap();
    let source_reports: i64 = source
        .conn()
        .query_row("SELECT COUNT(*) FROM baseline_reports", [], |r| r.get(0))
        .unwrap();
    assert_eq!(source_pending, 1, "exactly one pending review in source");
    assert_eq!(source_resolved, 1, "exactly one resolved review in source");
    assert_eq!(source_runs, 2, "two consolidation runs in source");
    assert_eq!(source_reports, 1, "one baseline report in source");

    let target = match Store::open(&url) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: Postgres unreachable: {e}");
            return;
        }
    };
    pamet::services::rebuild::rebuild_into(&source, &target).unwrap();

    assert_eq!(
        pg_count(&url, "SELECT COUNT(*) FROM memory_items"),
        source_items,
        "item count must match exactly, no duplicates"
    );
    assert_eq!(
        pg_count(
            &url,
            "SELECT COUNT(*) FROM review_queue WHERE status = 'pending'"
        ),
        1,
        "pending review must carry over"
    );
    assert_eq!(
        pg_count(
            &url,
            "SELECT COUNT(*) FROM review_queue WHERE status LIKE 'resolved%'"
        ),
        1,
        "resolved review must carry over"
    );
    assert_eq!(
        pg_count(&url, "SELECT COUNT(*) FROM consolidation_runs"),
        2,
        "both consolidation runs must carry over"
    );
    assert_eq!(
        pg_count(&url, "SELECT COUNT(*) FROM baseline_reports"),
        1,
        "baseline report (eval history) must carry over, per FR-009"
    );
}

/// T032: US2/AC3 and the "interrupted consolidation run" Edge Case — an
/// in-progress run that was never finished (process crash) MUST carry over
/// to the target still marked `interrupted`, not silently completed or
/// dropped. Simulates a crash the same way a real one leaves the event log:
/// a `consolidation_run_started` event with no matching `_finished` event,
/// then a later process start's `interrupt_stale_runs()` (invoked at the
/// top of every `consolidate()` call) marks it interrupted for real.
#[test]
fn migrate_carries_over_interrupted_consolidation_run_status() {
    let Some(url) = test_db_url() else {
        eprintln!("skipping: PAMET_TEST_POSTGRES_URL not set");
        return;
    };
    let _guard = lock_db();
    truncate_target(&url);

    let source = Store::open_in_memory().unwrap();
    pamet::remember(
        &source,
        &Interaction {
            source_id: "test".into(),
            text: "fact for the interrupted-run test".into(),
            explicit_remember: true,
        },
    )
    .unwrap();

    let stale_run_id = "stale-run-for-migration-test";
    pamet::services::events::append_event(
        &source,
        "consolidation_run_started",
        &serde_json::json!({"run_id": stale_run_id, "started_at": "2020-01-01T00:00:00Z"}),
    )
    .unwrap();
    source
        .conn()
        .execute(
            "INSERT INTO consolidation_runs (id, started_at, status) VALUES (?1, '2020-01-01T00:00:00Z', 'running')",
            rusqlite::params![stale_run_id],
        )
        .unwrap();
    // Next consolidate() call interrupts the stale run before doing its own work.
    pamet::consolidate(&source).unwrap();

    let source_status: String = source
        .conn()
        .query_row(
            "SELECT status FROM consolidation_runs WHERE id = ?1",
            rusqlite::params![stale_run_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(source_status, "interrupted");

    let target = match Store::open(&url) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: Postgres unreachable: {e}");
            return;
        }
    };
    pamet::services::rebuild::rebuild_into(&source, &target).unwrap();

    let mut client = postgres::Client::connect(&url, postgres::NoTls).unwrap();
    let target_status: String = client
        .query_one(
            "SELECT status FROM consolidation_runs WHERE id = $1",
            &[&stale_run_id],
        )
        .unwrap()
        .get(0);
    assert_eq!(
        target_status, "interrupted",
        "interrupted status must survive migration, not be silently completed or dropped"
    );
}

/// T033: SC-002's literal scale target — migrating up to 10,000 items
/// completes in under 5 minutes. Items are bulk-seeded directly into the
/// source SQLite file (not via 10,000 individual `remember()` calls, for
/// the same reason `test_postgres_scale.rs` bulk-seeds its target: the live
/// write path's O(n) near-duplicate scan per write would make this a test
/// of write-policy throughput, not migration throughput).
#[test]
fn migrate_10000_items_completes_under_5_minutes_sc002() {
    let Some(url) = test_db_url() else {
        eprintln!("skipping: PAMET_TEST_POSTGRES_URL not set");
        return;
    };
    let _guard = lock_db();
    truncate_target(&url);

    let source = Store::open_in_memory().unwrap();
    let n = 10_000;
    for i in 0..n {
        let content = format!("sc002 scale fact number {i} about topic {}", i % 101);
        let embedding = pamet::services::embedding::embed(&content);
        let id = uuid::Uuid::new_v4().to_string();
        let event_id = pamet::services::events::append_event(
            &source,
            "write_decision",
            &serde_json::json!({
                "candidate_content": content, "decision": "store", "rule_name": "sc002_seed",
                "rule_inputs": {}, "resulting_item_id": id,
            }),
        )
        .unwrap();
        source
            .conn()
            .execute(
                "INSERT INTO memory_items (id, item_type, content, embedding_json, entities_json, \
                    confidence, provenance_event_id, created_at, use_count) \
                 VALUES (?1, 'fact', ?2, ?3, '[]', 0.9, ?4, datetime('now'), 0)",
                rusqlite::params![
                    id,
                    content,
                    serde_json::to_string(&embedding).unwrap(),
                    event_id
                ],
            )
            .unwrap();
    }

    let target = match Store::open(&url) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: Postgres unreachable: {e}");
            return;
        }
    };

    let start = std::time::Instant::now();
    let summary = pamet::services::rebuild::rebuild_into(&source, &target).unwrap();
    let elapsed = start.elapsed();
    println!("migrated {} items in {elapsed:?}", summary.items);

    assert_eq!(summary.items, n as i64);
    assert!(
        elapsed.as_secs() < 300,
        "SC-002 violated: migrating {n} items took {elapsed:?} (>= 5 minutes)"
    );
}

/// T034: Edge Case — "the configured production backend is unreachable at
/// startup MUST fail fast with a clear, actionable error — MUST NOT
/// silently fall back to the embedded store". This is distinct from T024's
/// "feature not compiled in" case: here the feature IS compiled in, but no
/// server is listening on the given host/port at all.
#[test]
fn store_open_against_unreachable_postgres_server_fails_fast() {
    let start = std::time::Instant::now();
    let result = Store::open("postgres://postgres:pamet@127.0.0.1:1/pamet_test_unreachable");
    let elapsed = start.elapsed();

    assert!(
        result.is_err(),
        "an unreachable Postgres server must fail Store::open, not silently succeed"
    );
    assert!(
        elapsed.as_secs() < 10,
        "must fail fast, not hang: took {elapsed:?}"
    );
}
