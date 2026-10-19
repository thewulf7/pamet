//! Shared `memory_items` read helper (T009/T012/T015 — de-duplicates the
//! identical row-mapping dual-dialect code `write_policy::active_items` and
//! `consolidation::active_unconsolidated_items` would otherwise each repeat).

use crate::error::Result;
use crate::models::{ItemType, MemoryItem};
use crate::storage::{Backend, Store};

/// Fetch every non-tombstoned `memory_items` row, with an optional extra
/// SQL predicate ANDed on (e.g. a `NOT EXISTS (...)` clause) — `extra_where`
/// must be literal SQL with no caller-supplied values, since it is spliced
/// directly into the query string.
pub(crate) fn active_items_where(store: &Store, extra_where: &str) -> Result<Vec<MemoryItem>> {
    let extra = if extra_where.is_empty() {
        String::new()
    } else {
        format!(" AND {extra_where}")
    };
    let mut out = Vec::new();
    match &store.backend {
        Backend::Sqlite(conn) => {
            let sql = format!(
                "SELECT id, item_type, content, embedding_json, entities_json, confidence, \
                        provenance_event_id, supersedes_id, created_at, last_used_at, use_count, tombstoned_at \
                 FROM memory_items WHERE tombstoned_at IS NULL{extra}"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map([], |row| {
                let embedding_json: String = row.get(3)?;
                let entities_json: Option<String> = row.get(4)?;
                Ok(MemoryItem {
                    id: row.get(0)?,
                    item_type: ItemType::from_str(&row.get::<_, String>(1)?)
                        .unwrap_or(ItemType::Fact),
                    content: row.get(2)?,
                    embedding: serde_json::from_str(&embedding_json).unwrap_or_default(),
                    entities: entities_json
                        .and_then(|j| serde_json::from_str(&j).ok())
                        .unwrap_or_default(),
                    confidence: row.get::<_, f64>(5)? as f32,
                    provenance_event_id: row.get(6)?,
                    supersedes_id: row.get(7)?,
                    created_at: row.get(8)?,
                    last_used_at: row.get(9)?,
                    use_count: row.get(10)?,
                    tombstoned_at: row.get(11)?,
                })
            })?;
            for r in rows {
                out.push(r?);
            }
        }
        #[cfg(feature = "postgres-backend")]
        Backend::Postgres(pool) => {
            let mut conn = pool.get()?;
            let sql = format!(
                "SELECT id, item_type, content, embedding, entities_json, confidence, \
                        provenance_event_id, supersedes_id, created_at, last_used_at, use_count, tombstoned_at \
                 FROM memory_items WHERE tombstoned_at IS NULL{extra}"
            );
            for row in conn.query(&sql, &[])? {
                let embedding: pgvector::Vector = row.get(3);
                let entities_json: Option<String> = row.get(4);
                out.push(MemoryItem {
                    id: row.get(0),
                    item_type: ItemType::from_str(&row.get::<_, String>(1))
                        .unwrap_or(ItemType::Fact),
                    content: row.get(2),
                    embedding: embedding.to_vec(),
                    entities: entities_json
                        .and_then(|j| serde_json::from_str(&j).ok())
                        .unwrap_or_default(),
                    confidence: row.get::<_, f32>(5),
                    provenance_event_id: row.get(6),
                    supersedes_id: row.get(7),
                    created_at: row.get(8),
                    last_used_at: row.get(9),
                    use_count: row.get::<_, i32>(10) as i64,
                    tombstoned_at: row.get(11),
                });
            }
        }
    }
    Ok(out)
}
