//! pamet-core public library API (contracts/library-api.md).

pub mod error;
pub mod models;
pub mod services;
pub mod storage;

pub use error::{Error, Result};
pub use models::{Decision, ItemType, MemoryItem};
pub use storage::Store;

use chrono::Utc;
use models::{RankedCandidate, ReviewReason};
use rusqlite::params;
use uuid::Uuid;

pub struct Interaction {
    pub source_id: String,
    pub text: String,
    pub explicit_remember: bool,
}

#[derive(Debug, Clone)]
pub struct WriteDecisionOutcome {
    pub decision: Decision,
    pub rule_name: String,
    pub resulting_item_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RetrievalResult {
    pub event_id: i64,
    pub items: Vec<MemoryItem>,
}

const DEFAULT_RESULT_BUDGET: usize = 5;
/// Candidates below this fused score never clear the relevance bar
/// (FR-008 — "no relevant memory found" instead of forcing weak matches in).
const RELEVANCE_FLOOR: f32 = 0.0001;

/// T015/T043: extract candidate(s) from an interaction, run the write policy
/// on each, and persist exactly one WriteDecision per candidate (spec
/// SC-001; FR-001; spec.md Edge Case "same interaction yields more than one
/// memory candidate").
pub fn remember(store: &Store, interaction: &Interaction) -> Result<Vec<WriteDecisionOutcome>> {
    let candidate_texts = extract_candidate_texts(&interaction.text);
    let mut outcomes = Vec::with_capacity(candidate_texts.len());
    for text in candidate_texts {
        outcomes.push(remember_one(store, interaction, text)?);
    }
    Ok(outcomes)
}

/// Deterministic, hand-written extraction rule (Constitution Principle III —
/// rules before learning): split on sentence-ending punctuation / newlines so
/// a single interaction containing more than one distinct statement yields
/// one independently-logged candidate per statement, instead of collapsing
/// them into one write decision.
fn extract_candidate_texts(text: &str) -> Vec<String> {
    let parts: Vec<String> = text
        .split(['.', '!', '?', '\n'])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        // Nothing survived splitting (e.g. the text was pure punctuation) —
        // fall back to the whole trimmed text as a single candidate so every
        // interaction still produces at least one decision (spec SC-001).
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![trimmed]
        }
    } else {
        parts
    }
}

fn remember_one(
    store: &Store,
    interaction: &Interaction,
    text: String,
) -> Result<WriteDecisionOutcome> {
    let candidate = services::write_policy::Candidate {
        content: text,
        item_type: ItemType::Fact,
        explicit_remember: interaction.explicit_remember,
    };

    let outcome = services::write_policy::evaluate(store, &candidate)?;

    // Resolve the resulting/target item id(s) BEFORE appending the event, so
    // the event payload is self-sufficient for replay (Constitution
    // Principle I — the event log alone must be enough to rebuild derived
    // state; it must not depend on re-running non-deterministic steps like
    // UUID generation at replay time).
    let new_item_id = match outcome.decision {
        Decision::Store | Decision::Supersede => Some(Uuid::new_v4().to_string()),
        _ => None,
    };
    let target_item_id = outcome.target_item.as_ref().map(|i| i.id.clone());
    let resulting_item_id = match outcome.decision {
        Decision::Store | Decision::Supersede => new_item_id.clone(),
        Decision::Merge => target_item_id.clone(),
        Decision::Skip | Decision::Review => None,
    };
    let review_reason = if outcome.decision == Decision::Review {
        Some(if outcome.rule_name.contains("contradiction") {
            ReviewReason::UnresolvedContradiction
        } else {
            ReviewReason::LowConfidence
        })
    } else {
        None
    };

    let event_payload = serde_json::json!({
        "source_id": interaction.source_id,
        "candidate_content": candidate.content,
        "decision": outcome.decision.as_str(),
        "rule_name": outcome.rule_name,
        "rule_inputs": outcome.rule_inputs,
        "resulting_item_id": resulting_item_id,
        "target_item_id": target_item_id,
        "review_reason": review_reason.map(|r| r.as_str()),
    });
    let event_id = services::events::append_event(store, "write_decision", &event_payload)?;
    let now = Utc::now().to_rfc3339();

    match outcome.decision {
        Decision::Store => {
            insert_item(store, new_item_id.as_ref().unwrap(), &candidate, event_id, None, 0.9)?;
        }
        Decision::Merge => {
            merge_into(store, target_item_id.as_ref().unwrap(), &candidate.content)?;
        }
        Decision::Supersede => {
            insert_item(
                store,
                new_item_id.as_ref().unwrap(),
                &candidate,
                event_id,
                target_item_id.clone(),
                0.9,
            )?;
        }
        Decision::Skip | Decision::Review => {}
    }

    store.conn.execute(
        "INSERT INTO write_decisions (event_id, candidate_content, decision, rule_name, \
            rule_inputs_json, resulting_item_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            event_id,
            candidate.content,
            outcome.decision.as_str(),
            outcome.rule_name,
            outcome.rule_inputs.to_string(),
            resulting_item_id,
            now,
        ],
    )?;
    let write_decision_id = store.conn.last_insert_rowid();

    if let Some(reason) = review_reason {
        store.conn.execute(
            "INSERT INTO review_queue (write_decision_id, reason, status, created_at) \
             VALUES (?1, ?2, 'pending', ?3)",
            params![write_decision_id, reason.as_str(), now],
        )?;
    }

    Ok(WriteDecisionOutcome {
        decision: outcome.decision,
        rule_name: outcome.rule_name,
        resulting_item_id,
    })
}

