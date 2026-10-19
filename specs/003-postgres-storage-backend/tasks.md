---

description: "Task list template for feature implementation"
---

# Tasks: Production-Grade Storage Backend

**Input**: Design documents from `/specs/003-postgres-storage-backend/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/cli-contract.md, quickstart.md (all present)

**Tests**: Included — this project's Phase 1/2 precedent always ships tests alongside implementation.

**Organization**: Tasks are grouped by user story (US1 = genuine concurrency, US2 = migration, US3 = offline mode unaffected) to enable independent implementation and testing of each.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- Paths are repository-root-relative, single Rust crate (same as Phase 1/2)

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project/config changes needed before any story's code can land

- [X] T001 Add optional `postgres`, `r2d2`, `r2d2_postgres`, and `pgvector` (with its
  `postgres` feature) dependencies and a `postgres-backend` Cargo feature gating them, in
  `Cargo.toml` (mirrors the existing `mcp`/`baseline-hindsight` feature pattern)
- [X] T002 [P] Register the 3 new feature-gated `[[test]]` targets
  (`e2e_postgres_concurrency`, `e2e_migration`, `integration_postgres_scale`) in
  `Cargo.toml`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Backend-selection plumbing every user story needs

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T003 Add a `Backend` enum (`Sqlite(rusqlite::Connection)` |
  `Postgres(r2d2::Pool<PostgresConnectionManager<...>>)`, the latter feature-gated) and a
  `backend` field to `Store`; make `Store::open(db_url)` dispatch on the URL scheme
  (`postgres://`/`postgresql://` → Postgres, anything else → today's unchanged SQLite path)
  in `src/storage/store.rs` (depends on T001)
- [X] T004 [P] Create `src/storage/postgres_pool.rs` — `r2d2`/`r2d2_postgres` pool
  construction, feature-gated, fails fast with a clear error on an unreachable configured
  target (FR-007) (depends on T001)
- [X] T005 Add the Postgres DDL (per data-model.md's dialect-port table) to
  `src/storage/schema.rs`, applied when `Store::open` selects the Postgres backend
  (depends on T003, T004)
- [X] T006 Extend `src/services/events.rs`'s `append_event`/`export_log` with a
  Postgres-dialect branch (depends on T003, T005)
- [X] T007 [P] Generalize `src/services/rebuild.rs`'s `apply_event` replay logic to target
  either backend (not always a fresh in-memory SQLite store) — this is reused directly by
  `migrate` in US2 (depends on T003)

**Checkpoint**: Foundation ready — US1, US2, US3 implementation can now begin

---

## Phase 3: User Story 1 - Consolidation and live traffic run truly concurrently (Priority: P1) 🎯 MVP

**Goal**: Live remember/recall and a `consolidate()` run can execute from independent
connections against the Postgres backend without one blocking or measurably slowing the
other.

**Independent Test**: Run quickstart.md Scenario 1 — a live recall loop on one connection,
`consolidate()` on a second, concurrently; p95 latency differs by <5%.

### Tests for User Story 1

- [X] T008 [P] [US1] Write the genuine two-thread concurrency test (live recall loop +
  concurrent `consolidate()` on a second pooled connection, p95 comparison, SC-001) in
  `tests/e2e/test_postgres_concurrency.rs`, feature-gated and skipping cleanly if
  `PAMET_TEST_POSTGRES_URL` is unset/unreachable (depends on T002)

### Implementation for User Story 1

- [X] T009 [US1] Extend `write_policy.rs`'s `active_items` query with a Postgres-dialect
  branch (depends on T003, T006)
- [X] T010 [US1] Extend `retrieval_vector.rs` with a `pgvector` similarity-search branch
  (FR-005) alongside the existing SQLite brute-force scan (depends on T005)
- [X] T011 [US1] Extend `retrieval_keyword.rs` with a Postgres `tsvector`/GIN query branch
  alongside the existing SQLite FTS5 query (depends on T005)
- [X] T012 [US1] Extend `retrieval_fusion.rs`'s decay-lookup query with a Postgres-dialect
  branch (depends on T003)
- [X] T013 [US1] Extend `consolidation.rs`'s queries (active-unconsolidated-items,
  cluster-member inserts, the review gate, contradiction flagging, decay-observation
  events) with Postgres-dialect branches — the clustering/summary-selection/contradiction
  *rule logic* itself needs no change, it already operates purely on in-memory
  `MemoryItem`s (depends on T005, T006)
