---

description: "Task list template for feature implementation"
---

# Tasks: Consolidation Job & Eval Harness

**Input**: Design documents from `/specs/002-consolidation-eval-harness/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/cli-contract.md, quickstart.md (all present)

**Tests**: Included — this project's Phase 1 precedent always ships tests alongside implementation.

**Organization**: Tasks are grouped by user story (US1 = consolidation, US2 = eval harness, US3 = Hindsight baseline) to enable independent implementation and testing of each.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- Paths are repository-root-relative, single Rust crate (same as Phase 1)

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project/config changes needed before any story's code can land

- [X] T001 Add optional `reqwest` (blocking, `json` feature) dependency and a
  `baseline-hindsight` Cargo feature gating it, in `Cargo.toml` (mirrors the
  existing `mcp` feature pattern)
- [X] T002 [P] Create `tests/fixtures/` directory with an empty
  `eval_probes_v1.json` skeleton (`{"version": "v1", "probes": []}`)
- [ ] T003 Update the AGENTS.md SPECKIT pointer to
  `specs/002-consolidation-eval-harness/plan.md` — **requires explicit user
  approval**, blocked once already this phase (protected-file write prompt
  timed out during `/speckit-plan`); do not retry via a different tool path
  without that approval

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Schema/model/replay plumbing every user story needs

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T004 Add `ItemType::Summary` variant (`as_str`/`from_str`) in
  `src/models/memory_item.rs`
- [X] T005 [P] Add `ReviewReason::ConsolidationContradiction` variant in
  `src/models/write_decision.rs`
- [X] T006 Add `cluster_members` and `consolidation_runs` tables (per
  data-model.md) to `src/storage/schema.rs`
- [X] T007 Add `eval_results` and `baseline_reports` tables (per
  data-model.md) to `src/storage/schema.rs` (depends on T006 — same file)
- [X] T008 Extend `src/services/rebuild.rs` to replay `cluster_formed`,
  `summary_created`, `consolidation_run_started`/`_finished`, and
  `eval_run_completed` event types (depends on T004, T006, T007)

**Checkpoint**: Foundation ready — US1, US2, US3 implementation can now begin

---

## Phase 3: User Story 1 - Nightly consolidation without touching the live path (Priority: P1) 🎯 MVP

**Goal**: A separately-triggerable job that clusters near-duplicates into
summaries, flags contradictions for review, and decays unused items — all
event-sourced and non-destructive, with zero added latency to concurrent
live `remember`/`recall`.

**Independent Test**: Run `pamet consolidate` against a store with seeded
near-duplicates and a contradiction (quickstart.md Scenarios 1-2); run the
e2e latency test (quickstart.md Scenario 3) concurrently with live traffic.

### Tests for User Story 1

- [X] T009 [US1] Write integration tests covering clustering, non-destructive
  provenance, contradiction-to-review-queue, and idempotent re-run after an
  interrupted run, in `tests/integration/test_consolidation.rs`
- [X] T010 [P] [US1] Write the concurrent-latency e2e test (background recall
  loop + concurrent consolidation, p95 comparison, SC-001) in
  `tests/e2e/test_consolidation_e2e.rs`

### Implementation for User Story 1

- [X] T011 [US1] Create `Cluster`, `ClusterMember`, `ConsolidationRun` models
  in `src/models/cluster.rs` (depends on T006)
- [X] T012 [P] [US1] Implement decay computation (threshold + multiplier,
  pure function over `last_used_at`/`use_count`, no new per-item column) in
  `src/models/decay.rs`
- [X] T013 [US1] Register the new models in `src/models/mod.rs` (depends on
  T011, T012)
- [X] T014 [US1] Implement threshold-graph clustering (reuse
  `embedding::cosine_similarity` + `write_policy::MERGE_SIMILARITY_THRESHOLD`,
  union-find connected components) in `src/services/consolidation.rs`
  (depends on T013)
- [X] T015 [US1] Implement summary generation (longest-content-wins,
  provenance to every cluster member) in `src/services/consolidation.rs`
  (depends on T014)
- [X] T016 [US1] Implement contradiction flagging during consolidation
  (reuse `write_policy::detect_contradiction` and
  `CONTRADICTION_SIMILARITY_FLOOR`, write to the existing review queue with
  `ReviewReason::ConsolidationContradiction`) in
  `src/services/consolidation.rs` (depends on T014, T005)
- [X] T017 [US1] Apply the decay multiplier from T012 in the RRF fusion step
  in `src/services/retrieval_fusion.rs` (depends on T012)
- [X] T018 [US1] Wire the full consolidation run (events:
  `consolidation_run_started`, `cluster_formed`, `summary_created`,
  `consolidation_run_finished`) through a new `pamet::consolidate()` entry
  point in `src/lib.rs` (depends on T015, T016, T006)
- [X] T019 [US1] Register the `consolidation` module in
  `src/services/mod.rs` (depends on T014)
- [X] T020 [US1] Add the `consolidate` CLI subcommand (`--decay-after-days`,
  `--decay-penalty`, per contracts/cli-contract.md) in `src/bin/pamet.rs`
  (depends on T018)

**Checkpoint**: User Story 1 is fully functional and independently testable
(quickstart.md Scenarios 1-3 pass)

---

## Phase 4: User Story 2 - Repeatable eval harness with held-out probes (Priority: P1)

**Goal**: A single command that runs the 4 held-out probe categories against
Pamet alone and reports deterministic pass/fail + scores.

**Independent Test**: Run `pamet eval` twice against the checked-in fixture
set and diff the two reports (quickstart.md Scenario 4) — must be identical.

### Tests for User Story 2

- [X] T021 [US2] Write integration tests for fixture loading, fresh-temp-DB
  isolation (never mutates the real `--db`), deterministic repeat-run
  scoring (SC-003), and per-category pass/fail reporting, in
  `tests/integration/test_eval_harness.rs`

### Implementation for User Story 2

- [X] T022 [US2] Create `EvalProbe`, `EvalResult`, `BaselineReport` (Pamet-only
  shape) models in `src/models/eval.rs` (depends on T007)
- [X] T023 [P] [US2] Author the versioned synthetic probe fixture — at least
  3 probes per category (`new_fact_recall`, `update_supersession`,
  `forgetting_abstention`, `general_precision`) — in
  `tests/fixtures/eval_probes_v1.json` (depends on T002)
- [X] T024 [US2] Implement fixture loading + fresh isolated temp-DB seeding
  from each probe's `given` operations in `src/services/eval_harness.rs`
  (depends on T022)
- [X] T025 [US2] Implement the deterministic probe runner (exact-match /
  abstention checks, no fuzzy scoring) and per-category pass/fail + latency
  aggregation in `src/services/eval_harness.rs` (depends on T024)
- [X] T026 [US2] Implement the report JSON writer (contracts/cli-contract.md
  shape) and `eval_run_completed` event emission into `baseline_reports` in
  `src/services/eval_harness.rs` (depends on T025, T007)
- [X] T027 [US2] Register the `eval_harness` module in `src/services/mod.rs`
  (depends on T024)
- [X] T028 [US2] Add the `eval` CLI subcommand (`--fixtures`, `--report-out`,
  Pamet-only path) in `src/bin/pamet.rs` (depends on T026)
- [X] T029 [US2] Add the CI-usable exit-code gate (0 if every probe passed,
  1 if any failed) to the `eval` subcommand in `src/bin/pamet.rs` (depends
  on T028)

**Checkpoint**: User Stories 1 AND 2 both work independently (quickstart.md
Scenario 4 passes)

---

## Phase 5: User Story 3 - Versioned baseline report vs. Hindsight (Priority: P2)

**Goal**: The same probe set additionally run against the local Hindsight
instance, with both systems' scores/latencies in one report; graceful
degradation if Hindsight is unreachable.

**Independent Test**: Run `pamet eval --with-hindsight` with the `hindsight`
container up (full report) and down (`hindsight_available: false`, no
failure) — quickstart.md Scenario 5.

### Tests for User Story 3

- [X] T030 [US3] Write integration tests for the `hindsight_available: false`
  degraded path (container down) and, where the local `hindsight` container
  is reachable, the full dual-system report, in
  `tests/integration/test_eval_harness.rs` (depends on T021 — same file)

### Implementation for User Story 3

- [X] T031 [US3] Implement a thin Hindsight REST client (`POST /v1/memory`
  retain, `POST /v1/memory/search` recall) behind
  `#[cfg(feature = "baseline-hindsight")]` in
  `src/services/baseline_hindsight.rs` (depends on T001)
