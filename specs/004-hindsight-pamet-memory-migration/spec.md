# Feature Specification: Hindsight → Pamet Memory Migration & Comparison

**Feature Branch**: `004-hindsight-pamet-memory-migration`

**Created**: 2026-10-19

**Status**: Implemented (real data, partial corpus — see Results)

**Input**: User description: "create migration, then migrate and then we can really
compare quality and speed [of Hindsight vs. Pamet]"

## Context

Hindsight (vectorize-io) is the operator's current LLM-extraction-based memory backend
for Hermes, running live in Docker (`local_external`, bank `hermes`) with **15,417 real
memories** already accumulated. Pamet is the rule-based, event-sourced replacement being
built. This feature is a one-way, additive **export of Hindsight's memories into Pamet**
— Hindsight is NOT decommissioned or modified; both stores coexist afterward specifically
so they can be compared.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Bulk-migrate Hindsight's memory bank into Pamet (Priority: P1)

As the operator, I run one command that reads every memory currently in Hindsight's
`hermes` bank and writes each one into a Pamet store via Pamet's real write path (the
same `remember()` pipeline live usage goes through — this is what makes the later
quality comparison meaningful, not a clone of rows), so Pamet ends up holding an
independently-formed view of the same underlying facts.

**Why this priority**: Without the corpus in Pamet, there is nothing to compare.

**Independent Test**: Run the migration against the live Hindsight bank into a fresh
Pamet store; `pamet recall` returns results; row counts and timing are reported.

**Acceptance Scenarios**:

1. **Given** a Hindsight bank with N valid memories, **When** the migration runs to
   completion, **Then** Pamet's store contains write-decision provenance for all N
   source memories (stored, merged, superseded, or review-gated — never silently
   dropped), and running the migration again is a no-op (idempotent, resumable).
2. **Given** the migration is interrupted partway (process killed), **When** it is
   re-run, **Then** it resumes from where it left off rather than re-processing
   already-migrated memories or erroring out.
3. **Given** a Hindsight memory carries a `document_id`, `tags`, `fact_type`, and a
   `date`, **When** it is migrated, **Then** that provenance is preserved in Pamet's
   event log (not discarded), traceable back to the originating Hindsight memory id.

---

### User Story 2 - Invalidated Hindsight memories carry over as tombstones (Priority: P2)

As the operator, memories Hindsight has already marked `invalidated` (superseded or
retracted) should not resurrect as live facts in Pamet — but Pamet's Principle II
(provenance & reversibility, never a destructive delete) means they should still exist
as a tombstoned, traceable record rather than being silently skipped.

**Why this priority**: Correctness of the migrated corpus's *valid* state depends on
this; it is P2 rather than P1 because (per direct query) 0 of the first 3,000 sampled
Hindsight memories were observed in `invalidated` state — this path may be rare in
practice but must not be unhandled.

**Independent Test**: Seed or find a Hindsight memory in `invalidated` state; after
migration, confirm it exists in Pamet's event log but is excluded from `recall()`
results (tombstoned, not live).

**Acceptance Scenarios**:

1. **Given** a Hindsight memory with `state: invalidated`, **When** it is migrated,
   **Then** Pamet records it and then forgets it (tombstone), so it is provenance-
   traceable but not returned by `recall()`.

---

### User Story 3 - Side-by-side quality & speed comparison (Priority: P1)

As the operator, after migration I want one benchmark run that queries both systems
with the same set of questions and reports, side by side: per-query latency (p50/p95)
and the actual retrieved results for each system, so quality can be judged by reading
real output, not a synthetic score.

**Why this priority**: This is the entire stated reason for building the migration — it
is co-equal in priority with the migration itself, not a nice-to-have polish step.

**Independent Test**: Run the comparison harness against the migrated Pamet store and
the live Hindsight bank with a fixed query set; it produces a report with both systems'
results and timings for every query, runnable repeatedly without needing to re-migrate.

**Acceptance Scenarios**:

1. **Given** both stores hold the migrated corpus, **When** the comparison harness runs
   a fixed set of recall queries against both, **Then** it reports, per query: Pamet's
   top-k results + latency, Hindsight's top-k results + latency, with no query silently
   skipped on either side.
2. **Given** the harness has run once, **When** it is run again with the same query
   set, **Then** timings may vary (both systems are live) but the result sets for
   semantically-stable queries are stable, not random per run.

---

### Edge Cases

- What happens when a Hindsight memory's `text` is empty or whitespace-only? → Skip it,
  count it, do not write an empty fact into Pamet.
