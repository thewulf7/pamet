use serde::{Deserialize, Serialize};

/// One served query (data-model.md `retrieval_events`). T022.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalEvent {
    pub id: i64,
    pub event_id: i64,
    pub query_text: String,
    pub vector_candidates: Vec<RankedCandidate>,
    pub keyword_candidates: Vec<RankedCandidate>,
    pub fused_ranking: Vec<RankedCandidate>,
    pub result_budget: usize,
    pub returned_item_ids: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedCandidate {
    pub item_id: String,
    pub score: f32,
}