- [X] T014 [US1] [P] Extend `review.rs` and `outcome.rs` with Postgres-dialect branches
  (depends on T003)
- [X] T015 [US1] Extend `eval_harness.rs`'s `persist_report`/`persist_eval_results` with
  Postgres-dialect branches (depends on T005, T006)
- [X] T016 [US1] Confirm `lib.rs`'s `remember`/`recall`/`consolidate`/`tombstone`/
  `resolve_review` entry points work unchanged against either backend (they call into the
  services above, which now branch internally) (depends on T009-T015)
- [X] T017 [US1] Run T008's concurrency test against a real local `pgvector` container;
  fix any drift (depends on T008, T016)

**Checkpoint**: User Story 1 is fully functional and independently testable
(quickstart.md Scenario 1 passes)

---

## Phase 4: User Story 2 - Existing Pamet data moves over without loss (Priority: P1)

**Goal**: A one-time `pamet migrate` command moves a complete existing embedded store onto
the Postgres backend with no data loss, verifiable by replay.

**Independent Test**: Run quickstart.md Scenario 2 — migrate a store with a mix of facts, a
consolidated summary, a pending review, and a resolved review; counts match; the migrated
store's exported log replays identically.

### Tests for User Story 2

- [X] T018 [P] [US2] Write the migration e2e test — build a representative embedded-SQLite
  source store (facts, a near-duplicate pair that consolidates into a summary, a pending
  review, a resolved review, an interrupted consolidation run), migrate it, and assert item
  count, event count, and review-queue pending/resolved counts all match source vs. target,
  plus that the target's own exported log replays to equivalent state (SC-002, FR-004,
  FR-008, FR-009), in `tests/e2e/test_migration_e2e.rs`, feature-gated (depends on T002)

### Implementation for User Story 2

- [ ] T019 [US2] Create `src/services/migrate.rs` — read the source's complete event log
  via the existing `export_log()`, then replay it into a freshly-initialized target store
  using T007's generalized `rebuild::apply_event` logic (depends on T007)
- [ ] T020 [US2] Add the refuse-non-empty-target guard (data-model.md "Migration Run"
  re-run safety) to `migrate.rs` (depends on T019)
- [X] T021 [US2] Add the `migrate --from <source> --to <target>` CLI subcommand in
  `src/bin/pamet.rs`, printing
  `migration_complete items=<n> events=<n> reviews_pending=<n> reviews_resolved=<n>` on
  success (depends on T019, T020)
- [X] T022 [US2] Run T018's migration e2e test against a real local `pgvector` container;
  fix any drift (depends on T018, T021)

**Checkpoint**: User Stories 1 AND 2 both work independently (quickstart.md Scenario 2
passes)

---

## Phase 5: User Story 3 - Offline, dependency-free mode still works (Priority: P2)

**Goal**: With no production backend configured, every existing Pamet capability behaves
exactly as it did before this feature, fully offline.

**Independent Test**: Run quickstart.md Scenario 3 — the full existing command set against
an embedded SQLite store, no network reachability assumed.

### Tests for User Story 3

- [X] T023 [US3] Confirm the full pre-existing test suite (all 53 Phase 1/2 tests) passes
  unchanged on a default build (no `postgres-backend` feature) — this is a regression
  check, not new test code: `cargo build --locked` and `cargo test` with zero new
  dependencies pulled in (FR-006, SC-004) (depends on T009-T015's feature-gating being
  correct)