fn insert_item(
    store: &Store,
    id: &str,
    candidate: &services::write_policy::Candidate,
    event_id: i64,
    supersedes_id: Option<String>,
    confidence: f32,
) -> Result<()> {
    let embedding = services::embedding::embed(&candidate.content);
    let now = Utc::now().to_rfc3339();
    store.conn.execute(
        "INSERT INTO memory_items (id, item_type, content, embedding_json, entities_json, \
            confidence, provenance_event_id, supersedes_id, created_at, use_count) \
         VALUES (?1, ?2, ?3, ?4, '[]', ?5, ?6, ?7, ?8, 0)",
        params![
            id,
            candidate.item_type.as_str(),
            candidate.content,
            serde_json::to_string(&embedding)?,
            confidence as f64,
            event_id,
            supersedes_id,
            now,
        ],
    )?;
    store.conn.execute(
        "INSERT INTO memory_items_fts (id, content) VALUES (?1, ?2)",
        params![id, candidate.content],
    )?;
    Ok(())
}

fn merge_into(store: &Store, item_id: &str, new_content: &str) -> Result<()> {
    // Merge = refresh last-touched content without creating a duplicate row.
    store.conn.execute(
        "UPDATE memory_items SET content = ?1 WHERE id = ?2",
        params![new_content, item_id],
    )?;
    store.conn.execute(
        "UPDATE memory_items_fts SET content = ?1 WHERE id = ?2",
        params![new_content, item_id],
    )?;
    Ok(())
}

/// T026: hybrid retrieval — vector + keyword, fused with RRF, excludes
/// tombstoned items, bounded to `budget`.
pub fn recall(store: &Store, query: &str, budget: Option<usize>) -> Result<RetrievalResult> {
    let budget = budget.unwrap_or(DEFAULT_RESULT_BUDGET);

    let vector_candidates = services::retrieval_vector::search(store, query, budget * 3)?;
    let keyword_candidates = services::retrieval_keyword::search(store, query, budget * 3)?;
    let mut fused = services::retrieval_fusion::fuse(&[&vector_candidates, &keyword_candidates]);
    services::retrieval_fusion::apply_decay(
        store,
        &mut fused,
        models::decay::DECAY_AFTER_DAYS,
        models::decay::DECAY_PENALTY,
    )?;

    // T028: exclude anything currently pending review (not yet an accepted
    // memory) — tombstoned items are already excluded by the SQL in the
    // per-method searches above.
    let pending_review_items = pending_review_candidate_ids(store)?;
    let filtered: Vec<RankedCandidate> = fused
        .into_iter()
        .filter(|c| c.score > RELEVANCE_FLOOR && !pending_review_items.contains(&c.item_id))
        .collect();

    let bounded: Vec<RankedCandidate> = filtered.into_iter().take(budget).collect();
    let returned_item_ids: Vec<String> = bounded.iter().map(|c| c.item_id.clone()).collect();

    let event_payload = serde_json::json!({
        "query_text": query,
        "vector_candidates": vector_candidates,
        "keyword_candidates": keyword_candidates,
        "fused_ranking": bounded,
        "result_budget": budget,
        "returned_item_ids": returned_item_ids,
    });
    let event_id = services::events::append_event(store, "retrieval", &event_payload)?;
    let now = Utc::now().to_rfc3339();
    store.conn.execute(
        "INSERT INTO retrieval_events (event_id, query_text, vector_candidates_json, \
            keyword_candidates_json, fused_ranking_json, result_budget, returned_item_ids_json, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            event_id,
            query,
            serde_json::to_string(&vector_candidates)?,
            serde_json::to_string(&keyword_candidates)?,
            serde_json::to_string(&bounded)?,
            budget as i64,
            serde_json::to_string(&returned_item_ids)?,
            now,
        ],
    )?;

    let items = fetch_items(store, &returned_item_ids)?;
    Ok(RetrievalResult { event_id, items })
}

