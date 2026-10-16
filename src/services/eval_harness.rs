//! Eval harness (research.md §5-6, FR-005..FR-009). T024-T026, T032.
//! Deterministic, exact-match scoring only — no fuzzy/LLM-judge grading
//! (Constitution Principle III; SC-003 requires byte-identical repeat runs).

use crate::error::{Error, Result};
use crate::models::{
    BaselineReport, CategoryScore, EvalProbe, EvalProbeSet, ExpectedOutcome, ProbeAction,
    SystemReport,
};
use crate::storage::Store;
use crate::Interaction;
use chrono::Utc;
use rusqlite::params;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;
use uuid::Uuid;

pub fn load_probe_set(path: &Path) -> Result<EvalProbeSet> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::Validation(format!("failed to read fixture {path:?}: {e}")))?;
    serde_json::from_str(&text).map_err(Error::Serde)
}

/// Run every probe against a FRESH in-memory store per probe (never the
/// caller's real `--db`), deterministically.
pub fn run_pamet(probe_set: &EvalProbeSet) -> Result<SystemReport> {
    let mut categories: SystemReport = BTreeMap::new();
    for probe in &probe_set.probes {
        let store = Store::open_in_memory()?;
        for step in &probe.given {
            crate::remember(
                &store,
                &Interaction {
                    source_id: step.source_id.clone(),
                    text: step.text.clone(),
                    explicit_remember: step.explicit_remember,
                },
            )?;
        }
        let t0 = Instant::now();
        let passed = run_one_probe(&store, probe)?;
        let latency_ms = t0.elapsed().as_secs_f64() * 1000.0;
        record(&mut categories, probe, passed, latency_ms);
    }
    Ok(categories)
}

#[cfg(feature = "baseline-hindsight")]
fn run_hindsight(probe_set: &EvalProbeSet) -> (bool, Option<SystemReport>) {
    let client = crate::services::baseline_hindsight::HindsightClient::new(None);
    if !client.is_reachable() {
        return (false, None);
    }
    let mut categories: SystemReport = BTreeMap::new();
    for probe in &probe_set.probes {
        let bank_id = format!("pamet_eval_{}", Uuid::new_v4().simple());
        for step in &probe.given {
            let _ = client.retain(&bank_id, &step.text);
        }
        let t0 = Instant::now();
        let passed = match (&probe.action, &probe.expected) {
            (ProbeAction::Recall { query }, ExpectedOutcome::ContainsContent { value }) => client
                .recall(&bank_id, query)
                .map(|rs| rs.iter().any(|r| r.text.contains(value.as_str())))
                .unwrap_or(false),
            (ProbeAction::Recall { query }, ExpectedOutcome::NoRelevantMemory) => {
                client.recall(&bank_id, query).map(|rs| rs.is_empty()).unwrap_or(true)
            }
            (ProbeAction::Recall { query }, ExpectedOutcome::DoesNotContain { value }) => client
                .recall(&bank_id, query)
                .map(|rs| !rs.iter().any(|r| r.text.contains(value.as_str())))
                .unwrap_or(true),
            _ => false,
        };
        let latency_ms = t0.elapsed().as_secs_f64() * 1000.0;
        record(&mut categories, probe, passed, latency_ms);
    }
    (true, Some(categories))
}

#[cfg(not(feature = "baseline-hindsight"))]
fn run_hindsight(_probe_set: &EvalProbeSet) -> (bool, Option<SystemReport>) {
    (false, None)
}

fn record(categories: &mut SystemReport, probe: &EvalProbe, passed: bool, latency_ms: f64) {
    let entry = categories.entry(probe.category.as_str().to_string()).or_insert_with(CategoryScore::default);
    entry.total += 1;
    if passed {
        entry.passed += 1;
    }
    // Small fixture sets don't have enough samples for a real percentile —
    // max observed is used as the p95 stand-in (documented, not hidden).
    entry.p95_latency_ms = entry.p95_latency_ms.max(latency_ms);
}

fn run_one_probe(store: &Store, probe: &EvalProbe) -> Result<bool> {
    match (&probe.action, &probe.expected) {
        (ProbeAction::Recall { query }, ExpectedOutcome::ContainsContent { value }) => {
            let result = crate::recall(store, query, None)?;
            Ok(result.items.iter().any(|i| i.content.contains(value.as_str())))
        }
        (ProbeAction::Recall { query }, ExpectedOutcome::NoRelevantMemory) => {
            let result = crate::recall(store, query, None)?;
            Ok(result.items.is_empty())
        }
        (ProbeAction::Recall { query }, ExpectedOutcome::DoesNotContain { value }) => {
            let result = crate::recall(store, query, None)?;
            Ok(!result.items.iter().any(|i| i.content.contains(value.as_str())))
        }
        (ProbeAction::Remember { text }, _) => {
            let outcomes = crate::remember(
                store,
                &Interaction { source_id: "probe".into(), text: text.clone(), explicit_remember: true },
            )?;
            Ok(!outcomes.is_empty())
        }
    }
}

/// Run the full harness and persist the report (FR-008). `store` is the
/// real `--db` the CLI was invoked with — used ONLY to record the result,
/// never mutated with probe data (every probe runs on its own temp store).
pub fn run_and_report(
    store: &Store,
    fixtures_path: &Path,
    with_hindsight: bool,
) -> Result<BaselineReport> {
    let probe_set = load_probe_set(fixtures_path)?;
    let pamet = run_pamet(&probe_set)?;
    let (hindsight_available, hindsight) =
        if with_hindsight { run_hindsight(&probe_set) } else { (false, None) };

    let report = BaselineReport {
        id: Uuid::new_v4().to_string(),
        probe_set_version: probe_set.version.clone(),
        generated_at: Utc::now().to_rfc3339(),
        pamet,
        hindsight_available,
        hindsight,
    };
    persist_report(store, &report)?;
    Ok(report)
}

fn persist_report(store: &Store, report: &BaselineReport) -> Result<()> {
    let pamet_json = serde_json::to_string(&report.pamet)?;
    let hindsight_json = report.hindsight.as_ref().map(serde_json::to_string).transpose()?;
    crate::services::events::append_event(
        store,
        "eval_run_completed",
        &serde_json::json!({
            "report_id": report.id, "probe_set_version": report.probe_set_version,
            "generated_at": report.generated_at, "pamet_results_json": pamet_json,
            "hindsight_results_json": hindsight_json, "hindsight_available": report.hindsight_available,
        }),
    )?;
    store.conn.execute(
        "INSERT INTO baseline_reports (id, probe_set_version, generated_at, pamet_results_json, \
            hindsight_results_json, hindsight_available) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            report.id, report.probe_set_version, report.generated_at, pamet_json,
            hindsight_json, report.hindsight_available as i64
        ],
    )?;
    Ok(())
}

pub fn write_report_file(report: &BaselineReport, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json).map_err(|e| Error::Validation(format!("failed to write report to {path:?}: {e}")))?;
    Ok(())
}

pub fn pamet_report_all_passed(report: &BaselineReport) -> bool {
    crate::models::eval::all_passed(&report.pamet)
}
