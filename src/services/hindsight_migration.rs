//! Hindsight → Pamet migration (feature 004, FR-001..FR-006). Reads every
//! memory from a live Hindsight bank via `baseline_hindsight::HindsightClient`
//! and writes each one into a Pamet store through the real `remember()` write
//! path (FR-002) — not a bulk insert — so the resulting corpus reflects
//! Pamet's own write-policy decisions on real data, which is the entire point
//! of the later quality/speed comparison (US3).
//!
//! Feature-gated behind `baseline-hindsight` (same gate as the HTTP client
//! this depends on).

use crate::error::{Error, Result};
use crate::services::baseline_hindsight::{HindsightClient, HindsightMemory};
use crate::{Interaction, Store};
use rusqlite::{params, Connection};
use std::path::Path;
use std::time::Instant;

const PAGE_SIZE: i64 = 200;

#[derive(Debug, Default)]
pub struct MigrationSummary {
    pub total_in_bank: i64,
    pub already_migrated: i64,
    pub skipped_empty: i64,
    pub migrated_valid: i64,
    pub migrated_tombstoned: i64,
    pub errors: i64,
}

/// A small side-SQLite ledger — deliberately separate from the target
/// Pamet store's own event-sourced schema (Key Entities: "not part of
/// Pamet's own event-sourced model") — purely so a migration run can be
/// killed and resumed (FR-003) without re-scanning Pamet's write policy
/// against already-migrated memories.
struct Ledger {
    conn: Connection,
}

impl Ledger {
    fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| Error::Validation(format!("failed to open ledger {path:?}: {e}")))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS migrated (
                hindsight_id TEXT PRIMARY KEY,
                pamet_item_ids TEXT NOT NULL,
                status TEXT NOT NULL,
                migrated_at TEXT NOT NULL
            )",
            [],
        )?;
        Ok(Self { conn })
    }

    fn already_migrated(&self, hindsight_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM migrated WHERE hindsight_id = ?1",
                params![hindsight_id],
                |_| Ok(()),
            )
            .is_ok())
    }

    fn record(&self, hindsight_id: &str, pamet_item_ids: &[String], status: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO migrated (hindsight_id, pamet_item_ids, status, migrated_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![
                hindsight_id,
                pamet_item_ids.join(","),
                status,
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }
}

/// FR-001..FR-006: migrate every memory in `bank_id` into `target`.
/// Idempotent/resumable (FR-003) via `ledger_path`; prints progress every
/// `PAGE_SIZE` memories (FR-006).
pub fn migrate_bank(
    client: &HindsightClient,
    bank_id: &str,
    target: &Store,
    ledger_path: &Path,
) -> Result<MigrationSummary> {
    let ledger = Ledger::open(ledger_path)?;
    let mut summary = MigrationSummary::default();
    let mut offset: i64 = 0;
    let start = Instant::now();

    loop {
        let (page, total) = client
            .list_memories_page(bank_id, PAGE_SIZE, offset)
            .map_err(Error::Validation)?;
        summary.total_in_bank = total;
        if page.is_empty() {
            break;
        }
        for mem in &page {
            migrate_one(mem, target, &ledger, &mut summary)?;
        }
        offset += page.len() as i64;
        println!(
            "hindsight_migration_progress processed={offset} total={total} elapsed={:.1}s",
            start.elapsed().as_secs_f64()
        );
        if offset >= total {
            break;
        }
    }

    Ok(summary)
}

fn migrate_one(
    mem: &HindsightMemory,
    target: &Store,
    ledger: &Ledger,
    summary: &mut MigrationSummary,
) -> Result<()> {
    if ledger.already_migrated(&mem.id)? {
        summary.already_migrated += 1;
        return Ok(());
    }

    let text = mem.text.trim();
    if text.is_empty() {
        summary.skipped_empty += 1;
        ledger.record(&mem.id, &[], "skipped_empty")?;
        return Ok(());
    }

    // FR-005 / Edge Cases: fact_type carried as a content prefix, same
    // pattern the existing MEMORY.md→Hindsight migration already uses.
    let content = format!("[hindsight:{}] {}", mem.fact_type, text);
    let source_id = format!(
        "hindsight:{}",
        mem.document_id.as_deref().unwrap_or("unknown")
    );

    let decisions = crate::remember(
        target,
        &Interaction {
            source_id,
            text: content,
            explicit_remember: true,
        },
    );
    let decisions = match decisions {
        Ok(d) => d,
        Err(e) => {
            summary.errors += 1;
            eprintln!("hindsight_migration_error id={} err={e}", mem.id);
            return Ok(());
        }
    };

    let item_ids: Vec<String> = decisions
        .iter()
        .filter_map(|d| d.resulting_item_id.clone())
        .collect();

    // FR-005: record Hindsight provenance against the resulting Pamet
    // item(s) as an explicit event, not just the ledger (which is
    // migration-tool-local bookkeeping, not part of Pamet's own log).
    crate::services::events::append_event(
        target,
        "hindsight_memory_migrated",
        &serde_json::json!({
            "hindsight_id": mem.id,
            "hindsight_document_id": mem.document_id,
            "hindsight_tags": mem.tags,
            "hindsight_fact_type": mem.fact_type,
            "hindsight_date": mem.date,
            "hindsight_state": mem.state,
            "pamet_item_ids": item_ids,
        }),
    )?;

    // FR-004/US2: invalidated Hindsight memories are migrated (for
    // provenance) and then immediately tombstoned, never left live.
    if mem.state == "invalidated" {
        for id in &item_ids {
            crate::tombstone(target, id)?;
        }
        summary.migrated_tombstoned += 1;
        ledger.record(&mem.id, &item_ids, "migrated_tombstoned")?;
    } else {
        summary.migrated_valid += 1;
        ledger.record(&mem.id, &item_ids, "migrated_valid")?;
    }

    Ok(())
}
