use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ItemType {
    Episode,
    Fact,
    Preference,
    Procedure,
    Summary,
}

impl ItemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemType::Episode => "episode",
            ItemType::Fact => "fact",
            ItemType::Preference => "preference",
            ItemType::Procedure => "procedure",
            ItemType::Summary => "summary",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "episode" => Some(ItemType::Episode),
            "fact" => Some(ItemType::Fact),
            "preference" => Some(ItemType::Preference),
            "procedure" => Some(ItemType::Procedure),
            "summary" => Some(ItemType::Summary),
            _ => None,
        }
    }
}

/// A stored memory item (data-model.md `memory_items`). T012.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub id: String,
    pub item_type: ItemType,
    pub content: String,
    pub embedding: Vec<f32>,
    pub entities: Vec<String>,
    pub confidence: f32,
    pub provenance_event_id: i64,
    pub supersedes_id: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub use_count: i64,
    pub tombstoned_at: Option<String>,
}
