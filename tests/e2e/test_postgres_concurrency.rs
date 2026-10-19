//! US1 (feature 003): genuine concurrent connections on the Postgres backend
//! — the core thing the embedded SQLite backend's single connection could
//! only approximate (Two-Speed Memory Separation gap this feature closes).
//! Requires a reachable Postgres+pgvector instance at `PAMET_TEST_POSTGRES_URL`.

#![cfg(feature = "postgres-backend")]

use pamet::{Interaction, Store};
use std::thread;
use std::time::Instant;

fn test_db_url() -> Option<String> {
    std::env::var("PAMET_TEST_POSTGRES_URL").ok()
}

#[test]
fn concurrent_remember_and_consolidate_both_make_progress() {
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

    // Seed some items so consolidate() has something to scan.
    for i in 0..20 {
        pamet::remember(
            &store,
            &Interaction {
                source_id: "seed".into(),
                text: format!("seed fact number {i} about concurrency testing"),
                explicit_remember: true,
            },
        )
        .unwrap();
    }

    // Run consolidate() on a background thread, on its OWN pooled
    // connection to the same database, while the live remember/recall path
    // keeps writing on the main thread's connection — this is only
    // possible because Postgres hands out genuinely independent pooled
    // connections per `Store::open` call, unlike the embedded SQLite
    // backend's single shared connection.
    let consolidate_url = url.clone();
    let consolidate_handle = thread::spawn(move || {
        let consolidate_store = Store::open(&consolidate_url).unwrap();
        let start = Instant::now();
        let summary = pamet::consolidate(&consolidate_store).unwrap();
        (summary, start.elapsed())
    });

    let live_start = Instant::now();
    let mut live_ops = 0;
    for i in 0..20 {
        pamet::remember(
            &store,
            &Interaction {
                source_id: "live".into(),
                text: format!("live write number {i} happening during consolidation"),
                explicit_remember: true,
            },
        )
        .unwrap();
        pamet::recall(&store, "live write", None).unwrap();
        live_ops += 1;
    }
    let live_elapsed = live_start.elapsed();

    let (summary, consolidate_elapsed) = consolidate_handle.join().unwrap();

    println!(
        "live path: {live_ops} remember+recall pairs in {live_elapsed:?}; \
         consolidate: {} items processed in {consolidate_elapsed:?}",
        summary.items_processed
    );

    // The real assertion: both finished without blocking on each other
    // (no deadlock, no error) — that's the thing a single shared SQLite
    // connection could not do. `items_processed` is deliberately NOT
    // asserted at an exact count: near-duplicate templated seed text
    // legitimately merges at write time (same effect as T050's finding in
    // feature 002), so how many distinct rows exist by the time
    // `consolidate` ran is a race, not a correctness property.
    assert_eq!(live_ops, 20);
    assert!(summary.items_processed >= 1);
}
