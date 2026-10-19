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
fn truncate_target(url: &str) {
    let mut client = postgres::Client::connect(url, postgres::NoTls).expect("truncate connection");
    client
        .batch_execute(
            "TRUNCATE memory_items, events, write_decisions, review_queue, retrieval_events, \
             consolidation_runs, cluster_members, outcome_records, eval_results, baseline_reports \
             RESTART IDENTITY CASCADE",
        )
        .expect("truncate target tables");
}

#[test]
fn migrate_sqlite_source_into_postgres_target_preserves_recall() {
    let Some(url) = test_db_url() else {
        eprintln!("skipping: PAMET_TEST_POSTGRES_URL not set");
        return;
    };
    let _guard = DB_LOCK.lock().unwrap();
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
    let _guard = DB_LOCK.lock().unwrap();
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