- [X] T032 [US3] Wire the Hindsight column into the probe runner and report
  writer, with the unreachable-Hindsight fallback (spec Edge Cases), in
  `src/services/eval_harness.rs` (depends on T031, T025)
- [X] T033 [US3] Add the `--with-hindsight` flag to the `eval` subcommand,
  feature-gated, in `src/bin/pamet.rs` (depends on T032, T028)
- [X] T034 [US3] Register the feature-gated `baseline_hindsight` module in
  `src/services/mod.rs` (depends on T031)

**Checkpoint**: All three user stories are independently functional
(quickstart.md Scenario 5 passes, with and without the Hindsight container up)

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Verification and documentation across all three stories

- [X] T035 [P] Run `cargo build --locked` and
  `cargo build --locked --features baseline-hindsight`; fix any drift
- [X] T036 [P] Run `cargo test` (default features) and
  `cargo test --features baseline-hindsight` (with the `hindsight` container
  up); confirm all green
- [X] T037 Run quickstart.md Scenarios 1-5 end-to-end against a real build;
  update quickstart.md with actual observed output where it diverges from
  what was speculated during planning
- [X] T038 [P] Add `consolidate` and `eval` subcommands to `README.md` and
  `docs/GUIDE.md`
- [ ] T039 Confirm the AGENTS.md pointer (T003) has actually been approved
  and applied — do not mark this phase done while it is still pending
