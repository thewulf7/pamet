//! Consolidation job (research.md §1-4, FR-001..FR-004, FR-010). T014-T018.
//!
//! Rule-based — no learned component (Constitution Principle III). Runs as
//! its own entry point over a read of the event log; never invoked from
//! `remember()`/`recall()` (Principle IV, Two-Speed Memory Separation) —
//! callers (the CLI's `consolidate` subcommand) own the scheduling.

use crate::error::Result;
use crate::models::{Cluster, ItemType, MemoryItem, ReviewReason};
use crate::services::{embedding, events, write_policy};
use crate::storage::Store;
use chrono::Utc;
use rusqlite::params;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConsolidationSummary {
    pub run_id: String,
    pub items_processed: i64,
    pub clusters_formed: i64,
    pub contradictions_flagged: i64,
}

/// Run one consolidation pass: cluster near-duplicates into summaries, flag
/// unresolved contradictions for review, record the run. Safe to call
/// repeatedly (FR-010) — already-consolidated items are excluded from future
/// clustering, so a re-run over an unchanged store forms zero new clusters.
pub fn run(store: &Store) -> Result<ConsolidationSummary> {
    interrupt_stale_runs(store)?;

    let run_id = Uuid::new_v4().to_string();
    let started_at = Utc::now().to_rfc3339();
    events::append_event(
        store,
        "consolidation_run_started",
        &serde_json::json!({"run_id": run_id, "started_at": started_at}),
    )?;
    store.conn.execute(
        "INSERT INTO consolidation_runs (id, started_at, status) VALUES (?1, ?2, 'running')",
        params![run_id, started_at],
    )?;

    let items = active_unconsolidated_items(store)?;
    let items_processed = items.len() as i64;

    let clusters = cluster_items(&items);
    for cluster in &clusters {
        form_summary(store, &run_id, cluster, &items)?;
    }
    let clusters_formed = clusters.len() as i64;

    let contradictions_flagged = flag_contradictions(store, &items, &clusters)?;

    let finished_at = Utc::now().to_rfc3339();
    events::append_event(
        store,
        "consolidation_run_finished",
        &serde_json::json!({
            "run_id": run_id, "finished_at": finished_at, "items_processed": items_processed,
            "clusters_formed": clusters_formed, "contradictions_flagged": contradictions_flagged,
            "status": "completed",
        }),
    )?;
    store.conn.execute(
        "UPDATE consolidation_runs SET finished_at = ?1, items_processed = ?2, clusters_formed = ?3, \
            contradictions_flagged = ?4, status = 'completed' WHERE id = ?5",
        params![finished_at, items_processed, clusters_formed, contradictions_flagged, run_id],
    )?;

    Ok(ConsolidationSummary { run_id, items_processed, clusters_formed, contradictions_flagged })
}

