//! Vector similarity search over embeddings stored in `memory_items_vec`
//! (T009/T034, feature 009). Replaces the SQLite brute-force full-table scan
//! over JSON-blob embeddings (the original T023 implementation, which
//! missed SC-003's <100ms target by ~5x against a real 10,000-item store)
//! with `sqlite-vec`'s native `vec0` KNN query — the whole reason this
//! feature adopted `sqlite-vec` (research.md Decision 1, plan.md). Postgres
//! keeps its existing pgvector `<=>` path (FR-005) — no change there.

use crate::error::Result;
use crate::models::RankedCandidate;
use crate::services::embedding;
use crate::storage::{Backend, Store};
use zerocopy::IntoBytes;

/// Minimum cosine similarity to be considered a candidate at all (FR-008 —
/// an irrelevant item must not be force-ranked just because it exists).
const MIN_SIMILARITY: f32 = 0.15;

pub fn search(store: &Store, query: &str, limit: usize) -> Result<Vec<RankedCandidate>> {
    let query_vec = embedding::embed(query)?;
    let mut scored: Vec<(String, f32)> = Vec::new();
    match &store.backend {
        Backend::Sqlite(conn) => {
            // `vec0`'s `distance_metric=cosine` (schema.rs) makes `MATCH`'s
            // `distance` output a cosine *distance* (0 = identical, 2 = polar
            // opposite) — similarity = 1 - distance, same convention the
            // Postgres/pgvector branch below already uses for `<=>`.
            // Over-fetch a wider k than `limit` so post-filtering on
            // MIN_SIMILARITY and the `tombstoned_at IS NULL` join still
            // leaves `limit` worth of real candidates, not fewer.
            let k = (limit * 4).max(50);
            let mut stmt = conn.prepare(
                "SELECT m.id, 1.0 - v.distance AS similarity \
                 FROM memory_items_vec v \
                 JOIN memory_items m ON m.id = v.item_id \
                 WHERE v.embedding MATCH ?1 AND k = ?2 AND m.tombstoned_at IS NULL \
                 ORDER BY v.distance",
            )?;
            let query_bytes = query_vec.as_bytes();
            let rows = stmt.query_map(rusqlite::params![query_bytes, k as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?;
            for r in rows {
                let (id, similarity) = r?;
                let score = similarity as f32;
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
