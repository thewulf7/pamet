//! Deterministic per-cluster n-gram (trigram) model (feature 008,
//! specs/008-deterministic-gram-model).
//!
//! A frozen, statistical frequency table built from a consolidated cluster's
//! member texts — no gradients, no training objective, no external model
//! call. Structurally the same kind of "rule" this project's BM25 keyword
//! index already is, not the "small LM" Constitution Principle III gates on
//! (see plan.md's Constitution Check for the full argument). Built once per
//! consolidation run that touches a cluster (two-speed separation, Principle
//! IV — never on the live write/read path), persisted with provenance back to
//! that run (Principles I/II), and fully reconstructable by replaying the
//! event log alone (`rebuild.rs`).

use crate::error::{Error, Result};
use crate::services::embedding;
use crate::services::events;
use crate::storage::{Backend, Store};
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Fixed gram order for v1 (research.md Decision 1) — trigram: two tokens of
/// context predict the third. Not configurable per cluster; a second free
/// parameter before any generated output exists to justify one is premature
/// (spec.md Assumptions).
pub const GRAM_ORDER: usize = 3;

/// NUL separator joining the two context tokens into one map key — safe
/// because `embedding::tokenize()` only ever emits alphanumeric-lowercase
/// tokens, which cannot contain a NUL byte.
const KEY_SEP: char = '\u{0}';

/// A persisted, frozen trigram model for one consolidated cluster
/// (data-model.md `ClusterGramModel`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterGramModel {
    pub cluster_id: String,
    pub consolidation_run_id: String,
    pub gram_order: usize,
    pub vocab_size: usize,
    pub training_token_count: usize,
    /// `"tok_a\0tok_b" -> {tok_c: count}`.
    pub table: HashMap<String, HashMap<String, u32>>,
    pub created_at: String,
}

fn context_key(a: &str, b: &str) -> String {
    format!("{a}{KEY_SEP}{b}")
}

/// T010/US1: derive a frozen trigram model from a cluster's member texts.
/// Returns `None` if the combined token count is below `GRAM_ORDER` — too
/// short to be meaningful (FR-003/Edge Case), not an error.
pub fn build_for_cluster(
    cluster_id: &str,
    consolidation_run_id: &str,
    member_texts: &[String],
) -> Option<ClusterGramModel> {
    let combined = member_texts.join(" ");
    let tokens = embedding::tokenize(&combined);
    if tokens.len() < GRAM_ORDER {
        return None;
    }

    let mut table: HashMap<String, HashMap<String, u32>> = HashMap::new();
    for window in tokens.windows(GRAM_ORDER) {
        let key = context_key(&window[0], &window[1]);
        *table
            .entry(key)
            .or_default()
            .entry(window[2].clone())
            .or_insert(0) += 1;
    }

    let vocab_size = tokens
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();

    Some(ClusterGramModel {
        cluster_id: cluster_id.to_string(),
        consolidation_run_id: consolidation_run_id.to_string(),
        gram_order: GRAM_ORDER,
        vocab_size,
        training_token_count: tokens.len(),
        table,
        created_at: Utc::now().to_rfc3339(),
    })
}