/// Any run left `running` by a killed/crashed previous process is marked
/// `interrupted` via its own event (FR-010; every state change stays
/// event-sourced, Principle I) before a new run starts.
fn interrupt_stale_runs(store: &Store) -> Result<()> {
    let stale: Vec<(String, i64, i64, i64)> = {
        let mut stmt = store.conn.prepare(
            "SELECT id, items_processed, clusters_formed, contradictions_flagged \
             FROM consolidation_runs WHERE status = 'running'",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        out
    };
    for (run_id, items_processed, clusters_formed, contradictions_flagged) in stale {
        let finished_at = Utc::now().to_rfc3339();
        events::append_event(
            store,
            "consolidation_run_finished",
            &serde_json::json!({
                "run_id": run_id, "finished_at": finished_at, "items_processed": items_processed,
                "clusters_formed": clusters_formed, "contradictions_flagged": contradictions_flagged,
                "status": "interrupted",
            }),
        )?;
        store.conn.execute(
            "UPDATE consolidation_runs SET finished_at = ?1, status = 'interrupted' WHERE id = ?2",
            params![finished_at, run_id],
        )?;
    }
    Ok(())
}

/// Active items not yet folded into any cluster (data-model.md: once an
/// item is a `cluster_members.source_item_id`, it's permanently excluded
/// from future clustering — this is what makes re-running `consolidate`
/// idempotent, FR-010).
fn active_unconsolidated_items(store: &Store) -> Result<Vec<MemoryItem>> {
    let mut stmt = store.conn.prepare(
        "SELECT m.id, m.item_type, m.content, m.embedding_json, m.entities_json, m.confidence, \
                m.provenance_event_id, m.supersedes_id, m.created_at, m.last_used_at, m.use_count, m.tombstoned_at \
         FROM memory_items m \
         WHERE m.tombstoned_at IS NULL \
           AND NOT EXISTS (SELECT 1 FROM cluster_members cm WHERE cm.source_item_id = m.id)",
    )?;
    let mut out = Vec::new();
    let rows = stmt.query_map([], |row| {
        let embedding_json: String = row.get(3)?;
        let entities_json: Option<String> = row.get(4)?;
        Ok((
            row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
            embedding_json, entities_json, row.get::<_, f64>(5)?, row.get::<_, i64>(6)?,
            row.get::<_, Option<String>>(7)?, row.get::<_, String>(8)?,
            row.get::<_, Option<String>>(9)?, row.get::<_, i64>(10)?,
            row.get::<_, Option<String>>(11)?,
        ))
    })?;
    for r in rows {
        let (id, item_type, content, embedding_json, entities_json, confidence,
            provenance_event_id, supersedes_id, created_at, last_used_at, use_count, tombstoned_at) = r?;
        out.push(MemoryItem {
            id,
            item_type: ItemType::from_str(&item_type).unwrap_or(ItemType::Fact),
            content,
            embedding: serde_json::from_str(&embedding_json).unwrap_or_default(),
            entities: entities_json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default(),
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

/// Threshold-graph clustering (research.md §1, corrected after testing):
/// connects any pair whose cosine similarity clears `LOW_CONFIDENCE_THRESHOLD`
/// (the floor of write-time policy's "maybe the same thing" ambiguous band).
/// NOT `MERGE_SIMILARITY_THRESHOLD` — anything that clears that threshold is
/// already merged into a single row at write time (`write_policy::evaluate`
/// always re-scans active items before storing), so two coexisting active
/// items can never be >= `MERGE_SIMILARITY_THRESHOLD` similar to each other.
/// The items consolidation needs to catch are the ones write-time policy
/// deliberately left as separate rows — explicit-remember writes landing in
/// the 0.5-0.85 ambiguous band bypass the review gate and are stored verbatim
/// (write_policy.rs `evaluate`, `!candidate.explicit_remember` guard).
fn cluster_items(items: &[MemoryItem]) -> Vec<Cluster> {
    let n = items.len();
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }

    for i in 0..n {
        for j in (i + 1)..n {
            let sim = embedding::cosine_similarity(&items[i].embedding, &items[j].embedding);
            if sim >= write_policy::LOW_CONFIDENCE_THRESHOLD {
                union(&mut parent, i, j);
            }
        }
    }

    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    groups
        .into_values()
        .filter(|g| g.len() >= 2)
        .map(|g| Cluster {
            cluster_id: Uuid::new_v4().to_string(),
            member_item_ids: g.iter().map(|&idx| items[idx].id.clone()).collect(),
        })
        .collect()
}

/// Summary generation (research.md §2): deterministic "longest content
/// wins", not an LLM summary — a rule-based predecessor for Principle III.
fn form_summary(store: &Store, run_id: &str, cluster: &Cluster, items: &[MemoryItem]) -> Result<()> {
    let members: Vec<&MemoryItem> =
        cluster.member_item_ids.iter().filter_map(|id| items.iter().find(|i| &i.id == id)).collect();
    let chosen = members
        .iter()
        .max_by(|a, b| a.content.len().cmp(&b.content.len()).then_with(|| b.created_at.cmp(&a.created_at)))
        .expect("cluster must have at least 2 members");

    let summary_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    events::append_event(
        store,
        "cluster_formed",
        &serde_json::json!({
            "run_id": run_id, "cluster_id": cluster.cluster_id, "member_item_ids": cluster.member_item_ids,
        }),
    )?;

    let summary_event_id = events::append_event(
        store,
        "summary_created",
        &serde_json::json!({
            "run_id": run_id, "cluster_id": cluster.cluster_id, "summary_item_id": summary_id,
            "summary_content": chosen.content, "member_item_ids": cluster.member_item_ids,
        }),
    )?;

    let embedding = embedding::embed(&chosen.content);
    store.conn.execute(
        "INSERT INTO memory_items (id, item_type, content, embedding_json, entities_json, confidence, \
            provenance_event_id, supersedes_id, created_at, use_count) \
         VALUES (?1, 'summary', ?2, ?3, '[]', ?4, ?5, NULL, ?6, 0)",
        params![summary_id, chosen.content, serde_json::to_string(&embedding)?, chosen.confidence as f64, summary_event_id, now],
    )?;
    store.conn.execute(
        "INSERT INTO memory_items_fts (id, content) VALUES (?1, ?2)",
        params![summary_id, chosen.content],
    )?;

    for member_id in &cluster.member_item_ids {
        store.conn.execute(
            "INSERT INTO cluster_members (id, cluster_id, summary_item_id, source_item_id, consolidation_run_id, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![Uuid::new_v4().to_string(), cluster.cluster_id, summary_id, member_id, run_id, now],
        )?;
    }

    // Constitution II: "Low-confidence writes...MUST be flagged for human
    // review before being promoted into consolidated knowledge." Every
    // cluster consolidation forms is, by construction, in the ambiguous
    // 0.5-0.85 band (anything >= 0.85 already merged at write time — see
    // `cluster_items()` docs) — so the new summary is itself a low-
    // confidence promotion. It's created now (satisfying the spec's
    // Acceptance Scenario 1: "clustered into one consolidated item...
    // recoverable via the event log") but reuses the EXISTING
    // `pending_review_candidate_ids` filter in `lib.rs::recall()` to stay
    // invisible to retrieval until a human resolves this review entry —
    // the same "write_decision + review_queue, excluded from recall while
    // pending" mechanism Phase 1 already uses for ambiguous writes, so
    // nothing new is invented. Resolve via `pamet review resolve <id>
    // --decision store` (accept) or `--decision skip` (reject).
    write_review_gate(store, &summary_id, &chosen.content, summary_event_id, &now)?;

    Ok(())
}

/// Source items whose cluster's summary has been confirmed (non-pending,
/// non-rejected review) — T042/SC-005, see `lib.rs::recall()`.
pub fn confirmed_cluster_source_ids(store: &Store) -> Result<std::collections::HashSet<String>> {
    let mut stmt = store.conn.prepare(
        "SELECT cm.source_item_id FROM cluster_members cm \
         JOIN write_decisions wd ON wd.resulting_item_id = cm.summary_item_id \
         JOIN review_queue rq ON rq.write_decision_id = wd.id \
         WHERE rq.status NOT IN ('pending', 'resolved_skip')",
    )?;
    let mut out = std::collections::HashSet::new();
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for r in rows {
        out.insert(r?);
    }
    Ok(out)
}

fn write_review_gate(
    store: &Store,
    summary_id: &str,
    summary_content: &str,
    summary_event_id: i64,
    now: &str,
) -> Result<()> {
    store.conn.execute(
        "INSERT INTO write_decisions (event_id, candidate_content, decision, rule_name, \
            rule_inputs_json, resulting_item_id, created_at) VALUES (?1, ?2, 'review', \
            'consolidation_cluster_review', '{}', ?3, ?4)",
        params![summary_event_id, summary_content, summary_id, now],
    )?;
    let write_decision_id = store.conn.last_insert_rowid();
    store.conn.execute(
        "INSERT INTO review_queue (write_decision_id, reason, status, created_at) VALUES (?1, ?2, 'pending', ?3)",
        params![write_decision_id, ReviewReason::ConsolidationClusterPromotion.as_str(), now],
    )?;
    Ok(())
}

/// Contradiction flagging during consolidation (research.md §4): reuses the
/// exact write-time heuristic, applied pairwise among items that did NOT
/// cluster together, within the existing "maybe the same thing" similarity
/// band. Writes to the existing review queue — never auto-resolved.
fn flag_contradictions(store: &Store, items: &[MemoryItem], clusters: &[Cluster]) -> Result<i64> {
    let clustered: std::collections::HashSet<&str> =
        clusters.iter().flat_map(|c| c.member_item_ids.iter().map(|s| s.as_str())).collect();

    let mut flagged = 0i64;
    let mut seen_pairs: std::collections::HashSet<(String, String)> = Default::default();

    for i in 0..items.len() {
        if clustered.contains(items[i].id.as_str()) {
            continue;
        }
        for j in (i + 1)..items.len() {
            if clustered.contains(items[j].id.as_str()) {
                continue;
            }
            let sim = embedding::cosine_similarity(&items[i].embedding, &items[j].embedding);
            if sim < write_policy::CONTRADICTION_SIMILARITY_FLOOR || sim >= write_policy::MERGE_SIMILARITY_THRESHOLD {
                continue;
            }
            let candidate = write_policy::Candidate {
                content: items[j].content.clone(),
                item_type: items[j].item_type.clone(),
                explicit_remember: false,
            };
            if write_policy::detect_contradiction(&candidate, Some(&items[i])) {
                let mut pair = [items[i].id.clone(), items[j].id.clone()];
                pair.sort();
                let pair_key = (pair[0].clone(), pair[1].clone());
                if seen_pairs.insert(pair_key) && flag_one_contradiction(store, &items[i], &items[j])? {
                    flagged += 1;
                }
            }
        }
    }
    Ok(flagged)
}

/// Anchor the review entry on whichever of the two conflicting items has a
/// discoverable originating `write_decisions` row (every real item has one;
/// this is defensive for edge cases like replayed/synthetic data).
fn flag_one_contradiction(store: &Store, a: &MemoryItem, b: &MemoryItem) -> Result<bool> {
    let (newer, older) = if a.created_at >= b.created_at { (a, b) } else { (b, a) };
    let write_decision_id: Option<i64> = store
        .conn
        .query_row(
            "SELECT id FROM write_decisions WHERE resulting_item_id = ?1 ORDER BY id DESC LIMIT 1",
            params![newer.id],
            |r| r.get(0),
        )
        .ok()
        .or_else(|| {
            store
                .conn
                .query_row(
                    "SELECT id FROM write_decisions WHERE resulting_item_id = ?1 ORDER BY id DESC LIMIT 1",
                    params![older.id],
                    |r| r.get(0),
                )
                .ok()
        });
    let Some(write_decision_id) = write_decision_id else {
        return Ok(false);
    };
    let now = Utc::now().to_rfc3339();
    store.conn.execute(
        "INSERT INTO review_queue (write_decision_id, reason, status, created_at) VALUES (?1, ?2, 'pending', ?3)",
        params![write_decision_id, ReviewReason::ConsolidationContradiction.as_str(), now],
    )?;
    Ok(true)
}
