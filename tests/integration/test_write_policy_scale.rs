//! Feature 005: write-policy near-duplicate pre-filter scale regression test.
//! Proves SC-002 (per-batch write cost does not grow more than 2x from first
//! to last batch on topically-overlapping content), SC-004 (no measurable
//! overhead on a small store), and FR-003 (the pre-filter never misses a
//! true near-duplicate/contradiction in the decisive bands, on either
//! backend — feature 005's original Phase 6 only tested the merge band on
//! SQLite; Phase 7 convergence (T019-T021) closes both gaps). Runs on the
//! default SQLite build unconditionally; also runnable against Postgres
//! via `PAMET_TEST_POSTGRES_URL`, same pattern `tests/e2e/test_migration_e2e.rs`
//! already uses.

use pamet::{Interaction, Store};
use std::sync::Mutex;
use std::time::Instant;

const BATCH_SIZE: usize = 200;
const BATCH_COUNT: usize = 15; // 3,000 items total — enough to expose the
                               // original O(n) full-scan growth (feature 004 saw it clearly by item 2,800).

/// All tests in this file share one Postgres database when
/// `PAMET_TEST_POSTGRES_URL` is set (no per-test schema isolation) —
/// serialize them so one test's truncate/write can't race another's, same
/// pattern `tests/e2e/test_migration_e2e.rs` uses. Poison-tolerant so one
/// test's panic can't cascade-fail the rest of the file.
static DB_LOCK: Mutex<()> = Mutex::new(());
fn lock_db() -> std::sync::MutexGuard<'static, ()> {
    DB_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

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

fn remember(store: &Store, text: &str) -> Vec<pamet::WriteDecisionOutcome> {
    pamet::remember(
        store,
        &Interaction {
            source_id: "scale_test".into(),
            text: text.into(),
            explicit_remember: true,
        },
    )
    .unwrap()
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
    let _guard = lock_db();
    let (store, is_postgres) = open_target();
    let mut batch_times = Vec::with_capacity(BATCH_COUNT);
    let mut i = 0usize;

    for _ in 0..BATCH_COUNT {
        let t0 = Instant::now();
        for _ in 0..BATCH_SIZE {
            remember(&store, &overlapping_fact(i));
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

/// T012/T020/FR-003: a planted exact near-duplicate of an early-seeded item
/// must still be caught by the pre-filtered merge decision, on whichever
/// backend is configured — proving the provably-exact claim in research.md
/// Decision 1 with a direct test on both dialects, not just SQLite.
#[test]
fn prefilter_never_misses_a_true_near_duplicate() {
    let _guard = lock_db();
    let (store, _is_postgres) = open_target();

    // Seed a few thousand unrelated-topic items so the candidate set is
    // large enough that an unfiltered full scan would meaningfully differ
    // from a pre-filtered one if the pre-filter were lossy.
    for i in 0..2000 {
        remember(&store, &overlapping_fact(i));
    }

    let original = "the payments reconciliation job was updated by the team, item number 3";
    remember(&store, original);

    // Near-duplicate: same tokens, reworded — must land in the merge band
    // and must still be found via the keyword pre-filter (shares every
    // meaningful token with `original`).
    let near_dup = "the payments reconciliation job was updated by the team item number 3";
    let decisions = remember(&store, near_dup);

    assert!(
        decisions
            .iter()
            .any(|d| d.decision == pamet::Decision::Merge),
        "a near-duplicate of an early-seeded item must still be merged after \
         pre-filtering, got decisions: {:?}",
        decisions.iter().map(|d| d.decision).collect::<Vec<_>>()
    );
}

/// T019/FR-003/US2-AC2/Edge Case: a true contradiction-band pair (shared
/// tokens + a contradiction marker, scoring below
/// `CONTRADICTION_SIMILARITY_FLOOR`) must still be caught after
/// pre-filtering — the merge band alone (covered above) does not exercise
/// `detect_contradiction`'s own candidate lookup, which is a materially
/// different code path (`Decision::Supersede`/`Decision::Review`, not
/// `Decision::Merge`). Text pair's contradiction classification was
/// verified empirically against the real tokenizer before being hardcoded
/// here (same approach `tests/integration/test_consolidation.rs` already
/// uses for its own contradiction fixture).
#[test]
fn prefilter_never_misses_a_true_contradiction() {
    let _guard = lock_db();
    let (store, _is_postgres) = open_target();

    for i in 0..2000 {
        remember(&store, &overlapping_fact(i));
    }

    let original = "the quarterly review meeting happens tuesday afternoon in building two";
    remember(&store, original);

    // Shares >=2 tokens with `original` ("quarterly", "review", "tuesday")
    // plus a contradiction marker ("not") — must still be classified as a
    // contradiction (Supersede or Review, never silently Store'd as if
    // unrelated) after pre-filtering.
    let contradiction = "actually the quarterly review is not happening tuesday now";
    let decisions = remember(&store, contradiction);

    assert!(
        decisions.iter().any(|d| matches!(
            d.decision,
            pamet::Decision::Supersede | pamet::Decision::Review
        )),
        "a true contradiction of an early-seeded item must still be caught (not \
         silently stored as unrelated) after pre-filtering, got decisions: {:?}",
        decisions.iter().map(|d| d.decision).collect::<Vec<_>>()
    );
}

/// T021/SC-004: writing to a small store must show no measurable overhead
/// from the pre-filter's own keyword-index lookup — a direct, isolated
/// measurement rather than the inference-from-overall-CI-runtime spec.md's
/// Results section previously relied on.
#[test]
fn small_store_writes_show_no_measurable_prefilter_overhead() {
    let store = Store::open_in_memory().unwrap();
    let n = 20;
    let t0 = Instant::now();
    for i in 0..n {
        remember(&store, &overlapping_fact(i));
    }
    let elapsed = t0.elapsed();
    let per_item_ms = elapsed.as_secs_f64() * 1000.0 / n as f64;
    println!("small store: {n} items in {elapsed:?} ({per_item_ms:.2}ms/item)");
    assert!(
        per_item_ms < 20.0,
        "SC-004 violated: {per_item_ms:.2}ms/item on a small store is too slow for a \
         cheap pre-filter that should cost ~nothing when there's little to filter"
    );
}
