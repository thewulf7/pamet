//! Eval harness models (data-model.md). T022.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ProbeCategory {
    NewFactRecall,
    UpdateSupersession,
    ForgettingAbstention,
    GeneralPrecision,
}

impl ProbeCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProbeCategory::NewFactRecall => "new_fact_recall",
            ProbeCategory::UpdateSupersession => "update_supersession",
            ProbeCategory::ForgettingAbstention => "forgetting_abstention",
            ProbeCategory::GeneralPrecision => "general_precision",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "new_fact_recall" => Some(ProbeCategory::NewFactRecall),
            "update_supersession" => Some(ProbeCategory::UpdateSupersession),
            "forgetting_abstention" => Some(ProbeCategory::ForgettingAbstention),
            "general_precision" => Some(ProbeCategory::GeneralPrecision),
            _ => None,
        }
    }
}

/// One `given` setup step — mirrors a real `remember` call (FR-009: fixture
/// shape mirrors the real event-log shape so a future swap to real usage
/// logs is additive).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GivenStep {
    pub text: String,
    #[serde(default = "default_source_id")]
    pub source_id: String,
    #[serde(default)]
    pub explicit_remember: bool,
}

fn default_source_id() -> String {
    "fixture".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProbeAction {
    Recall { query: String },
    Remember { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExpectedOutcome {
    ContainsContent { value: String },
    NoRelevantMemory,
    DoesNotContain { value: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalProbe {
    pub id: String,
    pub category: ProbeCategory,
    #[serde(default)]
    pub given: Vec<GivenStep>,
    pub action: ProbeAction,
    pub expected: ExpectedOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalProbeSet {
    pub version: String,
    pub probes: Vec<EvalProbe>,
}

/// One probe's outcome in one harness run (data-model.md `eval_results`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    pub id: String,
    pub run_id: String,
    pub probe_id: String,
    pub category: ProbeCategory,
    pub probe_set_version: String,
    pub passed: bool,
    pub ran_at: String,
    pub latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CategoryScore {
    pub passed: u32,
    pub total: u32,
    pub p95_latency_ms: f64,
}

/// Category name -> score. A flat map (not a nested struct) so the JSON
/// shape matches contracts/cli-contract.md exactly:
/// `"pamet": {"new_fact_recall": {...}, ...}`.
pub type SystemReport = BTreeMap<String, CategoryScore>;

pub fn all_passed(report: &SystemReport) -> bool {
    report.values().all(|c| c.passed == c.total)
}

/// Versioned, timestamped comparison report (data-model.md `baseline_reports`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineReport {
    pub id: String,
    pub probe_set_version: String,
    pub generated_at: String,
    pub pamet: SystemReport,
    pub hindsight_available: bool,
    pub hindsight: Option<SystemReport>,
}
