//! SQLite schema for the event-sourced memory store (data-model.md). T004.

pub const SQLITE_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS events (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at   TEXT    NOT NULL,
    event_type    TEXT    NOT NULL CHECK (event_type IN
                      ('write_decision','retrieval','outcome_update','review_resolution','tombstone',
                       'cluster_formed','summary_created','consolidation_run_started',
                       'consolidation_run_finished','eval_run_completed','item_decayed',
                       'hindsight_memory_migrated','cluster_gram_model_built','embedding_backfilled')),
    payload_json  TEXT    NOT NULL,
    schema_version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS memory_items (
    id                   TEXT PRIMARY KEY,
    item_type            TEXT NOT NULL CHECK (item_type IN ('episode','fact','preference','procedure','summary')),
    content              TEXT NOT NULL,
    embedding_json        TEXT,
    embedding_model_id   TEXT REFERENCES embedding_models(id),
    entities_json        TEXT,
    confidence           REAL NOT NULL DEFAULT 1.0,
    provenance_event_id  INTEGER NOT NULL REFERENCES events(id),
    supersedes_id        TEXT REFERENCES memory_items(id),
    created_at           TEXT NOT NULL,
    last_used_at         TEXT,
    use_count            INTEGER NOT NULL DEFAULT 0,
    tombstoned_at        TEXT
);

CREATE VIRTUAL TABLE IF NOT EXISTS memory_items_fts USING fts5(
    id UNINDEXED,
    content
);

-- Feature 009: real semantic embedding (data-model.md). `embedding_models`
-- tracks which frozen model/version produced which items' vectors (FR-009);
-- `memory_items_vec` is a `vec0` virtual table (sqlite-vec) holding the real
-- vector itself, keyed by `memory_items.id` — not FK-enforced via SQL (vec0
-- virtual tables don't support ordinary FK constraints), enforced at the
-- application layer like `cluster_members`'s references already are.
CREATE TABLE IF NOT EXISTS embedding_models (
    id              TEXT PRIMARY KEY,
    dimension       INTEGER NOT NULL,
    introduced_at   TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS memory_items_vec USING vec0(
    item_id TEXT PRIMARY KEY,
    embedding FLOAT[384]
);

CREATE TABLE IF NOT EXISTS write_decisions (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id           INTEGER NOT NULL REFERENCES events(id),
    candidate_content  TEXT NOT NULL,
    decision           TEXT NOT NULL CHECK (decision IN ('store','merge','supersede','skip','review')),
    rule_name          TEXT NOT NULL,
    rule_inputs_json   TEXT NOT NULL,
    resulting_item_id  TEXT REFERENCES memory_items(id),
    created_at         TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS review_queue (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    write_decision_id   INTEGER NOT NULL REFERENCES write_decisions(id),
    reason              TEXT NOT NULL CHECK (reason IN
                          ('low_confidence','unresolved_contradiction','consolidation_contradiction',
                           'consolidation_cluster_promotion')),
    status              TEXT NOT NULL CHECK (status IN
                          ('pending','resolved_store','resolved_merge','resolved_supersede','resolved_skip')),
    created_at          TEXT NOT NULL,
    resolved_at         TEXT
);

CREATE TABLE IF NOT EXISTS retrieval_events (
    id                        INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id                  INTEGER NOT NULL UNIQUE REFERENCES events(id),
    query_text                TEXT NOT NULL,
    vector_candidates_json    TEXT NOT NULL,
    keyword_candidates_json   TEXT NOT NULL,
    fused_ranking_json        TEXT NOT NULL,
    result_budget             INTEGER NOT NULL,
    returned_item_ids_json    TEXT NOT NULL,
    created_at                TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS outcome_records (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    retrieval_event_id   INTEGER NOT NULL REFERENCES retrieval_events(event_id),
    item_id              TEXT NOT NULL REFERENCES memory_items(id),
    used                 INTEGER NOT NULL,
    reported_at          TEXT NOT NULL
);

-- Phase 2: consolidation (data-model.md).
CREATE TABLE IF NOT EXISTS consolidation_runs (
    id                      TEXT PRIMARY KEY,
    started_at              TEXT NOT NULL,
    finished_at             TEXT,
    items_processed         INTEGER NOT NULL DEFAULT 0,
    clusters_formed         INTEGER NOT NULL DEFAULT 0,
    contradictions_flagged  INTEGER NOT NULL DEFAULT 0,
    status                  TEXT NOT NULL CHECK (status IN ('running','completed','interrupted'))
);

CREATE TABLE IF NOT EXISTS cluster_members (
    id                     TEXT PRIMARY KEY,
    cluster_id              TEXT NOT NULL,
    summary_item_id        TEXT NOT NULL REFERENCES memory_items(id),
    source_item_id          TEXT NOT NULL REFERENCES memory_items(id),
    consolidation_run_id   TEXT NOT NULL REFERENCES consolidation_runs(id),
    created_at             TEXT NOT NULL
);

-- T029 [Convergence/SC-003]: index on summary_item_id speeds up the GROUP BY subquery in
-- list_enriched() which aggregates cluster_members per summary_item_id on every review list call.
CREATE INDEX IF NOT EXISTS cluster_members_summary_item_id_idx ON cluster_members(summary_item_id);

-- Feature 008: deterministic per-cluster n-gram model (data-model.md).
CREATE TABLE IF NOT EXISTS cluster_gram_models (
    cluster_id              TEXT PRIMARY KEY,
    consolidation_run_id   TEXT NOT NULL REFERENCES consolidation_runs(id),
    gram_order              INTEGER NOT NULL,
    vocab_size              INTEGER NOT NULL,
    training_token_count   INTEGER NOT NULL,
    table_json              TEXT NOT NULL,
    created_at              TEXT NOT NULL
);

-- Phase 2: eval harness (data-model.md).
CREATE TABLE IF NOT EXISTS eval_results (
    id                  TEXT PRIMARY KEY,
    run_id              TEXT NOT NULL,
    probe_id            TEXT NOT NULL,
    category            TEXT NOT NULL,
    probe_set_version   TEXT NOT NULL,
    passed              INTEGER NOT NULL,
    ran_at              TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS baseline_reports (
    id                       TEXT PRIMARY KEY,
    probe_set_version        TEXT NOT NULL,
    generated_at             TEXT NOT NULL,
    pamet_results_json       TEXT NOT NULL,
    hindsight_results_json  TEXT,
    hindsight_available      INTEGER NOT NULL DEFAULT 0
);
"#;

/// Postgres dialect of the same schema (data-model.md's dialect-port table),
/// feature-gated behind `postgres-backend`. Behaviorally equivalent, not
/// byte-identical: `vector(4096)` + a GIN-indexed generated `tsvector`
/// column give `memory_items` a real index for both retrieval paths
/// (FR-005), replacing the SQLite backend's JSON-blob brute-force scan and
/// FTS5 virtual table respectively. Every other column keeps the same type
/// family the Rust binding code already produces (e.g. 0/1 `INTEGER` for
/// the boolean-ish columns, `TEXT` for JSON payloads) so the same
/// parameter-binding code works against either backend without a second
/// per-column type translation layer — a disclosed scope-trim relative to
/// data-model.md's `BOOLEAN`/`JSONB` suggestion, not a correctness gap (see
/// tasks.md Implementation Notes).
#[cfg(feature = "postgres-backend")]
pub const POSTGRES_SCHEMA_SQL: &str = r#"
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS events (
    id            BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
    occurred_at   TEXT    NOT NULL,
    event_type    TEXT    NOT NULL CHECK (event_type IN
                      ('write_decision','retrieval','outcome_update','review_resolution','tombstone',
                       'cluster_formed','summary_created','consolidation_run_started',
                       'consolidation_run_finished','eval_run_completed','item_decayed',
                       'hindsight_memory_migrated','cluster_gram_model_built')),
    payload_json  TEXT    NOT NULL,
    schema_version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS memory_items (
    id                   TEXT PRIMARY KEY,
    item_type            TEXT NOT NULL CHECK (item_type IN ('episode','fact','preference','procedure','summary')),
    content              TEXT NOT NULL,
    embedding            vector(4096),
    content_tsv          tsvector GENERATED ALWAYS AS (to_tsvector('english', content)) STORED,
    entities_json        TEXT,
    confidence           REAL NOT NULL DEFAULT 1.0,
    provenance_event_id  BIGINT NOT NULL REFERENCES events(id),
    supersedes_id        TEXT REFERENCES memory_items(id),
    created_at           TEXT NOT NULL,
    last_used_at         TEXT,
    use_count            INTEGER NOT NULL DEFAULT 0,
    tombstoned_at        TEXT
);

CREATE INDEX IF NOT EXISTS memory_items_content_tsv_idx ON memory_items USING GIN (content_tsv);

CREATE TABLE IF NOT EXISTS write_decisions (
    id                 BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    event_id           BIGINT NOT NULL REFERENCES events(id),
    candidate_content  TEXT NOT NULL,
    decision           TEXT NOT NULL CHECK (decision IN ('store','merge','supersede','skip','review')),
    rule_name          TEXT NOT NULL,
    rule_inputs_json   TEXT NOT NULL,
    resulting_item_id  TEXT REFERENCES memory_items(id),
    created_at         TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS review_queue (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    write_decision_id   BIGINT NOT NULL REFERENCES write_decisions(id),
    reason              TEXT NOT NULL CHECK (reason IN
                          ('low_confidence','unresolved_contradiction','consolidation_contradiction',
                           'consolidation_cluster_promotion')),
    status              TEXT NOT NULL CHECK (status IN
                          ('pending','resolved_store','resolved_merge','resolved_supersede','resolved_skip')),
    created_at          TEXT NOT NULL,
    resolved_at         TEXT
);

CREATE TABLE IF NOT EXISTS retrieval_events (
    id                        BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    event_id                  BIGINT NOT NULL UNIQUE REFERENCES events(id),
    query_text                TEXT NOT NULL,
    vector_candidates_json    TEXT NOT NULL,
    keyword_candidates_json   TEXT NOT NULL,
    fused_ranking_json        TEXT NOT NULL,
    result_budget             INTEGER NOT NULL,
    returned_item_ids_json    TEXT NOT NULL,
    created_at                TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS outcome_records (
    id                   BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    retrieval_event_id   BIGINT NOT NULL REFERENCES retrieval_events(event_id),
    item_id              TEXT NOT NULL REFERENCES memory_items(id),
    used                 INTEGER NOT NULL,
    reported_at          TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS consolidation_runs (
    id                      TEXT PRIMARY KEY,
    started_at              TEXT NOT NULL,
    finished_at             TEXT,
    items_processed         INTEGER NOT NULL DEFAULT 0,
    clusters_formed         INTEGER NOT NULL DEFAULT 0,
    contradictions_flagged  INTEGER NOT NULL DEFAULT 0,
    status                  TEXT NOT NULL CHECK (status IN ('running','completed','interrupted'))
);

CREATE TABLE IF NOT EXISTS cluster_members (
    id                     TEXT PRIMARY KEY,
    cluster_id              TEXT NOT NULL,
    summary_item_id        TEXT NOT NULL REFERENCES memory_items(id),
    source_item_id          TEXT NOT NULL REFERENCES memory_items(id),
    consolidation_run_id   TEXT NOT NULL REFERENCES consolidation_runs(id),
    created_at             TEXT NOT NULL
);

-- Feature 008: deterministic per-cluster n-gram model (data-model.md).
CREATE TABLE IF NOT EXISTS cluster_gram_models (
    cluster_id              TEXT PRIMARY KEY,
    consolidation_run_id   TEXT NOT NULL REFERENCES consolidation_runs(id),
    gram_order              INTEGER NOT NULL,
    vocab_size              INTEGER NOT NULL,
    training_token_count   INTEGER NOT NULL,
    table_json              TEXT NOT NULL,
    created_at              TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS eval_results (
    id                  TEXT PRIMARY KEY,
    run_id              TEXT NOT NULL,
    probe_id            TEXT NOT NULL,
    category            TEXT NOT NULL,
    probe_set_version   TEXT NOT NULL,
    passed              INTEGER NOT NULL,
    ran_at              TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS baseline_reports (
    id                       TEXT PRIMARY KEY,
    probe_set_version        TEXT NOT NULL,
    generated_at             TEXT NOT NULL,
    pamet_results_json       TEXT NOT NULL,
    hindsight_results_json  TEXT,
    hindsight_available      INTEGER NOT NULL DEFAULT 0
);
"#;
