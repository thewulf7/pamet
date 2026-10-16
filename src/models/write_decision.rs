use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Decision {
    Store,
    Merge,
    Supersede,
    Skip,
    Review,
}

impl Decision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Decision::Store => "store",
            Decision::Merge => "merge",
            Decision::Supersede => "supersede",
            Decision::Skip => "skip",
            Decision::Review => "review",
        }
    }
}

/// Outcome of processing one write candidate (data-model.md `write_decisions`). T013.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteDecisionRecord {
    pub id: i64,
    pub event_id: i64,
    pub candidate_content: String,
    pub decision: Decision,
    pub rule_name: String,
    pub rule_inputs: serde_json::Value,
    pub resulting_item_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewReason {
    LowConfidence,
    UnresolvedContradiction,
    ConsolidationContradiction,
    ConsolidationClusterPromotion,
}

impl ReviewReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewReason::LowConfidence => "low_confidence",
            ReviewReason::UnresolvedContradiction => "unresolved_contradiction",
            ReviewReason::ConsolidationContradiction => "consolidation_contradiction",
            ReviewReason::ConsolidationClusterPromotion => "consolidation_cluster_promotion",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewStatus {
    Pending,
    ResolvedStore,
    ResolvedMerge,
    ResolvedSupersede,
    ResolvedSkip,
}

impl ReviewStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewStatus::Pending => "pending",
            ReviewStatus::ResolvedStore => "resolved_store",
            ReviewStatus::ResolvedMerge => "resolved_merge",
            ReviewStatus::ResolvedSupersede => "resolved_supersede",
            ReviewStatus::ResolvedSkip => "resolved_skip",
        }
    }

    pub fn from_decision(d: Decision) -> Self {
        match d {
            Decision::Store => ReviewStatus::ResolvedStore,
            Decision::Merge => ReviewStatus::ResolvedMerge,
            Decision::Supersede => ReviewStatus::ResolvedSupersede,
            Decision::Skip => ReviewStatus::ResolvedSkip,
            Decision::Review => ReviewStatus::Pending,
        }
    }
}

/// A write candidate held for review (data-model.md `review_queue`). T013.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewQueueEntry {
    pub id: i64,
    pub write_decision_id: i64,
    pub reason: ReviewReason,
    pub status: ReviewStatus,
    pub created_at: String,
    pub resolved_at: Option<String>,
}