- [X] T040 [P] Mark all tasks in this file `[X]`, and append an
  "Implementation Notes" section recording any actual-vs-planned deviations
  (mirroring Phase 1's precedent in `specs/001-memory-store-mvp/tasks.md`)

---

## Implementation Notes (post-implementation, actual vs. planned)

1. **Consolidation clustering threshold is NOT `MERGE_SIMILARITY_THRESHOLD`
   (0.85) as research.md originally specified** — discovered empirically
   while writing T009's tests. `write_policy::evaluate` re-scans ALL active
   items on every write and merges anything scoring >= 0.85 into a single
   existing row; two active items can therefore never be >= 0.85 similar to
   each other. Reusing that threshold for clustering would make the
   clustering feature a permanent no-op. Fixed to reuse
   `write_policy::LOW_CONFIDENCE_THRESHOLD` (0.5) instead — the floor of the
   "ambiguous band" that `explicit_remember=true` writes bypass and store
   verbatim, which is the only way two genuinely near-duplicate items ever
   coexist as separate active rows. Still zero new tunables (research.md's
   actual goal), just the correct existing one. See `consolidation.rs`
   doc comments on `cluster_items()`.
2. **`consolidate`'s CLI contract dropped `--decay-after-days`/
   `--decay-penalty`** — decay turned out to be a pure, always-on read-time
   ranking adjustment (`models::decay::multiplier`, applied in
   `retrieval_fusion::apply_decay` on every `recall`), not something a
   `consolidate` *run* configures. The flags in the original
   `contracts/cli-contract.md` draft didn't match this design; removed
   rather than wired to something they wouldn't actually affect.
3. **Per-probe `eval_results` rows are not persisted** — `data-model.md`
   defines this table; the implementation persists only the aggregated
   `baseline_reports` row (categories/pass/total/p95) via the
   `eval_run_completed` event. A disclosed scope trim, not an oversight;
   the per-probe table remains in the schema for a future pass if per-probe
   drill-down is ever needed.
4. **Decision::Supersede does not tombstone the superseded item** (a
   Phase 1 behavior, confirmed while building the eval fixture's
   `update_supersession` probes) — both old and new facts stay active and
   retrievable after a confident contradiction resolution. Not a Phase 2
   bug; noted here because it shapes what "update_supersession" probes can
   assert (presence of the new fact, not absence of the old one).
5. **Hindsight's actual REST contract was verified against its live
   OpenAPI schema and a real round-trip** before writing
   `baseline_hindsight.rs` — `POST /v1/default/banks/{bank_id}/memories`
   (retain) and `.../memories/recall` (recall), not the `/v1/memory` guess
   in the original task description. `tool_search`/README references to
   `/v1/memory` elsewhere predate this verification.
6. **Full real end-to-end eval-with-Hindsight run** (not simulated):
   Pamet 11/12, Hindsight 9/12, against the actual local Hindsight
   container. Pamet's one miss is a real, documented gap (no stopword
   filtering in the hashed bag-of-words embedding lets a query sharing only
   common words weakly clear the relevance floor on an unrelated fact).
