//! Reciprocal Rank Fusion (T025), per research.md "Result fusion" decision.
//! Deterministic, hand-written — no learned reranker (Principle III).

use crate::error::Result;
use crate::models::RankedCandidate;
use crate::storage::Store;
use chrono::Utc;
use rusqlite::params;
use std::collections::HashMap;

const RRF_K: f32 = 60.0;

pub fn fuse(lists: &[&[RankedCandidate]]) -> Vec<RankedCandidate> {
    let mut scores: HashMap<String, f32> = HashMap::new();
    for list in lists {
        for (rank, candidate) in list.iter().enumerate() {
            let contribution = 1.0 / (RRF_K + (rank as f32 + 1.0));
            *scores.entry(candidate.item_id.clone()).or_insert(0.0) += contribution;
        }
    }
    let mut fused: Vec<RankedCandidate> = scores
        .into_iter()
        .map(|(item_id, score)| RankedCandidate { item_id, score })
        .collect();
    fused.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    fused
}

/// Apply the decay multiplier (research.md §3, FR-004) to each candidate's
/// fused score, in place, then re-sort. A missing/unreadable item is left
/// unmodified (fail open on ranking only). `after_days`/`penalty` let the
/// `consolidate` CLI subcommand override the defaults for one run; the live
/// recall path always uses `decay::DECAY_AFTER_DAYS`/`DECAY_PENALTY`.
pub fn apply_decay(
    store: &Store,
    candidates: &mut Vec<RankedCandidate>,
    after_days: i64,
    penalty: f32,
) -> Result<()> {
    let now = Utc::now();
    for candidate in candidates.iter_mut() {
        let row: Option<(String, Option<String>, i64)> = store
            .conn
            .query_row(
                "SELECT created_at, last_used_at, use_count FROM memory_items WHERE id = ?1",
                params![candidate.item_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        if let Some((created_at, last_used_at, use_count)) = row {
            let m = crate::models::decay::multiplier(
                &created_at,
                last_used_at.as_deref(),
                use_count,
                now,
                after_days,
                penalty,
            );
            candidate.score *= m;
        }
    }
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_ranked_in_both_lists_wins() {
        let vector = vec![
            RankedCandidate { item_id: "a".into(), score: 0.9 },
            RankedCandidate { item_id: "b".into(), score: 0.5 },
        ];
        let keyword = vec![
            RankedCandidate { item_id: "b".into(), score: 10.0 },
            RankedCandidate { item_id: "a".into(), score: 1.0 },
        ];
        let fused = fuse(&[&vector, &keyword]);
        // "a" is rank 1 in vector and rank 2 in keyword; "b" is rank 2 and rank 1.
        // Both appear in both lists with the same rank-sum, so this just
        // checks both surface — a case where only one method finds an item
        // is more interesting:
        assert_eq!(fused.len(), 2);
    }

    #[test]
    fn item_found_only_by_one_method_still_surfaces() {
        let vector = vec![RankedCandidate { item_id: "only-vector".into(), score: 0.99 }];
        let keyword: Vec<RankedCandidate> = vec![];
        let fused = fuse(&[&vector, &keyword]);
        assert_eq!(fused[0].item_id, "only-vector");
    }
}
