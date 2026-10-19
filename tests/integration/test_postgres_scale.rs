//! Polish (feature 003): scale sanity on the Postgres backend — a store
//! with several hundred items stays responsive for both recall and
//! consolidate, now that writes/reads/consolidation all hit a real indexed
//! backend instead of SQLite's JSON-blob brute-force scan. Requires a
//! reachable Postgres+pgvector instance at `PAMET_TEST_POSTGRES_URL`.

#![cfg(feature = "postgres-backend")]

use pamet::{Interaction, Store};
use std::time::Instant;

fn test_db_url() -> Option<String> {
    std::env::var("PAMET_TEST_POSTGRES_URL").ok()
}

#[test]
fn recall_and_consolidate_stay_responsive_against_a_few_hundred_items() {
    let Some(url) = test_db_url() else {
        eprintln!("skipping: PAMET_TEST_POSTGRES_URL not set");
        return;
    };
    let store = match Store::open(&url) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: Postgres unreachable: {e}");
            return;
        }
    };

    let n = 300;
    let write_start = Instant::now();
    for i in 0..n {
        pamet::remember(
            &store,
            &Interaction {
                source_id: "scale".into(),
                text: format!("distinct scale-test fact number {i} about topic {}", i % 37),
                explicit_remember: true,
            },
        )
        .unwrap();
    }
    let write_elapsed = write_start.elapsed();

    let recall_start = Instant::now();
    let result = pamet::recall(&store, "distinct scale-test fact", Some(10)).unwrap();
    let recall_elapsed = recall_start.elapsed();
    assert!(!result.items.is_empty());

    let consolidate_start = Instant::now();
    let summary = pamet::consolidate(&store).unwrap();
    let consolidate_elapsed = consolidate_start.elapsed();

    println!(
        "{n} writes in {write_elapsed:?}; recall in {recall_elapsed:?}; \
         consolidate ({} items) in {consolidate_elapsed:?}",
        summary.items_processed
    );

    // Not a strict perf SLA — a smoke bound that catches a pathological
    // regression (e.g. an accidental full-table-per-row scan) without being
    // flaky on a loaded CI box.
    assert!(
        recall_elapsed.as_secs() < 10,
        "recall took too long: {recall_elapsed:?}"
    );
    assert!(
        consolidate_elapsed.as_secs() < 60,
        "consolidate took too long: {consolidate_elapsed:?}"
    );
}
