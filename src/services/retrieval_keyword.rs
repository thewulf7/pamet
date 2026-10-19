//! Keyword/lexical search via SQLite FTS5 with its built-in BM25 ranking
//! (T024), per research.md "Keyword matching" decision.

use crate::error::Result;
use crate::models::RankedCandidate;
use crate::storage::{Backend, Store};

pub fn search(store: &Store, query: &str, limit: usize) -> Result<Vec<RankedCandidate>> {
    let sanitized = sanitize_fts_query(query);
    if sanitized.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    match &store.backend {
        Backend::Sqlite(conn) => {
            // FTS5 `bm25()` returns lower-is-better; we negate to make
            // higher-is-better consistent with the vector search scores for
            // fusion. Joined against memory_items to exclude tombstoned rows
            // (FR-014 / data-model.md: a tombstoned item MUST NOT be
            // returned by retrieval).
            let mut stmt = conn.prepare(
                "SELECT f.id, bm25(memory_items_fts) AS rank FROM memory_items_fts f \
                 JOIN memory_items m ON m.id = f.id \
                 WHERE memory_items_fts MATCH ?1 AND m.tombstoned_at IS NULL ORDER BY rank LIMIT ?2",
            )?;
            let rows = stmt.query_map(rusqlite::params![sanitized, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            });
            let rows = match rows {
                Ok(r) => r,
                Err(_) => return Ok(Vec::new()),
            };
            for r in rows {
                let (item_id, bm25_rank) = r?;
                out.push(RankedCandidate {
                    item_id,
                    score: -bm25_rank as f32,
                });
            }
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            let tsquery = sanitize_tsquery(query);
            if tsquery.is_empty() {
                return Ok(Vec::new());
            }
            let rows = conn.query(
                "SELECT id, ts_rank(content_tsv, to_tsquery('english', $1)) AS rank \
                 FROM memory_items \
                 WHERE content_tsv @@ to_tsquery('english', $1) AND tombstoned_at IS NULL \
                 ORDER BY rank DESC LIMIT $2",
                &[&tsquery, &(limit as i64)],
            );
            let rows = match rows {
                Ok(r) => r,
                Err(_) => return Ok(Vec::new()),
            };
            for row in rows {
                out.push(RankedCandidate {
                    item_id: row.get(0),
                    score: row.get::<_, f32>(1),
                });
            }
        }
    }
    Ok(out)
}

/// FTS5 MATCH has its own query syntax; for an MVP we OR together the
/// tokenized query words so any overlap counts as a candidate.
fn sanitize_fts_query(query: &str) -> String {
    let tokens: Vec<String> = crate::services::embedding::tokenize(query);
    tokens
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Same OR-of-tokens shape as `sanitize_fts_query`, in Postgres `tsquery`
/// syntax (`|` is OR; each token quoted so punctuation can't break parsing).
#[cfg(feature = "postgres-backend")]
fn sanitize_tsquery(query: &str) -> String {
    let tokens: Vec<String> = crate::services::embedding::tokenize(query);
    tokens
        .iter()
        .filter(|t| !t.is_empty())
        .map(|t| format!("'{}'", t.replace('\'', "")))
        .collect::<Vec<_>>()
        .join(" | ")
}