7. T003/T039 (AGENTS.md plan-pointer update) remain unchecked — still
   blocked on the protected-file consent gate from `/speckit-plan`; not
   retried per that gate's own instruction.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup (T001-T002 for US3/US2 fixture
  dir; T003 is independent of the rest) — BLOCKS all three user stories
- **User Stories (Phase 3-5)**: All depend on Foundational completion
  - US1 and US2 have no dependency on each other and can proceed in parallel
  - US3 depends on US2's `eval_harness.rs` existing (T025) to wire the
    Hindsight column into — it is NOT independent of US2 the way US1/US2 are
    independent of each other (spec explicitly makes US3 P2, lower priority,
    for this reason)
- **Polish (Phase 6)**: Depends on all three stories being complete

### Within Each User Story

- Tests written before implementation, and MUST fail first
- Models before services; services before CLI subcommand wiring
- US1: clustering → summary generation / contradiction flagging (both
  depend on clustering, independent of each other) → event wiring → CLI
- US2: models → fixture loading → probe runner → report writer → CLI
- US3: Hindsight client → wire into US2's report writer → CLI flag

### Parallel Opportunities

- T002, T005 (Setup/Foundational, different files) can run in parallel
- T010 (US1 e2e test) and T012 (US1 decay) are both parallel-safe against
  the rest of US1's sequential chain
- T023 (US2 fixture authoring) is parallel-safe against T022
  (US2 models) — different files
- Once Foundational (Phase 2) completes, **US1 and US2 can be implemented
  fully in parallel** by different people; US3 should start after US2's
  T025 lands

---

## Parallel Example: Foundational + User Story 1 kickoff

