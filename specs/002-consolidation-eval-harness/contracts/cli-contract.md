# CLI Contract: Consolidation Job & Eval Harness

Extends `specs/001-memory-store-mvp/contracts/cli-contract.md` with two new
`pamet` subcommands. All existing Phase 1 subcommands are unchanged.

## `pamet consolidate`

```text
pamet --db <path> consolidate
```

- Reads all active (non-tombstoned) `memory_items`, clusters near-duplicates
  (research.md §1, corrected post-implementation — see tasks.md
  Implementation Notes #1: the clustering floor is
  `write_policy::LOW_CONFIDENCE_THRESHOLD`, not the merge threshold), writes
  `cluster_formed` + `summary_created` events for each cluster found, and
  flags unresolved contradictions to the review queue.
- Decay is NOT something `consolidate` runs or configures — it's a pure,
  always-on read-time ranking adjustment applied on every `recall`
  (`models::decay`, fixed `DECAY_AFTER_DAYS`/`DECAY_PENALTY` constants). The
  original draft of this contract had `--decay-after-days`/`--decay-penalty`
  flags on this subcommand; dropped once the real design made clear they
  wouldn't affect anything here.
- Exit code 0 on completion (including the zero-items no-op case, per spec
  Edge Cases); non-zero only on a storage error.
- Prints a final
  `consolidation_run=<id> items=<n> clusters=<n> contradictions=<n> decayed=<n>`
  line to stdout — same style as the existing `remember`/`recall` output.
  `decayed` (added T047) is the count of currently-decayed active items
  observed this run; each one also gets its own `item_decayed` event in
  `export-log` (FR-002/SC-006 traceability) — decay itself remains a pure
  read-time ranking computation with no new per-item stored column.
- Safe to re-run at any time, including immediately after an interrupted run
  (FR-010) — any item already folded into a cluster is permanently excluded
  from future clustering, so re-running over an unchanged store forms zero
  new clusters.

## `pamet eval`

```text
pamet --db <path> eval [--fixtures <path>] [--with-hindsight] [--report-out <path>]
```

- `--fixtures` defaults to the checked-in `tests/fixtures/eval_probes_v1.json`.
- Runs every probe against a **fresh temporary SQLite DB** seeded from that
  probe's `given` operations (never the `--db` path given on the command
  line — the eval harness must not mutate a real store).
- Without `--with-hindsight`: Pamet-only `BaselineReport` — `hindsight` is
  `null` and `hindsight_available` is `false` in the JSON (not omitted, just
  empty; `hindsight_available: false` also covers the "requested but
  unreachable" case — both read the same from the report).
- With `--with-hindsight` (requires the crate built with
  `--features baseline-hindsight`): also runs the identical probe set against
  the local Hindsight instance (`http://localhost:8888` by default) using a
  fresh, isolated bank per run; if Hindsight is unreachable, the report still
  completes with `hindsight_available: false` (spec Edge Cases) rather than
  failing the whole command.
- `--report-out` defaults to `./pamet-eval-report-<timestamp>.json`; the
  report is also recorded in `baseline_reports` in the `--db` store via an
  `eval_run_completed` event (Principle I).
- Exit code: 0 if every probe passed, 1 if any probe failed (so this is
  CI-usable as a gate), independent of whether `--with-hindsight` was used.
- Running `eval` twice with the same `--fixtures` and no store mutation in
  between MUST produce byte-identical `pamet_results_json` (SC-003).

## Report JSON shape (written by `pamet eval`)

```json
{
  "probe_set_version": "v1",
  "generated_at": "2026-10-16T12:00:00Z",
  "pamet": {
    "new_fact_recall": {"passed": 5, "total": 5, "p95_latency_ms": 8.1},
    "update_supersession": {"passed": 4, "total": 4, "p95_latency_ms": 9.0},
    "forgetting_abstention": {"passed": 3, "total": 3, "p95_latency_ms": 7.4},
    "general_precision": {"passed": 5, "total": 5, "p95_latency_ms": 7.9}
  },
  "hindsight_available": true,
  "hindsight": {
    "new_fact_recall": {"passed": 5, "total": 5, "p95_latency_ms": 2100.0},
    "update_supersession": {"passed": 4, "total": 4, "p95_latency_ms": 2250.0},
    "forgetting_abstention": {"passed": 2, "total": 3, "p95_latency_ms": 180.0},
    "general_precision": {"passed": 5, "total": 5, "p95_latency_ms": 165.0}
  }
}
```
