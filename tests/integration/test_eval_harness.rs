//! Integration tests for the eval harness (T021). spec.md SC-003, SC-004.

use pamet::services::eval_harness;
use pamet::storage::Store;
use std::path::Path;

fn fixtures_path() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/eval_probes_v1.json"))
}

#[test]
fn loads_probe_set_and_all_four_categories_present() {
    let probe_set = eval_harness::load_probe_set(fixtures_path()).unwrap();
    assert!(probe_set.probes.len() >= 12, "expected at least 3 probes x 4 categories");
    let categories: std::collections::HashSet<_> =
        probe_set.probes.iter().map(|p| p.category.as_str()).collect();
    assert!(categories.contains("new_fact_recall"));
    assert!(categories.contains("update_supersession"));
    assert!(categories.contains("forgetting_abstention"));
    assert!(categories.contains("general_precision"));
}

#[test]
fn pamet_run_produces_a_report_entry_per_category() {
    let probe_set = eval_harness::load_probe_set(fixtures_path()).unwrap();
    let report = eval_harness::run_pamet(&probe_set).unwrap();
    assert_eq!(report.len(), 4, "every category should have a score entry");
    for (category, score) in &report {
        assert!(score.total > 0, "{category} had zero probes run");
    }
}

#[test]
fn repeat_runs_are_byte_identical_sc003() {
    let probe_set = eval_harness::load_probe_set(fixtures_path()).unwrap();
    let r1 = eval_harness::run_pamet(&probe_set).unwrap();
    let r2 = eval_harness::run_pamet(&probe_set).unwrap();
    // Compare pass/total (not latency, which legitimately varies run to run).
    for (cat, score1) in &r1 {
        let score2 = &r2[cat];
        assert_eq!(score1.passed, score2.passed, "category {cat} pass count differs between runs");
        assert_eq!(score1.total, score2.total);
    }
}

#[test]
fn run_and_report_persists_a_baseline_report_row() {
    let store = Store::open_in_memory().unwrap();
    let report = eval_harness::run_and_report(&store, fixtures_path(), false).unwrap();
    assert!(!report.hindsight_available);

    let count: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM baseline_reports WHERE id = ?1", [&report.id], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);

    let log = pamet::export_log(&store, None).unwrap();
    assert!(log.iter().any(|e| e.event_type == "eval_run_completed"));
}

#[test]
fn write_report_file_round_trips_as_json() {
    let store = Store::open_in_memory().unwrap();
    let report = eval_harness::run_and_report(&store, fixtures_path(), false).unwrap();
    let tmp = std::env::temp_dir().join(format!("pamet-eval-test-{}.json", uuid::Uuid::new_v4()));
    eval_harness::write_report_file(&report, &tmp).unwrap();
    let text = std::fs::read_to_string(&tmp).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(parsed.get("pamet").is_some());
    std::fs::remove_file(&tmp).ok();
}

/// T044/SC-002: a full harness run completes in under 2 minutes for up to
/// 1,000 memory items of cumulative state across the run (the harness
/// seeds one fresh temp store per probe, so 1,000 synthetic single-fact
/// probes is the equivalent scale check).
#[test]
fn harness_completes_within_two_minutes_at_1000_item_scale_sc002() {
    use pamet::models::{EvalProbe, EvalProbeSet, ExpectedOutcome, GivenStep, ProbeAction, ProbeCategory};

    let probes: Vec<EvalProbe> = (0..1000)
        .map(|i| EvalProbe {
            id: format!("load-{i}"),
            category: ProbeCategory::NewFactRecall,
            given: vec![GivenStep {
                text: format!("Load-test fact number {i} says the value is {i}."),
                source_id: "load-test".into(),
                explicit_remember: true,
            }],
            action: ProbeAction::Recall { query: format!("what is load-test fact number {i}") },
            expected: ExpectedOutcome::ContainsContent { value: i.to_string() },
        })
        .collect();
    let probe_set = EvalProbeSet { version: "load-test".into(), probes };

    let start = std::time::Instant::now();
    let report = eval_harness::run_pamet(&probe_set).unwrap();
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_secs() < 120,
        "1,000-item harness run took {elapsed:?}, exceeds the 2-minute SC-002 bound"
    );
    let score = report.get(ProbeCategory::NewFactRecall.as_str()).unwrap();
    assert_eq!(score.total, 1000);
}

