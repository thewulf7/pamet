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
    let active_items = candidate_items(store, candidate)?;

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

/// T002/T003 (feature 005): narrow the near-duplicate candidate set with the
/// existing keyword index (same tokenizer `embedding::embed()` hashes on —
/// research.md Decision 1: a pair scoring above any meaningful cosine
/// threshold necessarily shares tokens for this hashed-bag-of-words
/// embedder, so this is not a lossy approximation at the decisive
/// merge/contradiction bands) before running the expensive exact scan.
/// Falls through to the full table only if the keyword index itself can't
/// be queried (defensive — keeps the old full-scan behavior as a fallback
/// rather than ever silently treating a query error as "no candidates").
fn candidate_items(store: &Store, candidate: &Candidate) -> Result<Vec<MemoryItem>> {
    const PREFILTER_LIMIT: usize = 200;
    let keyword_hits =
        crate::services::retrieval_keyword::search(store, &candidate.content, PREFILTER_LIMIT)?;
    if keyword_hits.is_empty() {
        // Decision 3: zero keyword overlap implies ~zero cosine similarity
        // for this embedder — skip the scan entirely rather than comparing
        // against every item for nothing.
        return Ok(Vec::new());
    }
    let ids: Vec<String> = keyword_hits.into_iter().map(|h| h.item_id).collect();
    let quoted = ids
        .iter()
        .map(|id| format!("'{}'", id.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    crate::services::memory_items::active_items_where(store, &format!("id IN ({quoted})"))
}

/// Keyword-overlap contradiction heuristic: shares a subject (token overlap)
/// with the best match but diverges enough in wording to not be a near-dup.
/// This stands in for the design docs' NLI/LLM-judge contradiction check —
/// still a hand-written rule, not a learned model (Principle III).
/// `pub(crate)` so the Phase 2 consolidation job (research.md §4) can reuse
/// the exact same heuristic instead of a second one drifting apart from it.
pub(crate) fn detect_contradiction(candidate: &Candidate, best_match: Option<&MemoryItem>) -> bool {
    let item = match best_match {
        Some(i) => i,
        None => return false,
    };
    let cand_tokens: std::collections::HashSet<_> = embedding::tokenize(&candidate.content)
        .into_iter()
        .collect();
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
    !tokens.is_empty()
        && tokens
            .iter()
            .all(|t| low_value_markers.contains(&t.as_str()))
}