/// T007: persist a model, logging the event first (Constitution Principle
/// I). The full model (including `table_json`) is serialized into the event
/// payload so `rebuild.rs` can re-insert the row verbatim on replay, rather
/// than recomputing it — what makes replay byte-for-byte, not just
/// same-shape (FR-008).
///
/// `ON CONFLICT ... DO NOTHING`, not an update: a `cluster_id` is minted
/// fresh exactly once by `consolidation::cluster_items()` and its membership
/// never changes afterward (an item is permanently excluded from future
/// clustering the moment it joins a cluster — `active_unconsolidated_items`).
/// A second `persist()` call for the same `cluster_id` can therefore only
/// ever be a harmless re-run over the identical input, never a genuine
/// membership change to apply — FR-004's "unchanged cluster, no rebuild" is
/// satisfied by this existing architectural invariant, not by a separate
/// membership-diff check (discovered during implementation; see spec.md
/// Results).
pub fn persist(store: &Store, model: &ClusterGramModel) -> Result<()> {
    let table_json = serde_json::to_string(&model.table)?;

    events::append_event(
        store,
        "cluster_gram_model_built",
        &serde_json::json!({
            "cluster_id": model.cluster_id,
            "consolidation_run_id": model.consolidation_run_id,
            "gram_order": model.gram_order,
            "vocab_size": model.vocab_size,
            "training_token_count": model.training_token_count,
            "table_json": table_json,
            "created_at": model.created_at,
        }),
    )?;

    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "INSERT INTO cluster_gram_models \
                    (cluster_id, consolidation_run_id, gram_order, vocab_size, training_token_count, table_json, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                 ON CONFLICT(cluster_id) DO NOTHING",
                params![
                    model.cluster_id,
                    model.consolidation_run_id,
                    model.gram_order as i64,
                    model.vocab_size as i64,
                    model.training_token_count as i64,
                    table_json,
                    model.created_at,
                ],
            )?;
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.execute(
                "INSERT INTO cluster_gram_models \
                    (cluster_id, consolidation_run_id, gram_order, vocab_size, training_token_count, table_json, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7) \
                 ON CONFLICT(cluster_id) DO NOTHING",
                &[
                    &model.cluster_id,
                    &model.consolidation_run_id,
                    &(model.gram_order as i32),
                    &(model.vocab_size as i32),
                    &(model.training_token_count as i32),
                    &table_json,
                    &model.created_at,
                ],
            )?;
        }
    }
    Ok(())
}

/// Insert a row verbatim at replay time (rebuild.rs, T022) — no event
/// emitted (the event already exists in the log being replayed) and no
/// `ON CONFLICT` guard needed (a fresh target store starts empty).
pub(crate) fn insert_from_replay(
    store: &Store,
    model: &ClusterGramModel,
    table_json: &str,
) -> Result<()> {
    match &store.backend {
        Backend::Sqlite(conn) => {
            conn.execute(
                "INSERT INTO cluster_gram_models \
                    (cluster_id, consolidation_run_id, gram_order, vocab_size, training_token_count, table_json, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    model.cluster_id,
                    model.consolidation_run_id,
                    model.gram_order as i64,
                    model.vocab_size as i64,
                    model.training_token_count as i64,
                    table_json,
                    model.created_at,
                ],
            )?;
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.execute(
                "INSERT INTO cluster_gram_models \
                    (cluster_id, consolidation_run_id, gram_order, vocab_size, training_token_count, table_json, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &model.cluster_id,
                    &model.consolidation_run_id,
                    &(model.gram_order as i32),
                    &(model.vocab_size as i32),
                    &(model.training_token_count as i32),
                    &table_json,
                    &model.created_at,
                ],
            )?;
        }
    }
    Ok(())
}