- What happens when the Hindsight API is unreachable mid-migration? → The run halts with
  a clear error and is resumable (US1/AC2) — it must not have discarded progress already
  committed to Pamet's event log.
- What happens when two Hindsight memories have near-identical text (Hindsight's own
  `experience`/`observation` pair for the same source turn, as seen in the live bank)?
  → This is exactly what Pamet's write-policy near-duplicate clustering is *for*; it is
  expected and desired behavior, not a bug, and is one of the things the comparison in
  US3 should surface.
- What happens to Hindsight's `fact_type` (`experience`/`observation`/`world`)? → Carried
  through as a tag/prefix on the migrated content (same pattern the existing MEMORY.md→
  Hindsight migration already uses: `"[hindsight:{fact_type}] {text}"`), not dropped.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The migration MUST read every memory from a given Hindsight bank via its
  REST API (`GET /v1/default/banks/{bank_id}/memories/list`), paginating until
  exhausted, not just the first page.
- **FR-002**: Each migrated memory MUST be written into Pamet via the same write path
  live usage uses (`pamet::remember()`/the `Interaction` struct), not a bulk/raw insert
  — this feature is explicitly about comparing the two systems' real write-time
  behavior (dedup, clustering, review-gating), so bypassing that behavior would
  invalidate the comparison.
- **FR-003**: The migration MUST be idempotent and resumable: re-running it after a
  partial run or a full prior run MUST NOT re-process memories already migrated, and
  MUST NOT error out — it continues from where it left off. A local ledger keyed by the
  Hindsight memory id (separate from Pamet's own event log) tracks migration state.
- **FR-004**: Hindsight memories in `invalidated` state MUST be migrated and then
  tombstoned in Pamet (recorded, not live), never silently dropped and never left live.
- **FR-005**: Hindsight provenance (`id`, `document_id`, `tags`, `fact_type`, `date`)
  MUST be preserved as part of the event Pamet records for that write, so a migrated
  Pamet item is traceable back to its originating Hindsight memory.
- **FR-006**: The migration MUST report progress (count processed / total, elapsed
  time) during a long run, not go silent until completion.
- **FR-007**: A comparison harness MUST accept a fixed list of natural-language queries
  and, for each, query both Hindsight (`POST .../memories/recall`) and Pamet
  (`pamet::recall()`), recording latency and the returned result text for both.
- **FR-008**: The comparison harness's output MUST be a durable report file (not only
  console output), so results from a given run can be reviewed later and diffed against
  a subsequent run.
- **FR-009**: The migration MUST skip empty/whitespace-only source text without writing
  a fact for it, while still counting it in the final report.

### Key Entities

- **Hindsight memory**: `id`, `text`, `context`, `date`, `fact_type`
  (observation/experience/world), `document_id`, `tags`, `state` (valid/invalidated),
  `proof_count`, `entities`. Source of truth for this migration; untouched by it.
- **Migration ledger**: a small local record (Hindsight memory id → migrated/skipped +
  resulting Pamet provenance) used purely for idempotency/resumability; not part of
  Pamet's own event-sourced model.
- **Comparison report**: one row per benchmark query × system, holding latency and
  returned result text, written to a durable file per run.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of valid (non-empty-text) Hindsight memories in the target bank
  exist, traceably, in Pamet's event log after migration completes — verified by count
  and by spot-checking provenance on a sample.
- **SC-002**: Re-running the migration against an already-fully-migrated bank processes
  0 new memories and completes in well under a minute (ledger-driven skip, not a full
  re-scan of Pamet's write-policy).
- **SC-003**: The full migration of the live ~15,000-memory bank completes; the actual
  wall-clock time is measured and reported rather than assumed — given Pamet's live
  write path does a near-duplicate scan per write (O(n) per item, i.e. O(n²) total), no
  a priori time bound is asserted here; the real number from the real run becomes the
  basis for any future optimization decision, the same way SC-002/SC-003 in feature 003
  were set from real measurement, not guesswork.
- **SC-004**: The comparison harness produces a side-by-side report for at least 15
  real queries drawn from actual past conversation topics, with zero queries silently
  failing on either system.

## Assumptions

- Hindsight stays live and unmodified throughout and after this feature — this is an
  additive export, not a cutover or decommissioning.
- The migration targets Hindsight's `hermes` bank specifically (the one the operator
  actually uses), via the already-configured `local_external` API at
  `http://localhost:8888`.
