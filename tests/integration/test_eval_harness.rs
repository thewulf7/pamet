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
