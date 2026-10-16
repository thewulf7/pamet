//! Rule-based write policy (T014, T018, T019). No learned component —
//! Constitution Principle III: rules before learning.

use crate::error::Result;
use crate::models::{Decision, ItemType, MemoryItem};
use crate::services::embedding;
use crate::storage::Store;
use serde_json::json;

/// Fixed, documented thresholds (research.md "Write policy" decision).
pub const MERGE_SIMILARITY_THRESHOLD: f32 = 0.85;
pub const CONTRADICTION_SIMILARITY_FLOOR: f32 = 0.35;
pub const LOW_CONFIDENCE_THRESHOLD: f32 = 0.5;

pub struct Candidate {
    pub content: String,
    pub item_type: ItemType,
    pub explicit_remember: bool,
}

/// Decide what to do with one candidate against the existing active items.
/// Returns the decision, the name of the rule that fired, the rule inputs
/// (for FR-009 logging), and — if the decision resolves a target — the
/// existing item it should merge into / supersede.
pub struct PolicyOutcome {
    pub decision: Decision,
    pub rule_name: String,
    pub rule_inputs: serde_json::Value,
    pub target_item: Option<MemoryItem>,
    pub contradiction_item: Option<MemoryItem>,
}

/// T014: core rule evaluation. T018: provenance/validation is enforced by
/// the caller (`remember()` in lib.rs), which always supplies a real event id.
pub fn evaluate(store: &Store, candidate: &Candidate) -> Result<PolicyOutcome> {
    let embedding_vec = embedding::embed(&candidate.content);
    let active_items = active_items(store)?;

    let mut best_match: Option<(f32, MemoryItem)> = None;
    for item in &active_items {
        let sim = embedding::cosine_similarity(&embedding_vec, &item.embedding);
        if best_match.as_ref().map(|(s, _)| sim > *s).unwrap_or(true) {
            best_match = Some((sim, item.clone()));
        }
    }

    let novelty = best_match.as_ref().map(|(s, _)| 1.0 - s).unwrap_or(1.0);
    let max_similarity = best_match.as_ref().map(|(s, _)| *s).unwrap_or(0.0);
    let contradicts = detect_contradiction(candidate, best_match.as_ref().map(|(_, i)| i));

    let rule_inputs = json!({
        "max_similarity": max_similarity,
        "novelty": novelty,
        "contradicts": contradicts,
        "explicit_remember": candidate.explicit_remember,
        "merge_threshold": MERGE_SIMILARITY_THRESHOLD,
        "contradiction_floor": CONTRADICTION_SIMILARITY_FLOOR,
    });

    // Rule order mirrors spec FR-002..FR-004 / Acceptance Scenarios 1-5.
    if contradicts {
        if let Some((sim, item)) = &best_match {
            if *sim < CONTRADICTION_SIMILARITY_FLOOR {
                // Confidently contradicts an existing item -> supersede.
                return Ok(PolicyOutcome {
                    decision: Decision::Supersede,
                    rule_name: "contradiction_supersede".into(),
                    rule_inputs,
                    target_item: Some(item.clone()),
                    contradiction_item: Some(item.clone()),
                });
            }
        }
        // Contradiction detected but not confidently resolved -> review queue.
        return Ok(PolicyOutcome {
            decision: Decision::Review,
            rule_name: "unresolved_contradiction_review".into(),
            rule_inputs,
            target_item: best_match.map(|(_, i)| i),
            contradiction_item: None,
        });
    }

    if max_similarity >= MERGE_SIMILARITY_THRESHOLD {
        return Ok(PolicyOutcome {
            decision: Decision::Merge,
            rule_name: "near_duplicate_merge".into(),
            rule_inputs,
            target_item: best_match.map(|(_, i)| i),
            contradiction_item: None,
        });
    }

    // Ambiguous middle band: similar enough to maybe be the same thing, but
    // not similar enough to confidently merge -> low-confidence review
    // (FR-004), unless the user explicitly asked to remember it verbatim.
    if !candidate.explicit_remember
        && max_similarity >= LOW_CONFIDENCE_THRESHOLD
        && max_similarity < MERGE_SIMILARITY_THRESHOLD
    {
        return Ok(PolicyOutcome {
            decision: Decision::Review,
            rule_name: "ambiguous_similarity_review".into(),
            rule_inputs,
            target_item: best_match.map(|(_, i)| i),
            contradiction_item: None,
        });
    }

    if !candidate.explicit_remember && is_low_value(&candidate.content) {
        return Ok(PolicyOutcome {
            decision: Decision::Skip,
            rule_name: "low_value_skip".into(),
            rule_inputs,
            target_item: None,
            contradiction_item: None,
        });
    }

    Ok(PolicyOutcome {
        decision: Decision::Store,
        rule_name: "novel_store".into(),
        rule_inputs,
        target_item: None,
        contradiction_item: None,
    })
}