- [ ] T024 [P] [US3] Add a test confirming a `postgres://` URL passed to a build *without*
  `postgres-backend` fails with a clear "not compiled in" error rather than a silent no-op
  (FR-007, contracts/cli-contract.md "Feature gating"), in `tests/smoke/test_smoke.rs`
  (depends on T003)

**Checkpoint**: All three user stories are independently functional (quickstart.md
Scenario 3 passes with zero network reachability)

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Verification and documentation across all three stories

- [ ] T025 [P] Write the SC-003 scale test (10,000-item store, similarity search under
  100ms per query) in `tests/integration/test_postgres_scale.rs`, feature-gated
- [X] T026 [P] Run `cargo build --locked` and
  `cargo build --locked --features postgres-backend`; fix any drift
- [X] T027 [P] Run `cargo test` (default features) and
  `cargo test --features postgres-backend` (with a real local `pgvector` container up);
  confirm all green
- [ ] T028 Run quickstart.md Scenarios 1-5 end-to-end against a real build; update
  quickstart.md with actual observed output where it diverges from what was speculated
  during planning
- [ ] T029 [P] Add the `migrate` subcommand and Postgres backend selection to `README.md`
  and `docs/GUIDE.md`
- [ ] T030 [P] Mark all tasks in this file `[X]`, and append an "Implementation Notes"
  section recording any actual-vs-planned deviations (mirroring Phase 1/2's precedent)

---

## Implementation Notes (T030 — actual vs. planned)

**Done (22/30)**: T001-T018, T021-T023, T026-T027. Core architecture, all US1
per-service Postgres-dialect branches, and US2's actual migration mechanism are
implemented and verified against a real local `pgvector/pgvector:pg16` container
(not mocked) — `remember`/`recall`/`consolidate`/`tombstone`/`review` round-trip
correctly on Postgres, SQLite→Postgres migration round-trips a real item through
`recall()` on the target, and all 53 pre-existing Phase 1/2 tests still pass
unchanged on the default (no `postgres-backend`) build.

**Not done — genuine gaps, not silently skipped**:
- **T019/T020**: no separate `src/services/migrate.rs` module exists. The
  functionality lives directly as `rebuild::rebuild_into()` + the CLI's `migrate`
  subcommand in `pamet.rs` — same Principle-I-reuse approach the task intended,
  different file layout. The **refuse-non-empty-target guard (T020) was never
  added** — `pamet migrate --to <existing-store>` will currently append/duplicate
  rather than refusing. Needs a follow-up check (`SELECT COUNT(*) FROM events`
  on the target before replay) before this is safe for a real adopted deployment.
- **T024**: no test asserts the "clear error, not silent no-op" behavior for a
  `postgres://` URL on a build without `postgres-backend` — the code path
  (`Store::open_postgres`'s `#[cfg(not(...))]` arm) exists and returns a
  `Validation` error, but it's unverified by an automated test.
- **T025**: `tests/integration/test_postgres_scale.rs` was written and passes
  against the real container, but at **300 items / <10s**, not SC-003's literal
  **10,000 items / <100ms per query** — a materially looser bound, same kind of
  scope-trim Phase 2's T050 convergence pass caught and fixed for its own scale
  test. Real measured numbers at 300 items: recall ~28ms, consolidate ~31ms —
  promising, but not evidence SC-003 holds at 10k.
- **T028/T029**: quickstart.md was not re-run end-to-end to replace its
  speculated output with real observed output, and README.md/docs/GUIDE.md were
  not updated to mention the Postgres backend or `migrate` subcommand.

**Disclosed deviations within "done" tasks**:
- Vector search (T010) scores via a Rust-side cosine scan read from the
  `vector` column, not pgvector's native `<=>` operator/index — same ranking
  behavior as SQLite for parity, but not yet exploiting the index FR-005 asked
  for. The column and a future index are there; the query isn't using it.
- Several Postgres column types required casts not anticipated in data-model.md
  (`confidence REAL` → bind as `f32` not `f64`; `use_count`/`result_budget`/
  `items_processed` etc. `INTEGER` → bind as `i32` not `i64`) — found and fixed
  via real runtime errors against the live container, not by static review.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup (T001 for the feature/deps) — BLOCKS all
  three user stories
- **User Stories (Phase 3-5)**: All depend on Foundational completion
  - US1 and US2 do not depend on each other and can proceed in parallel once US1's
    per-file Postgres-dialect branches (T009-T015) land — US2's `migrate.rs` (T019) depends
    on T007 (Foundational), not on US1's retrieval/consolidation query branches
  - US3 is almost entirely a regression check on US1's work (feature-gating correctness),
    so in practice it runs last even though it has no hard dependency edge beyond
    Foundational
- **Polish (Phase 6)**: Depends on all three stories being complete

### Within Each User Story

- Tests written before implementation, and MUST fail first (or skip cleanly with no
  Postgres reachable, per research.md Decision 5)
- Storage/query layer before CLI wiring
- US1: per-service Postgres-dialect branches (independent of each other, same pattern
  repeated per file) → `lib.rs` confirmation → concurrency test run
- US2: `migrate.rs` (built on Foundational's generalized replay) → CLI subcommand →
  migration test run
- US3: regression confirmation only, no new production code path

### Parallel Opportunities

- T002, T004 (Setup/Foundational, different files) can run in parallel
- T009-T015 (US1's per-service Postgres-dialect branches) are almost entirely parallel-safe
  against each other — different files, same mechanical pattern, no cross-dependencies
  beyond the shared Foundational `Backend` enum
- T018 (US2 test) and T008 (US1 test) are parallel-safe against each other — different
  files
- T024 (US3 feature-gating test) is parallel-safe against most of US1/US2's work

---

## Parallel Example: Foundational + User Story 1 kickoff

```bash
# Foundational, parallel-safe pair:
Task: "Create src/storage/postgres_pool.rs — r2d2/r2d2_postgres pool construction"
Task: "Generalize rebuild.rs's apply_event replay logic to target either backend"

# Once Foundational lands, US1's per-service branches are almost all parallel-safe:
Task: "Extend retrieval_vector.rs with a pgvector similarity-search branch"
Task: "Extend retrieval_keyword.rs with a Postgres tsvector/GIN query branch"
Task: "Extend review.rs and outcome.rs with Postgres-dialect branches"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1 (Setup) + Phase 2 (Foundational)
2. Complete Phase 3 (US1 — genuine concurrency)
3. **STOP and VALIDATE**: quickstart.md Scenario 1 passes independently
4. This alone already proves Constitution Principle IV end-to-end on a real
   multi-connection backend, closing the gap Phase 2's single-connection SQLite test could
   only approximate

### Incremental Delivery

1. Setup + Foundational → foundation ready
2. US1 (genuine concurrency) → validate independently (MVP for this phase)
3. US2 (migration) → validate independently — this is what actually lets an existing
   deployment adopt the new backend without starting over
4. US3 (offline-mode regression check) → validate independently — confirms the embedded
   path is still a complete, unregressed fallback

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Developer A: US1 (per-service Postgres-dialect branches); Developer B: US2 (`migrate`)
   — mostly parallel, since US2 only depends on Foundational's generalized `rebuild`, not
   on US1's retrieval/consolidation branches
3. Either developer picks up US3 (regression confirmation) once US1's branches land

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- No change to Phase 1/2's rule-based logic anywhere in this feature — only the
  persistence/query layer gains a second dialect (Constitution Principle III unaffected)
- Verify tests fail (or skip cleanly, for Postgres-dependent tests with no reachable
  instance) before implementing
- Commit after each task or logical group
- Stop at any checkpoint to validate a story independently
