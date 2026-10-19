# Data Model: Production-Grade Storage Backend

This feature changes *where* Phase 1/2's existing entities are stored, not *what* they are.
Every table from `specs/002-consolidation-eval-harness/data-model.md` carries over unchanged in
shape (columns, meaning, rebuild rules). This document covers only what's new: the Postgres
dialect of that same schema, and the two small new concepts this feature itself introduces
(Backend Configuration, Migration Run).

## Postgres schema (dialect port of `storage/schema.rs`)

Behaviorally equivalent to the existing SQLite DDL, not byte-identical — Postgres idioms used
where they are the natural fit:

| SQLite (today) | Postgres (this feature) | Why |
|---|---|---|
| `INTEGER PRIMARY KEY AUTOINCREMENT` | `BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY` | Postgres's native identity-column idiom; same semantics (monotonic, unique, server-assigned). |
| `TEXT NOT NULL CHECK (x IN (...))` | `TEXT NOT NULL CHECK (x IN (...))` | Identical — standard SQL, no port needed. |
| `embedding_json TEXT` | `embedding vector(4096)` (pgvector) | FR-005: a real indexed vector column instead of a JSON-encoded array scanned in application code. `4096` matches the existing frozen embedder's fixed dimension (`services::embedding::DIM`) — unchanged by this feature (FR-010). |
| `memory_items_fts` (FTS5 virtual table) | `content_tsv tsvector GENERATED ALWAYS AS (to_tsvector('english', content)) STORED` + a GIN index | Postgres's native full-text search; same purpose (keyword retrieval candidate list) as the SQLite FTS5 table it replaces. |
| `INTEGER` used as boolean (0/1) | `BOOLEAN` | Postgres has a native boolean type; no behavior change, just the idiomatic column type. |
| `TEXT` for all JSON payload columns (e.g. `payload_json`, `rule_inputs_json`) | `JSONB` | Postgres's native JSON type — same logical content, queryable/indexable if ever needed, no application-level change (still serialized/deserialized via `serde_json` at the Rust boundary). |

All table and column **names** are unchanged, so every existing `MemoryItem`, `Event`,
`WriteDecision`, `ReviewQueue`, `Cluster`/`ClusterMember`, `ConsolidationRun`, `EvalResult`, and
`BaselineReport` Rust struct (from Phase 1/2) is reused as-is; only the SQL that reads/writes
them gains a second, Postgres-dialect implementation per Decision 3 in `research.md`.

**Rebuild rule, unchanged**: every row above remains populated only by replaying the `events`
table, exactly as today (Constitution Principle I) — the backend swap does not change *which*
tables are derived vs. source-of-truth, only how the derived tables are physically stored and
indexed.

## Backend Configuration (new concept, no table — a startup-time choice)

- **Represents**: which storage backend a given Pamet process is pointed at, and how to reach
  it.
- **Fields**: a single connection string/path (the existing `--db` CLI argument, now also
  accepting a `postgres://`/`postgresql://` URL in addition to its current file-path/`:memory:`
  forms).
- **Lifecycle**: resolved once at process startup (`Store::open`); not a per-call or
  per-request choice. An unreachable configured Postgres backend fails fast at startup
  (FR-007) — there is no fallback behavior to model.

## Migration Run (new concept, CLI-level — not a new persisted table)

- **Represents**: one execution of `pamet migrate --from <source> --to <target>`.
- **Mechanism** (per `research.md` Decision 4): read the complete event log from `<source>` via
  the existing `export_log()`, then replay it into a freshly-initialized `<target>` store via
  the existing `rebuild::apply_event` logic, generalized to target either backend.
- **Idempotency / re-run safety** (FR-004, Edge Cases): the target store MUST be empty before a
  migration run starts (the command refuses to run against a non-empty target rather than
  guessing how to merge) — a partially-completed migration is resolved by discarding the
  partial target and re-running from a clean target, not by resuming mid-stream. This mirrors
  how `rebuild_from_events` has always worked (always builds into a fresh store) and avoids a
  second, bespoke "resume" state machine.
- **Verification** (FR-008): after migration, the CLI prints a summary (item count, event
  count, review-queue pending/resolved counts) from both the source and the freshly-migrated
  target for the operator to diff; `pamet export-log` against the target and a subsequent
  `pamet rebuild`-equivalent check (already exists as the library's `rebuild_from_events`,
  exercised by the existing test suite) gives the same replay-equivalence guarantee Phase 1/2
  already requires, now also proving the cross-backend copy was faithful.