- "Compare quality" in US3 is operator-judged by reading the side-by-side report, not an
  automated scoring metric — there is no labeled ground-truth answer set for this
  conversational history to score against automatically.
- Given FR-002's constraint (real write path, not bulk insert) and Pamet's current O(n)
  per-write near-duplicate scan, the ~15,000-item migration may take a non-trivial
  amount of wall-clock time; SC-003 deliberately does not pre-commit to a numeric bound
  for this reason — see SC-003's rationale.
- The migration tool runs as a standalone Rust binary/example within the `pamet` repo
  (consistent with `pamet migrate`'s existing CLI pattern from feature 003), talking to
  Hindsight over plain HTTP on localhost — no new external dependency beyond an HTTP
  client.

## Results (real run, 2026-10-19)

**Migration (US1/US2, FR-001..006)**: `pamet import-hindsight` built and run for real
against the live `hermes` bank (15,446 memories at run time). **SC-003's own premise was
confirmed empirically, and worse than the stated worst case**: per-write cost did not
stay flat — elapsed time per 200-item batch grew from 4.4s (first batch) to 101.9s (batch
ending at item 2,800), i.e. genuinely super-linear, not the flat ~146 items/s a synthetic
benchmark with short, low-overlap text had suggested beforehand. A full-corpus linear-growth
extrapolation projected **~4+ hours** to completion — SC-003 deliberately left this
unbounded for exactly this reason, and the real number is the reason the run was stopped at
a checkpoint (2,936 Hindsight memories → 4,631 Pamet items) rather than driven to
completion. **The migration IS resumable** (FR-003, verified: the ledger records each item
individually, not just per-page, so killing the process mid-batch loses nothing already
recorded) — resuming to full completion is a `pamet import-hindsight` re-run away, at the
cost of several more hours of wall-clock time.

- **Root cause (not fixed, documented)**: Pamet's live write path scans *all* currently-
  active items for near-duplicates on every write (`write_policy::active_items`); on
  short, low-semantic-overlap synthetic text this stays cheap because few candidates
  pass the embedding-similarity pre-filter, but real multi-year conversational memory
  has far higher topical overlap, so the comparison set — and therefore cost — per write
  grows with corpus size. This is a genuine, newly-discovered O(n²)-in-practice
  characteristic of Pamet's current rule-based write policy on realistic data, not a bug
  introduced by this feature; fixing it (e.g., an approximate pre-filter/index before the
  exact near-dup scan) is future work, out of scope here.
- **US2 (tombstones)**: 0 `invalidated`-state memories were observed in the portion of the
  bank migrated (consistent with the 0/3,000 sampled during spec-writing) — the tombstone
  code path is implemented and event-log-verified in isolation but has not yet been
  exercised against a real invalidated Hindsight memory from this bank.

**Comparison (US3, FR-007/008, SC-004)**: `pamet compare-hindsight` built and run for real
against the migrated partial corpus (4,631 items) and the live `hermes` bank, with 18 real
queries (`tests/fixtures/hindsight_comparison_queries_v1.json`) drawn from actual past
conversation topics. Report: `/tmp/pamet_hindsight_comparison.json` (not committed —
operator-local, references real personal/work content).

- **A second real, independently significant finding**: **Hindsight's `recall()` is
  currently broken for the live `hermes` bank** — every one of the 18 queries returned
  HTTP 500 `DataCorruptedError: missing chunk number 0 for toast value 126124 in
  pg_toast_16797` (a Postgres TOAST-chunk corruption inside Hindsight's embedded pg0
  instance, discovered live via direct `curl` before the harness was even built — not an
  artifact of this feature's code). Hindsight's `list` endpoint (used for the migration's
  read side) is unaffected and worked throughout.
- **Pamet, by contrast, answered 18/18 queries successfully** (avg latency 180ms, max
  188ms) with relevant results drawn from the real migrated corpus (e.g., the Chuck
  Norris K8s query surfaced the actual "base image should be updated to avoid security
  [CVEs]" memory; the wife's-career query surfaced the actual Track-1/Track-2 options
  previously recorded).
- Net result as it stands today: on real data, right now, **Pamet is faster to write at
  small-to-medium scale but degrades badly at this corpus's actual semantic density past
  a few thousand items, while Hindsight's write side was never exercised in this
  comparison (no new writes were sent to Hindsight) and its read side is currently down
  entirely** — not a controlled, apples-to-apples speed verdict, but a real, honest
  snapshot of both systems' current operational state.