/// T007: load a persisted model, or `None` if the cluster has none — not an
/// error at this layer (FR-009's "specific error" is a CLI-layer concern).
pub fn load(store: &Store, cluster_id: &str) -> Result<Option<ClusterGramModel>> {
    let row: Option<(String, i64, i64, i64, String, String)> = match &store.backend {
        Backend::Sqlite(conn) => conn
            .query_row(
                "SELECT consolidation_run_id, gram_order, vocab_size, training_token_count, table_json, created_at \
                 FROM cluster_gram_models WHERE cluster_id = ?1",
                params![cluster_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .ok(),
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            conn.query_opt(
                "SELECT consolidation_run_id, gram_order, vocab_size, training_token_count, table_json, created_at \
                 FROM cluster_gram_models WHERE cluster_id = $1",
                &[&cluster_id],
            )?
            .map(|r| {
                (
                    r.get(0),
                    r.get::<_, i32>(1) as i64,
                    r.get::<_, i32>(2) as i64,
                    r.get::<_, i32>(3) as i64,
                    r.get(4),
                    r.get(5),
                )
            })
        }
    };

    let Some((
        consolidation_run_id,
        gram_order,
        vocab_size,
        training_token_count,
        table_json,
        created_at,
    )) = row
    else {
        return Ok(None);
    };
    let table: HashMap<String, HashMap<String, u32>> = serde_json::from_str(&table_json)?;
    Ok(Some(ClusterGramModel {
        cluster_id: cluster_id.to_string(),
        consolidation_run_id,
        gram_order: gram_order as usize,
        vocab_size: vocab_size as usize,
        training_token_count: training_token_count as usize,
        table,
        created_at,
    }))
}

/// T017/US2: hand-written deterministic PRNG (splitmix64, research.md
/// Decision 3) — no `rand` crate dependency, so this crate's own source is
/// the only thing that can ever change the output sequence for a given seed
/// (same reasoning `embedding.rs` already applies to its FNV-1a hash).
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Uniform index in `[0, n)`. `n` is always small (a context's observed
    /// next-token count) so the modulo-bias this introduces is negligible
    /// and not worth a rejection-sampling loop.
    fn next_index(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// T018/US2: sample from a persisted model. Seed resolution: an explicit
/// seed makes output byte-identical across runs (FR-006/SC-002); omitted,
/// a time-derived seed is used and reproducibility is not guaranteed — the
/// contract (contracts/cli-contract.md) is explicit that determinism is
/// conditioned on *supplying* `--rng-seed`.
///
/// Stops the moment the current two-token context has no observed
/// continuation (research.md Decision 4) — never falls back to a
/// lower-order model, so every emitted token is provably drawn from the
/// cluster's own trigram table (SC-003), never fabricated.
pub fn generate(model: &ClusterGramModel, seed: Option<u64>, max_tokens: Option<usize>) -> String {
    let seed = seed.unwrap_or_else(|| Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64);
    let mut rng = SplitMix64::new(seed);
    let cap = max_tokens.unwrap_or(50);

    // Deterministic start state: the lexicographically-first context key
    // (stable given the same model, independent of HashMap iteration order,
    // which Rust does not guarantee is stable across runs).
    let Some(start_key) = model.table.keys().min() else {
        return String::new();
    };
    let mut parts: Vec<&str> = start_key.split(KEY_SEP).collect();
    let mut output: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
    let mut context = context_key(parts[0], parts[1]);

    while output.len() < cap {
        let Some(continuations) = model.table.get(&context) else {
            break;
        };
        // Deterministic ordering before sampling: a HashMap's iteration
        // order is not part of its contract, so without sorting first, the
        // *input* to the PRNG draw (which index corresponds to which
        // continuation) could silently vary run-to-run even with the same
        // seed and the same logical table (FR-006).
        let mut options: Vec<(&String, &u32)> = continuations.iter().collect();
        options.sort_by(|a, b| a.0.cmp(b.0));
        let total: u32 = options.iter().map(|(_, c)| **c).sum();
        let mut roll = rng.next_index(total as usize) as u32;
        let mut chosen = options[0].0;
        for (tok, count) in &options {
            if roll < **count {
                chosen = tok;
                break;
            }
            roll -= **count;
        }
        output.push(chosen.clone());
        let prev = output[output.len() - 2].clone();
        context = context_key(&prev, chosen);
    }

    parts.clear();
    output.join(" ")
}

/// T023/US3: stats for `pamet gram show` — no generation, no PRNG involved.
pub fn describe(model: &ClusterGramModel) -> String {
    format!(
        "gram_order={} vocab_size={} training_tokens={} consolidation_run_id={} created_at={}",
        model.gram_order,
        model.vocab_size,
        model.training_token_count,
        model.consolidation_run_id,
        model.created_at
    )
}

/// FR-009: specific, actionable error for a missing model — never silent
/// empty output at the CLI layer.
pub fn no_model_error(cluster_id: &str) -> Error {
    Error::Validation(format!(
        "no gram model for cluster {cluster_id} — run 'pamet consolidate' first"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_same_seed_same_sequence() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        for _ in 0..20 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn splitmix64_different_seeds_diverge() {
        let mut a = SplitMix64::new(1);
        let mut b = SplitMix64::new(2);
        let seq_a: Vec<u64> = (0..10).map(|_| a.next_u64()).collect();
        let seq_b: Vec<u64> = (0..10).map(|_| b.next_u64()).collect();
        assert_ne!(seq_a, seq_b);
    }

    #[test]
    fn splitmix64_next_index_stays_in_bounds() {
        let mut rng = SplitMix64::new(7);
        for _ in 0..100 {
            let idx = rng.next_index(5);
            assert!(idx < 5);
        }
    }
}
