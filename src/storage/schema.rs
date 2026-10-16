//! SQLite schema for the event-sourced memory store (data-model.md). T004.

pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS events (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at   TEXT    NOT NULL,
    event_type    TEXT    NOT NULL CHECK (event_type IN
                      ('write_decision','retrieval','outcome_update','review_resolution','tombstone',
                       'cluster_formed','summary_created','consolidation_run_started',
                       'consolidation_run_finished','eval_run_completed')),
    payload_json  TEXT    NOT NULL,
    schema_version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS memory_items (
    id                   TEXT PRIMARY KEY,
    item_type            TEXT NOT NULL CHECK (item_type IN ('episode','fact','preference','procedure','summary')),
    content              TEXT NOT NULL,
    embedding_json        TEXT,
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
                          ('low_confidence','unresolved_contradiction','consolidation_contradiction')),
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