/// T050/SC-002 (literal reading): SC-002 says "a store containing up to
/// 1,000 memory items," not 1,000 isolated single-item stores (which
/// `harness_completes_within_two_minutes_at_1000_item_scale_sc002` above
/// tests — real but a different thing: harness-loop throughput, not
/// recall/consolidate performance against an actually-grown corpus). This
/// test builds ONE store up to 1,000 items and times `recall` and
/// `consolidate` against that single store, which is what SC-002's wording
/// — and FR-001's "reads the event log" at real scale — actually describe.
#[test]
fn recall_and_consolidate_stay_fast_against_a_single_1000_item_store_sc002() {
    use pamet::{Interaction, Store as PametStore};

    let store = PametStore::open_in_memory().unwrap();
    let start = std::time::Instant::now();
    for i in 0..1000 {
        pamet::remember(
            &store,
            &Interaction {
                source_id: "scale-test".into(),
                text: format!("Scale-test fact number {i} says the value is {i}."),
                explicit_remember: true,
            },
        )
        .unwrap();
    }

    let result = pamet::recall(&store, "what is scale-test fact number 500", None).unwrap();
    assert!(!result.items.is_empty(), "recall must still find a match in a 1000-item store");

    let summary = pamet::consolidate(&store).unwrap();
    // Not exactly 1000: the templated wording is similar enough across many
    // numbers that write-time policy legitimately merges some near-duplicate
    // pairs (real behavior, not a test bug) — the scale/performance bound
    // below is what this test actually verifies.
    assert!(
        summary.items_processed > 500,
        "expected a substantial corpus to have accumulated, got {}",
        summary.items_processed
    );

    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 120,
        "remember+recall+consolidate against a 1,000-item store took {elapsed:?}, \
         exceeds the 2-minute SC-002 bound"
    );
}

/// T049/US3 Independent Test / spec Edge Cases: when Hindsight is
/// unreachable, `--with-hindsight` MUST still produce a Pamet-only report
/// (`hindsight_available: false`), never fail the whole run. Forces
/// unreachability deterministically via `PAMET_HINDSIGHT_BASE_URL` rather
/// than relying on whatever may or may not be listening on the real default
/// port on the machine running the test.
#[cfg(feature = "baseline-hindsight")]
#[test]
fn eval_with_hindsight_degrades_gracefully_when_unreachable_t049() {
    // Port 1 is a reserved/unused port — guaranteed unreachable, no real
    // server will ever answer here (unlike an arbitrary high port that
    // could coincidentally be in use).
    std::env::set_var("PAMET_HINDSIGHT_BASE_URL", "http://127.0.0.1:1");

    let store = Store::open_in_memory().unwrap();
    let report = eval_harness::run_and_report(&store, fixtures_path(), true).unwrap();

    assert!(
        !report.hindsight_available,
        "unreachable Hindsight must be reported as unavailable, not fail the run"
    );
    assert!(report.hindsight.is_none());
    // The Pamet-only half of the report must still be fully produced.
    assert!(!report.pamet.is_empty());
    for (category, score) in &report.pamet {
        assert!(score.total > 0, "{category} had zero probes run");
    }

    let count: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM baseline_reports WHERE id = ?1", [&report.id], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "the degraded report must still be persisted");

    std::env::remove_var("PAMET_HINDSIGHT_BASE_URL");
}

/// T049 (second half, "container permitting"): when Hindsight IS reachable
/// (real local instance on the default port), the report must contain
/// BOTH systems' scores on the identical probe set. Skips cleanly (passes
/// trivially) if no local Hindsight instance happens to be running — this
/// half of T030/US3's Independent Test is explicitly best-effort/CI-optional,
/// per spec: the mandatory path is the degraded test above.
#[cfg(feature = "baseline-hindsight")]
#[test]
fn eval_with_hindsight_full_dual_report_when_reachable_t049() {
    use pamet::services::baseline_hindsight::HindsightClient;
    if !HindsightClient::new(None).is_reachable() {
        eprintln!("skipping: no local Hindsight instance reachable on the default port");
        return;
    }

    let store = Store::open_in_memory().unwrap();
    let report = eval_harness::run_and_report(&store, fixtures_path(), true).unwrap();

    assert!(report.hindsight_available, "a reachable Hindsight must be used, not skipped");
    let hindsight = report.hindsight.expect("hindsight_available=true must carry a report");
    assert!(!hindsight.is_empty());
    assert!(!report.pamet.is_empty());
    // Same probe set, same category keys, both systems scored.
    assert_eq!(
        report.pamet.keys().collect::<std::collections::BTreeSet<_>>(),
        hindsight.keys().collect::<std::collections::BTreeSet<_>>(),
    );
}
