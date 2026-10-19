//! Vector similarity search over embeddings stored in `memory_items` (T023).
//! Deviation note (see embedding.rs): SQLite computes cosine similarity in
//! Rust over stored JSON float vectors (no `sqlite-vec` extension loaded);
//! Postgres pushes the distance computation into SQL via pgvector's native
//! `<=>` cosine-distance operator (FR-005) instead of pulling every row's
//! full vector over the wire and scoring in Rust — the latter was the
//! original T023 implementation and, measured against a real 10,000-item
//! store, missed SC-003's <100ms target by ~5x (490ms), mostly from network
//! transfer of ~40MB of embeddings per query, not from the scoring math
//! itself.

use crate::error::Result;
use crate::models::RankedCandidate;
use crate::services::embedding;
use crate::storage::{Backend, Store};

/// Minimum cosine similarity to be considered a candidate at all (FR-008 —
/// an irrelevant item must not be force-ranked just because it exists).
const MIN_SIMILARITY: f32 = 0.15;

pub fn search(store: &Store, query: &str, limit: usize) -> Result<Vec<RankedCandidate>> {
    let query_vec = embedding::embed(query);
    let mut scored: Vec<(String, f32)> = Vec::new();
    match &store.backend {
        Backend::Sqlite(conn) => {
            let mut stmt = conn.prepare(
                "SELECT id, embedding_json FROM memory_items WHERE tombstoned_at IS NULL",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for r in rows {
                let (id, embedding_json) = r?;
                let item_vec: Vec<f32> = serde_json::from_str(&embedding_json).unwrap_or_default();
                let score = embedding::cosine_similarity(&query_vec, &item_vec);
                if score >= MIN_SIMILARITY {
                    scored.push((id, score));
                }
            }
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            let vector = pgvector::Vector::from(query_vec);
            // `<=>` is pgvector's cosine-distance operator; similarity =
            // 1 - distance. Filtering and ordering happen in SQL (backed by
            // the ivfflat index on this column, schema.rs) so only `limit`
            // rows' worth of candidates ever cross the wire, not every
            // item's full embedding.
            let max_distance = 1.0 - MIN_SIMILARITY;
            for row in conn.query(
                "SELECT id, 1 - (embedding <=> $1) AS similarity FROM memory_items \
                 WHERE tombstoned_at IS NULL AND embedding <=> $1 <= $2 \
                 ORDER BY embedding <=> $1 LIMIT $3",
                &[&vector, &(max_distance as f64), &(limit as i64)],
            )? {
                let id: String = row.get(0);
                let similarity: f64 = row.get(1);
                scored.push((id, similarity as f32));
            }
        }
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(limit);
    Ok(scored
        .into_iter()
        .map(|(item_id, score)| RankedCandidate { item_id, score })
        .collect())
}