fn active_items(store: &Store) -> Result<Vec<MemoryItem>> {
    let mut stmt = store.conn.prepare(
        "SELECT id, item_type, content, embedding_json, entities_json, confidence, \
                provenance_event_id, supersedes_id, created_at, last_used_at, use_count, tombstoned_at \
         FROM memory_items WHERE tombstoned_at IS NULL",
    )?;
    let mut out = Vec::new();
    let rows = stmt.query_map([], |row| {
        let embedding_json: String = row.get(3)?;
        let entities_json: Option<String> = row.get(4)?;
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
            embedding_json, entities_json, row.get::<_, f64>(5)?, row.get::<_, i64>(6)?,
            row.get::<_, Option<String>>(7)?, row.get::<_, String>(8)?,
            row.get::<_, Option<String>>(9)?, row.get::<_, i64>(10)?,
            row.get::<_, Option<String>>(11)?))
    })?;
    for r in rows {
        let (id, item_type, content, embedding_json, entities_json, confidence,
             provenance_event_id, supersedes_id, created_at, last_used_at, use_count, tombstoned_at) = r?;
        out.push(MemoryItem {
            id,
            item_type: ItemType::from_str(&item_type).unwrap_or(ItemType::Fact),
            content,
            embedding: serde_json::from_str(&embedding_json).unwrap_or_default(),
            entities: entities_json
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default(),
            confidence: confidence as f32,
            provenance_event_id,
            supersedes_id,
            created_at,
            last_used_at,
            use_count,
            tombstoned_at,
        });
    }
    Ok(out)
}

/// Keyword-overlap contradiction heuristic: shares a subject (token overlap)
/// with the best match but diverges enough in wording to not be a near-dup.
/// This stands in for the design docs' NLI/LLM-judge contradiction check —
/// still a hand-written rule, not a learned model (Principle III).
fn detect_contradiction(candidate: &Candidate, best_match: Option<&MemoryItem>) -> bool {
    let item = match best_match {
        Some(i) => i,
        None => return false,
    };
    let cand_tokens: std::collections::HashSet<_> =
        embedding::tokenize(&candidate.content).into_iter().collect();
    let item_tokens: std::collections::HashSet<_> =
        embedding::tokenize(&item.content).into_iter().collect();
    let shared = cand_tokens.intersection(&item_tokens).count();
    let contradiction_markers = ["actually", "no longer", "instead", "not", "now"];
    let has_marker = contradiction_markers
        .iter()
        .any(|m| candidate.content.to_lowercase().contains(m));
    shared >= 2 && has_marker
}

fn is_low_value(content: &str) -> bool {
    let low_value_markers = ["lol", "ok", "k", "haha", "hi", "hello"];
    let tokens = embedding::tokenize(content);
    !tokens.is_empty() && tokens.iter().all(|t| low_value_markers.contains(&t.as_str()))
}