fn pending_review_candidate_ids(store: &Store) -> Result<std::collections::HashSet<String>> {
    let mut stmt = store.conn.prepare(
        "SELECT wd.resulting_item_id FROM review_queue rq \
         JOIN write_decisions wd ON wd.id = rq.write_decision_id \
         WHERE rq.status = 'pending' AND wd.resulting_item_id IS NOT NULL",
    )?;
    let mut out = std::collections::HashSet::new();
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for r in rows {
        out.insert(r?);
    }
    Ok(out)
}

fn fetch_items(store: &Store, ids: &[String]) -> Result<Vec<MemoryItem>> {
    let mut items = Vec::new();
    for id in ids {
        let item = store.conn.query_row(
            "SELECT id, item_type, content, embedding_json, entities_json, confidence, \
                    provenance_event_id, supersedes_id, created_at, last_used_at, use_count, tombstoned_at \
             FROM memory_items WHERE id = ?1",
            params![id],
            |row| {
                let embedding_json: String = row.get(3)?;
                Ok(MemoryItem {
                    id: row.get(0)?,
                    item_type: ItemType::from_str(&row.get::<_, String>(1)?).unwrap_or(ItemType::Fact),
                    content: row.get(2)?,
                    embedding: serde_json::from_str(&embedding_json).unwrap_or_default(),
                    entities: Vec::new(),
                    confidence: row.get::<_, f64>(5)? as f32,
                    provenance_event_id: row.get(6)?,
                    supersedes_id: row.get(7)?,
                    created_at: row.get(8)?,
                    last_used_at: row.get(9)?,
                    use_count: row.get(10)?,
                    tombstoned_at: row.get(11)?,
                })
            },
        )?;
        items.push(item);
    }
    Ok(items)
}

/// T032 wrapper per contracts/library-api.md.
pub fn report_outcome(store: &Store, event_id: i64, item_id: &str, used: bool) -> Result<()> {
    services::outcome::report_outcome(store, event_id, item_id, used)
}

/// T041: rebuild a brand-new in-memory store purely by replaying `store`'s
/// event log (Constitution Principle I). Used for drift-checking that the
/// event log alone is sufficient to reconstruct every derived table.
pub fn rebuild_from_events(store: &Store) -> Result<Store> {
    services::rebuild::rebuild_from_events(store)
}

/// Resolve a pending review entry.
pub fn resolve_review(store: &Store, review_id: i64, decision: Decision) -> Result<()> {
    services::review::resolve(store, review_id, decision)
}

/// Tombstone a memory item (FR-014) — reversible via the event log, never a
/// hard delete.
pub fn tombstone(store: &Store, item_id: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let updated = store.conn.execute(
        "UPDATE memory_items SET tombstoned_at = ?1 WHERE id = ?2 AND tombstoned_at IS NULL",
        params![now, item_id],
    )?;
    if updated == 0 {
        return Err(Error::Validation(format!("item {item_id} not found or already tombstoned")));
    }
    let payload = serde_json::json!({ "item_id": item_id });
    services::events::append_event(store, "tombstone", &payload)?;
    Ok(())
}

/// T033 wrapper: replay-ordered export of the event log (spec SC-006).
pub fn export_log(store: &Store, since: Option<&str>) -> Result<Vec<services::events::Event>> {
    services::events::export_log(store, since)
}

/// Phase 2: run one consolidation pass (FR-001..FR-004, FR-010) — a
/// separate entry point from `remember`/`recall` (Principle IV).
pub fn consolidate(store: &Store) -> Result<services::consolidation::ConsolidationSummary> {
    services::consolidation::run(store)
}
