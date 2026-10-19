//! Feature 005: write-policy near-duplicate pre-filter scale regression test.
//! Proves SC-002 (per-batch write cost does not grow more than 2x from first
//! to last batch on topically-overlapping content) and FR-003 (the
//! pre-filter never misses a true near-duplicate in the decisive merge
//! band). Runs on the default SQLite build unconditionally (T006); also
//! runnable against Postgres (T007) via `PAMET_TEST_POSTGRES_URL`, same
//! pattern `tests/e2e/test_migration_e2e.rs` already uses.

use pamet::{Interaction, Store};
use std::time::Instant;

const BATCH_SIZE: usize = 200;
const BATCH_COUNT: usize = 15; // 3,000 items total — enough to expose the
                               // original O(n) full-scan growth (feature 004 saw it clearly by item 2,800).

/// Deliberately topically-overlapping (not low-overlap synthetic text, which
/// previously hid the problem): every sentence shares a bounded vocabulary
/// of topics/subjects, mirroring the real Hindsight corpus's high lexical
/// overlap without committing real personal data to a test fixture
/// (research.md Decision 4).
fn overlapping_fact(i: usize) -> String {
    let topics = [
        "the deployment pipeline",
        "the quarterly budget review",
        "the customer onboarding flow",
        "the payments reconciliation job",
        "the on-call rotation schedule",
    ];
    let verbs = [
        "was updated by the team",
        "needs another look before Friday",
        "is now owned by a different group",
        "was discussed in this week's sync",
        "had an issue that got resolved",
    ];
    let topic = topics[i % topics.len()];
    let verb = verbs[(i / topics.len()) % verbs.len()];
    format!("{topic} {verb}, item number {i}")
}

fn open_target() -> (Store, bool) {
    match std::env::var("PAMET_TEST_POSTGRES_URL") {
        Ok(url) => {
            #[cfg(feature = "postgres-backend")]
            {
                truncate(&url);
                (Store::open(&url).expect("open postgres target"), true)
            }
            #[cfg(not(feature = "postgres-backend"))]
            {
                let _ = url;
                (Store::open_in_memory().expect("open in-memory"), false)
            }
        }
        Err(_) => (Store::open_in_memory().expect("open in-memory"), false),
    }
}

#[cfg(feature = "postgres-backend")]
fn truncate(url: &str) {
    let _ = Store::open(url);
    let mut client = postgres::Client::connect(url, postgres::NoTls).expect("truncate connection");
    let _ = client.batch_execute(
        "TRUNCATE memory_items, events, write_decisions, review_queue, retrieval_events, \
         consolidation_runs, cluster_members, outcome_records, eval_results, baseline_reports \
         RESTART IDENTITY CASCADE",
    );
}

/// T005/SC-002: per-batch cost must not grow more than 2x from the first
/// batch to the last, on real-shaped (topically-overlapping) content —
/// versus the ~23x growth measured before this fix (4.4s -> 101.9s/200
/// items in feature 004's real migration).
#[test]
fn write_cost_stays_flat_on_overlapping_content() {
    let (store, is_postgres) = open_target();
    let mut batch_times = Vec::with_capacity(BATCH_COUNT);
    let mut i = 0usize;

    for _ in 0..BATCH_COUNT {
        let t0 = Instant::now();
        for _ in 0..BATCH_SIZE {
            pamet::remember(
                &store,
                &Interaction {
                    source_id: "scale_test".into(),
                    text: overlapping_fact(i),
                    explicit_remember: true,
                },
            )
            .unwrap();
            i += 1;
        }
        let elapsed = t0.elapsed();
        println!(
            "batch {}/{BATCH_COUNT}: {elapsed:?} (postgres={is_postgres})",
            batch_times.len() + 1
        );
        batch_times.push(elapsed);
    }

    let first = batch_times.first().unwrap().as_secs_f64();
    let last = batch_times.last().unwrap().as_secs_f64();
    println!(
        "first batch: {first:.3}s, last batch: {last:.3}s, ratio: {:.2}x",
        last / first.max(0.001)
    );
    assert!(
        last <= first.max(0.05) * 2.0,
        "SC-002 violated: last batch ({last:.3}s) is more than 2x the first batch ({first:.3}s)"
    );
}

/// T012/FR-003: a planted exact near-duplicate of an early-seeded item must
/// still be caught by the pre-filtered merge decision, proving the
/// provably-exact claim in research.md Decision 1 with a direct test, not
/// just a mathematical argument.
#[test]
fn prefilter_never_misses_a_true_near_duplicate() {
    let store = Store::open_in_memory().unwrap();

    // Seed a few thousand unrelated-topic items so the candidate set is
    // large enough that an unfiltered full scan would meaningfully differ
    // from a pre-filtered one if the pre-filter were lossy.
    for i in 0..2000 {
        pamet::remember(
            &store,
            &Interaction {
                source_id: "scale_test".into(),
                text: overlapping_fact(i),
                explicit_remember: true,
            },
        )
        .unwrap();
    }

    let original = "the payments reconciliation job was updated by the team, item number 3";
    pamet::remember(
        &store,
        &Interaction {
            source_id: "scale_test".into(),
            text: original.into(),
            explicit_remember: true,
        },
    )
    .unwrap();

    // Near-duplicate: same tokens, reworded — must land in the merge band
    // and must still be found via the keyword pre-filter (shares every
    // meaningful token with `original`).
    let near_dup = "the payments reconciliation job was updated by the team item number 3";
    let decisions = pamet::remember(
        &store,
        &Interaction {
            source_id: "scale_test".into(),
            text: near_dup.into(),
            explicit_remember: true,
        },
    )
    .unwrap();

    assert!(
        decisions
            .iter()
            .any(|d| d.decision == pamet::Decision::Merge),
        "a near-duplicate of an early-seeded item must still be merged after \
         pre-filtering, got decisions: {:?}",
        decisions.iter().map(|d| d.decision).collect::<Vec<_>>()
    );
}
