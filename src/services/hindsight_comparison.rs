//! Side-by-side quality/speed comparison harness (feature 004, US3,
//! FR-007/FR-008). Runs a fixed list of real queries against both the live
//! Hindsight bank and a migrated Pamet store and writes a durable report —
//! deliberately not a pass/fail score (Assumptions: no labeled ground truth
//! exists for this conversational history), just latency + raw results for
//! the operator to read.

use crate::error::{Error, Result};
use crate::services::baseline_hindsight::HindsightClient;
use crate::Store;
use serde::Serialize;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub struct SystemResult {
    pub latency_ms: f64,
    pub ok: bool,
    pub error: Option<String>,
    pub results: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct QueryComparison {
    pub query: String,
    pub pamet: SystemResult,
    pub hindsight: SystemResult,
}

#[derive(Debug, Serialize)]
pub struct ComparisonReport {
    pub generated_at: String,
    pub bank_id: String,
    pub queries: Vec<QueryComparison>,
}

fn run_pamet_query(store: &Store, query: &str) -> SystemResult {
    let t0 = Instant::now();
    match crate::recall(store, query, None) {
        Ok(r) => SystemResult {
            latency_ms: t0.elapsed().as_secs_f64() * 1000.0,
            ok: true,
            error: None,
            results: r.items.into_iter().map(|i| i.content).collect(),
        },
        Err(e) => SystemResult {
            latency_ms: t0.elapsed().as_secs_f64() * 1000.0,
            ok: false,
            error: Some(e.to_string()),
            results: vec![],
        },
    }
}

fn run_hindsight_query(client: &HindsightClient, bank_id: &str, query: &str) -> SystemResult {
    let t0 = Instant::now();
    match client.recall(bank_id, query) {
        Ok(rs) => SystemResult {
            latency_ms: t0.elapsed().as_secs_f64() * 1000.0,
            ok: true,
            error: None,
            results: rs.into_iter().map(|r| r.text).collect(),
        },
        Err(e) => SystemResult {
            latency_ms: t0.elapsed().as_secs_f64() * 1000.0,
            ok: false,
            error: Some(e),
            results: vec![],
        },
    }
}

/// FR-007: every query in `queries` is run against both systems — neither
/// side is skipped even if one errors (the error is captured in the report
/// instead, per this session's discovery that Hindsight's own `recall` is
/// currently broken for the live bank).
pub fn run_comparison(
    pamet_store: &Store,
    hindsight: &HindsightClient,
    bank_id: &str,
    queries: &[String],
) -> ComparisonReport {
    let comparisons = queries
        .iter()
        .map(|q| QueryComparison {
            query: q.clone(),
            pamet: run_pamet_query(pamet_store, q),
            hindsight: run_hindsight_query(hindsight, bank_id, q),
        })
        .collect();

    ComparisonReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        bank_id: bank_id.to_string(),
        queries: comparisons,
    }
}

/// FR-008: the report MUST be a durable file, not console-only.
pub fn write_report_file(report: &ComparisonReport, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)
        .map_err(|e| Error::Validation(format!("failed to write report to {path:?}: {e}")))?;
    Ok(())
}

pub fn load_queries(path: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::Validation(format!("failed to read queries {path:?}: {e}")))?;
    serde_json::from_str(&text).map_err(Error::Serde)
}