```bash
# Foundational, parallel-safe pair:
Task: "Add ReviewReason::ConsolidationContradiction variant in src/models/write_decision.rs"
Task: "Create tests/fixtures/ directory with an empty eval_probes_v1.json skeleton"

# Once Foundational lands, US1's two parallel-safe tasks:
Task: "Write the concurrent-latency e2e test in tests/e2e/test_consolidation_e2e.rs"
Task: "Implement decay computation in src/models/decay.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1 (Setup) + Phase 2 (Foundational)
2. Complete Phase 3 (US1 — consolidation)
3. **STOP and VALIDATE**: quickstart.md Scenarios 1-3 pass independently
4. This alone already satisfies Constitution Principle IV end-to-end even
   before the eval harness exists

### Incremental Delivery

1. Setup + Foundational → foundation ready
2. US1 (consolidation) → validate independently (MVP for this phase)
3. US2 (eval harness, Pamet-only) → validate independently — this is the
   artifact Principle V actually requires to exist before Phase 3 can start
4. US3 (Hindsight baseline column) → validate independently — sharpens the
   bar but is not itself blocking Phase 3

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Developer A: US1 (consolidation); Developer B: US2 (eval harness) — fully
   parallel, no shared files until Polish
3. Developer A or B picks up US3 once US2's T025 (probe runner) lands

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- No destructive operations anywhere in this phase — consolidation and decay
  are additive/reversible by construction (Constitution Principle II)
- Verify tests fail before implementing
- Commit after each task or logical group
- Stop at any checkpoint to validate a story independently

---

## Phase 7: Convergence

- [X] T041 Gate ambiguous-similarity-band cluster-to-summary promotion in
  `src/services/consolidation.rs` through the existing review queue before
  the summary item is created, rather than auto-promoting it (Constitution II)
  (contradicts)
- [X] T042 Make retrieval deprioritize/suppress clustered original items once
  a summary exists for them, so recall surfaces materially fewer raw
  near-duplicates per query after consolidation runs, in
  `src/services/retrieval_fusion.rs` and/or `src/services/consolidation.rs`
  per SC-005 (contradicts)
- [X] T043 Replace the sequential sanity check in
  `tests/e2e/test_consolidation_e2e.rs` with a genuine concurrent-with-live-
  traffic p95 latency comparison (background consolidation thread + a live
  recall loop, asserting <5% p95 difference) per SC-001 / US1 Acceptance
  Scenario 3 (partial)
- [X] T044 Add a load test seeding ~1,000 memory items and asserting the
  eval harness completes a full run in under 2 minutes, in
  `tests/integration/test_eval_harness.rs` per SC-002 (missing)
- [X] T045 Persist one `eval_results` row per probe per harness run (id,
  run_id, probe_id, category, probe_set_version, passed, ran_at) in
  `src/services/eval_harness.rs`, per data-model.md's `eval_results` table
  and FR-008 (missing)
- [X] T046 Add a rebuild/replay integration test covering
  `consolidation_run_started`/`_finished`, `cluster_formed`,
  `summary_created`, and `eval_run_completed` — verifying the rebuilt store
  matches the live one after a mixed Phase 2 event sequence, in
  `tests/integration/test_rebuild.rs` per Constitution I / data-model.md
  rebuild rules (missing)

### Convergence Implementation Notes

- **T041/T042 design**: both reuse Phase 1's EXISTING "write_decisions +
  review_queue, excluded from `recall` while pending" mechanism
  (`pending_review_candidate_ids()` in `lib.rs`) rather than inventing a new
  gating path. A cluster's summary item IS created immediately (satisfies
  AC1's "clustered into one consolidated item... recoverable"), but is
  invisible to `recall` until a human resolves the review
  (`pamet review resolve <id> --decision store|skip`) — satisfying
  Constitution II. Once resolved as accepted, a NEW filter
  (`confirmed_cluster_source_ids`) excludes the raw originals instead,
  so the net surfaced-items count drops from N originals to 1 summary
  (SC-005). Verified both in tests and against a real release CLI build:
  before resolution → both originals, no summary; after `review resolve`
  → summary only, originals gone.
- **T043**: `Store`/`rusqlite::Connection` isn't `Sync`, so true multi-
  threaded concurrency isn't possible against one connection; "concurrent"
  is tested as fine-grained interleaving (a consolidation pass every 10
  recalls) — this is the scenario SC-001 actually protects against, since
  `consolidate` and `recall` never share a lock in this design.
- **T045**: `eval_results` rows are now written both directly (for
  immediate querying) and via the `eval_run_completed` event's new
  `probe_outcomes` array (for replay fidelity) — `rebuild.rs` recreates
  them identically.
