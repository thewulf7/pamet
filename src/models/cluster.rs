//! Consolidation models (data-model.md). T011.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConsolidationStatus {
    Running,
    Completed,
    Interrupted,
}

impl ConsolidationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConsolidationStatus::Running => "running",
            ConsolidationStatus::Completed => "completed",
            ConsolidationStatus::Interrupted => "interrupted",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "running" => Some(ConsolidationStatus::Running),
            "completed" => Some(ConsolidationStatus::Completed),
            "interrupted" => Some(ConsolidationStatus::Interrupted),
            _ => None,
        }
    }
}

/// One execution of the consolidation job (data-model.md `consolidation_runs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationRun {
    pub id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub items_processed: i64,
    pub clusters_formed: i64,
    pub contradictions_flagged: i64,
    pub status: ConsolidationStatus,
}

/// A group of near-duplicate items identified during consolidation, plus the
/// resulting summary item id it produced (data-model.md `cluster_members`
/// carries this at the row level; this struct is the in-memory grouping used
/// while clustering before anything is written).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cluster {
    pub cluster_id: String,
    pub member_item_ids: Vec<String>,
}

/// One `cluster_members` row (data-model.md).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMember {
    pub id: String,
    pub cluster_id: String,
    pub summary_item_id: String,
    pub source_item_id: String,
    pub consolidation_run_id: String,
    pub created_at: String,
}
