# Data Model: Consolidation Job & Eval Harness

All new tables are additive to the existing Phase 1 schema (`events`,
`memory_items`, `write_decisions`, `retrieval_events`, `outcome_records`,
`review_queue`) and remain rebuildable by replaying `events` alone
(Principle I) — every row below is produced by replaying a new consolidation
or eval event type, never written directly without a corresponding event.

## memory_items (extended)

- `item_type` gains a new variant: `summary` (alongside the existing
  `episode`, `fact`, `preference`, `procedure`). A `summary` item is the
  output of consolidating a `Cluster`; it carries a normal `provenance_event_id`
  (the consolidation-run event that created it) plus the `cluster_members`
  rows below for per-source provenance.
- No other column changes. `last_used_at` / `use_count` (already present)
  drive the new `DecayState` computation — decay does not need its own
  per-item mutable column; it is computed at read time from existing fields
  plus the `decay_config` constants (no new per-row state to keep in sync).

## cluster_members (new table)

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT (uuid) PK | |
| `cluster_id` | TEXT | groups rows belonging to one consolidation cluster |
| `summary_item_id` | TEXT FK → memory_items.id | the resulting `summary` item |
| `source_item_id` | TEXT FK → memory_items.id | one original item folded into the cluster |
| `consolidation_run_id` | TEXT FK → consolidation_runs.id | which run produced this |
| `created_at` | TEXT (ISO8601) | |

Rebuild rule: populated only by replaying `cluster_formed` events; never
written directly.

## consolidation_runs (new table)

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT (uuid) PK | |
| `started_at` | TEXT | |
| `finished_at` | TEXT (nullable) | null while in progress / if interrupted |
| `items_processed` | INTEGER | |
| `clusters_formed` | INTEGER | |
| `contradictions_flagged` | INTEGER | |
| `status` | TEXT | `running` \| `completed` \| `interrupted` |

Rebuild rule: one row per `consolidation_run_started` / `_finished` event
pair; an `interrupted` status is itself derivable at replay time (a
`_started` event with no matching `_finished` event) — satisfies FR-010's
idempotent-rerun requirement, since a re-run can detect and account for a
prior interrupted run rather than double-counting.

## review_queue (reused, not extended)

Contradictions flagged during consolidation use the exact same table and
`ReviewReason`/`ReviewStatus` enums Phase 1 already ships — a new
`ReviewReason::ConsolidationContradiction` variant distinguishes the source,
but no new table is introduced (per spec Assumptions: "this spec does not
introduce a second review mechanism").

## eval_probes (fixture, not a DB table)

Lives on disk as versioned JSON (`tests/fixtures/eval_probes_v1.json`), not
in SQLite — probes are test inputs, not runtime state. Schema:

```json
{
  "version": "v1",
  "probes": [
    {
      "id": "new-fact-recall-001",
      "category": "new_fact_recall",
      "given": [
        {"text": "The project is called Pamet.", "source_id": "fixture", "explicit_remember": true}
      ],
      "action": {"type": "recall", "query": "what is the project called"},
      "expected": {"type": "contains_content", "value": "Pamet"}
    }
  ]
}
```

Probe categories (FR-005): `new_fact_recall`, `update_supersession`,
`forgetting_abstention`, `general_precision`.

## eval_results (new table)

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT (uuid) PK | |
| `run_id` | TEXT | groups all results from one harness invocation |
| `probe_id` | TEXT | matches `eval_probes.json` probe id |
| `category` | TEXT | one of the 4 probe categories |
| `probe_set_version` | TEXT | e.g. `"v1"` — required by spec Edge Cases (never compare across versions silently) |
| `passed` | INTEGER (bool) | |
| `ran_at` | TEXT | |

Rebuild rule: eval runs are NOT replayed from `events` — they are
read-only probes against a point-in-time store snapshot, re-runnable on
demand; `eval_results` rows are themselves written via a `eval_run_completed`
event for audit-trail consistency with Principle I, but a fresh eval run
(not a replay) is the normal way to regenerate them.

## baseline_reports (new table, written by the eval harness)

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT (uuid) PK | |
| `probe_set_version` | TEXT | |
| `generated_at` | TEXT | |
| `pamet_results_json` | TEXT | serialized per-category scores + latencies |
| `hindsight_results_json` | TEXT (nullable) | null + `hindsight_available = 0` if Hindsight was unreachable (spec Edge Cases) |
| `hindsight_available` | INTEGER (bool) | |
