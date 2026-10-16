use serde::{Deserialize, Serialize};

/// Usage feedback attached after the fact to a retrieval event
/// (data-model.md `outcome_records`). T031.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeRecord {
    pub id: i64,
    pub retrieval_event_id: i64,
    pub item_id: String,
    pub used: bool,
    pub reported_at: String,
}
