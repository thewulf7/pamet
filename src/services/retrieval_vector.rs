//! Vector similarity search over embeddings stored in `memory_items` (T023).
//! Deviation note (see embedding.rs): cosine similarity computed in Rust
//! over stored JSON float vectors, instead of the `sqlite-vec` extension —
//! same ranked-candidate contract, no loadable C extension required.

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
            // T023 deviation note: Rust-side cosine scan for behavioral parity
            // with the SQLite path, not pgvector's native `<=>` operator — the
            // `vector` column and its index exist for a later optimization
            // pass, scored identically on both backends for now (plan.md).
            for row in conn.query(
                "SELECT id, embedding FROM memory_items WHERE tombstoned_at IS NULL",
                &[],
            )? {
                let id: String = row.get(0);
                let embedding: pgvector::Vector = row.get(1);
                let item_vec = embedding.to_vec();
                let score = embedding::cosine_similarity(&query_vec, &item_vec);
                if score >= MIN_SIMILARITY {
                    scored.push((id, score));
                }
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
