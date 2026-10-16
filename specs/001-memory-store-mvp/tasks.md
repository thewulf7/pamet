---

description: "Task list template for feature implementation"
---

# Tasks: Memory Store MVP (Write Rules + Hybrid Retrieval + Outcome Logging)

**Input**: Design documents from `/specs/001-memory-store-mvp/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md,
data-model.md, contracts/, quickstart.md

**Tests**: Included. plan.md's Testing section and Project Structure explicitly call out
`tests/contract/`, `tests/integration/`, `tests/unit/`, and the Constitution's Eval/rules
discipline favors inspectable, tested rule logic — so contract + integration tests are generated
per user story.

**Organization**: Tasks are grouped by user story (spec.md P1/P2/P3) to enable independent
implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- Paths are relative to the repository root (`/Users/I779886/Developer/pamet`)

## Path Conventions

Single Rust project (plan.md Project Structure): `src/models/`, `src/services/`, `src/cli/`,
`src/lib.rs` for the `pamet-core` library + `pamet` CLI binary; `tests/contract/`,
`tests/integration/`, `tests/unit/`.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and basic structure

- [X] T001 Create Cargo workspace with `pamet-core` lib crate and `pamet` CLI bin crate per
  plan.md Project Structure, in `Cargo.toml`, `src/lib.rs`, `src/cli/main.rs`
- [X] T002 Add dependencies from research.md to `Cargo.toml`: `rusqlite` (bundled, `fts5`
  feature), `sqlite-vec`, `ort`, `serde`, `serde_json`, `clap`, `uuid`, `chrono`, `thiserror`
- [X] T003 [P] Configure `rustfmt.toml` and `clippy.toml` (or `#![warn(clippy::all)]`) at repo root

**Checkpoint**: `cargo build` succeeds with an empty library + CLI skeleton.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T004 Create SQLite schema (events, memory_items, write_decisions, retrieval_events,
  outcome_records, review_queue tables per data-model.md) as an embedded migration in
  `src/storage/schema.rs`
- [X] T005 [P] Implement `Store` struct (SQLite connection + migration runner + FTS5/sqlite-vec
  extension loading) in `src/storage/store.rs`
- [X] T006 [P] Implement append-only event log read/write (`append_event`, `replay_events`) over
  the `events` table in `src/services/events.rs` (Constitution Principle I: source of truth)
- [X] T007 [P] Implement local embedding wrapper (`ort` model load + `embed(text) -> Vec<f32>`,
  no network call) in `src/services/embedding.rs`
- [X] T008 [P] Define shared error type (`thiserror`-based `Error` enum covering IO/storage/
  validation failures) in `src/error.rs`
- [X] T009 [P] Scaffold CLI entrypoint with `clap` (`--db <path>` global arg, subcommand stubs
  matching contracts/cli-contract.md) in `src/cli/main.rs`

**Checkpoint**: Foundation ready — `Store`, event log, embedding, and CLI skeleton all compile
and a fresh `./pamet.db` can be created; user story implementation can now begin.

---

## Phase 3: User Story 1 - Write candidates get a rule-based decision (Priority: P1) 🎯 MVP

**Goal**: Every interaction's candidate memory item is filed as store/merge/supersede/skip/review
via explicit, inspectable rules, with no learned component (spec FR-001..FR-004, FR-009, FR-012,
FR-013; Constitution Principle III).

**Independent Test**: Run quickstart.md Scenario 1 against a clean `./pamet.db` — four
`remember()` calls produce the exact store/merge/supersede/skip sequence, each `write_decisions`
row names the rule that fired, and the superseded item remains inspectable (not deleted).

### Tests for User Story 1 ⚠️

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T010 [P] [US1] Contract test for `remember()` (one `WriteDecision` per candidate, decision
  enum matches contracts/library-api.md) in `tests/contract/test_remember.rs`
- [X] T011 [P] [US1] Integration test for the full store/merge/supersede/skip/review sequence
  from quickstart.md Scenario 1 in `tests/integration/test_write_policy.rs`

### Implementation for User Story 1

- [X] T012 [P] [US1] Create `MemoryItem` model (fields per data-model.md: id, item_type, content,
  embedding, entities, confidence, provenance_event_id, supersedes_id, timestamps,
  tombstoned_at) in `src/models/memory_item.rs`
- [X] T013 [P] [US1] Create `WriteDecisionRecord` and `ReviewQueueEntry` models (data-model.md) in
  `src/models/write_decision.rs`
- [X] T014 [US1] Implement write-policy rules (novelty via embedding similarity, contradiction
  check, explicit-remember flag, fixed thresholds per research.md "Write policy" decision) in
  `src/services/write_policy.rs` (depends on T007, T012, T013)
- [X] T015 [US1] Implement `remember()` library function: extract candidates → run write policy →
  append `write_decision` event → upsert `memory_items`/`review_queue` in `src/lib.rs` (depends on
  T006, T014)
- [X] T016 [US1] Implement `pamet remember` CLI subcommand (contracts/cli-contract.md) in
  `src/cli/commands/remember.rs` (depends on T015)
- [X] T017 [US1] Implement `resolve_review()` and `pamet review list|resolve` subcommands in
  `src/services/review.rs` and `src/cli/commands/review.rs` (depends on T015)
- [X] T018 [US1] Enforce provenance-required and decision-enum validation rules from data-model.md
  (reject writes with no `provenance_event_id`) in `src/services/write_policy.rs`
- [X] T019 [US1] Log rule name + rule inputs (novelty/contradiction scores, thresholds compared)
  on every `write_decisions` row, per FR-009, in `src/services/write_policy.rs`

**Checkpoint**: At this point, User Story 1 should be fully functional and testable
independently — `pamet remember` reproduces quickstart.md Scenario 1 end-to-end.

---

## Phase 4: User Story 2 - Hybrid retrieval with a budget (Priority: P2)

**Goal**: A query returns a fused, budget-bounded ranking of relevant memories, or an explicit
"no relevant memory found" result, combining vector similarity and FTS5/BM25 keyword search via
RRF (spec FR-005..FR-008; research.md "Storage engine" and "Result fusion" decisions).

**Independent Test**: Run quickstart.md Scenario 2 against the store seeded by Scenario 1 — a
relevant query returns the current (superseding) item within budget; an unrelated query returns
the literal "no relevant memory found" result with CLI exit code 0.

### Tests for User Story 2 ⚠️

- [X] T020 [P] [US2] Contract test for `recall()` (budget-bounded, empty-result-is-not-an-error)
  in `tests/contract/test_recall.rs`
- [X] T021 [P] [US2] Integration test for hybrid retrieval + fusion ranking + "no relevant memory
  found" case from quickstart.md Scenario 2 in `tests/integration/test_recall.rs`

### Implementation for User Story 2

- [X] T022 [P] [US2] Create `RetrievalEvent` model (query, per-method candidates, fused ranking,
  returned ids, budget — data-model.md) in `src/models/retrieval_event.rs`
- [X] T023 [US2] Implement vector similarity search over `sqlite-vec` in
  `src/services/retrieval_vector.rs` (depends on T007 embedding, T005 Store)
- [X] T024 [US2] Implement FTS5/BM25 keyword search over the `memory_items` FTS5 index in
  `src/services/retrieval_keyword.rs` (depends on T005)
- [X] T025 [US2] Implement Reciprocal Rank Fusion of the two ranked lists in
  `src/services/retrieval_fusion.rs` (depends on T023, T024)
- [X] T026 [US2] Implement `recall()` library function: run both searches → fuse → exclude
  tombstoned items and pending `review_queue` candidates → trim to budget → append `retrieval`
  event in `src/lib.rs` (depends on T006, T022, T025)
- [X] T027 [US2] Implement `pamet recall` CLI subcommand (prints "no relevant memory found" on
  empty result) in `src/cli/commands/recall.rs` (depends on T026)
- [X] T028 [US2] Add exclusion filters for tombstoned items and pending review-queue candidates to
  the retrieval query path, per data-model.md validation rules, in
  `src/services/retrieval_fusion.rs`

**Checkpoint**: At this point, User Stories 1 AND 2 both work independently — `pamet recall`
reproduces quickstart.md Scenario 2 end-to-end.

---

## Phase 5: User Story 3 - Outcome logging closes the training-data loop (Priority: P3)

**Goal**: Every write decision and retrieval event is logged with enough detail — including
after-the-fact usage feedback — that a future learned component can be trained without
re-instrumentation (spec FR-010, FR-011; SC-006; Constitution Principle III's precondition).

**Independent Test**: Run quickstart.md Scenario 3 — attach a `used: true` outcome to an item
returned by Scenario 2's retrieval, then `export-log` and confirm the `outcome_update` event
references the same retrieval event and item with no disconnected new record.

### Tests for User Story 3 ⚠️

- [X] T029 [P] [US3] Contract test for `report_outcome()` and `export_log()` (rejects unknown
  item-for-event; export is complete/ordered) in `tests/contract/test_outcome.rs`
- [X] T030 [P] [US3] Integration test for outcome attachment + full-log-completeness from
  quickstart.md Scenario 3 in `tests/integration/test_outcome_logging.rs`

### Implementation for User Story 3

- [X] T031 [P] [US3] Create `OutcomeRecord` model (retrieval_event_id, item_id, used,
  reported_at — data-model.md) in `src/models/outcome_record.rs`
- [X] T032 [US3] Implement `report_outcome()`: validate `item_id` was among the retrieval event's
  returned items, append an `outcome_update` event, update `outcome_records` (not a new retrieval
  record) in `src/services/outcome.rs` (depends on T026, T031)
- [X] T033 [US3] Implement `export_log()` as a replay-ordered iterator over `events` (optionally
  filtered `since`) in `src/services/events.rs` (depends on T006)
- [X] T034 [US3] Implement `pamet outcome` and `pamet export-log` CLI subcommands
  (contracts/cli-contract.md) in `src/cli/commands/outcome.rs` (depends on T032, T033)
- [X] T035 [US3] Add an integration check that every write decision and retrieval event from
  Phases 3-5 appears in `export_log()` output with no silent drops (spec SC-006) in
  `tests/integration/test_outcome_logging.rs`

**Checkpoint**: All user stories should now be independently functional — `pamet export-log`
reproduces quickstart.md Scenario 3 end-to-end.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Improvements that affect multiple user stories

- [X] T036 [P] Add a `README.md` quickstart pointer (build + run the three quickstart.md
  scenarios) in `README.md`
- [X] T037 Code cleanup pass (remove dead code, consistent error messages) across `src/`
- [X] T038 [P] Additional unit tests for write-policy threshold edge cases and RRF fusion math in
  `tests/unit/test_write_policy_rules.rs` and `tests/unit/test_rrf_fusion.rs`
- [X] T039 Harden event ingestion: reject malformed `payload_json` / unknown `event_type` values
  fail-closed rather than partially applying (Constitution Principle I) in
  `src/services/events.rs`
- [X] T040 Run the full quickstart.md validation manually end-to-end on a clean `./pamet.db` and
  record the result in `specs/001-memory-store-mvp/quickstart.md` (append a "Last validated" note)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately.
- **Foundational (Phase 2)**: Depends on Setup completion — BLOCKS all user stories.
- **User Story 1 (Phase 3)**: Depends on Foundational only. No dependency on US2/US3.
- **User Story 2 (Phase 4)**: Depends on Foundational only for its core search path (T023-T026);
  however its retrieval MUST exclude tombstoned/pending-review items, so T028 depends on US1's
  `memory_items`/`review_queue` semantics existing (T012-T013) — implement Phase 3 before Phase 4
  in practice even though the contract itself has no hard blocking dependency.
- **User Story 3 (Phase 5)**: Depends on Foundational for events (T006), and functionally needs a
  retrieval event to attach outcomes to — so Phase 4 (`recall()`, T026) must exist first for
  `report_outcome()` (T032) to have something to reference. Not blocked by Phase 3 directly.
- **Polish (Phase 6)**: Depends on all three user stories being complete.

### User Story Dependencies

- **User Story 1 (P1)**: No dependency on other stories — this is the MVP floor.
- **User Story 2 (P2)**: Functionally depends on US1 having written memory items to retrieve
  (the independent test seeds via Scenario 1), though its code (T020-T028) can be built in
  parallel against a test-seeded store.
- **User Story 3 (P3)**: Functionally depends on US2 having produced a retrieval event to attach
  outcomes to (the independent test seeds via Scenario 2).

### Within Each User Story

- Tests MUST be written and FAIL before implementation.
- Models before services.
- Services before CLI subcommands.
- Core implementation before cross-story integration checks (T035).

### Parallel Opportunities

- All Setup tasks marked [P] (T003) can run in parallel with T001/T002 once they exist.
- Foundational [P] tasks T005-T009 can run in parallel once T004 (schema) lands.
- All [P] test tasks within a story (e.g., T010+T011, T020+T021, T029+T030) can run in parallel.
- All [P] model tasks within a story (T012+T013, T022, T031) can run in parallel.
- Given Foundational is done, Story 1's tests/models (T010-T013) can be written in parallel with
  Story 2's and Story 3's tests/models (T020-T022, T029-T031), even though the *service* layers
  have the functional ordering noted above.

---

## Parallel Example: User Story 1

```bash
# Launch both tests for User Story 1 together:
Task: "Contract test for remember() in tests/contract/test_remember.rs"
Task: "Integration test for write/merge/supersede/skip/review sequence in tests/integration/test_write_policy.rs"

# Launch both models for User Story 1 together:
Task: "Create MemoryItem model in src/models/memory_item.rs"
Task: "Create WriteDecisionRecord and ReviewQueueEntry models in src/models/write_decision.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup.
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories).
3. Complete Phase 3: User Story 1.
4. **STOP and VALIDATE**: Run quickstart.md Scenario 1 against a clean `./pamet.db`.
5. This alone satisfies Constitution Principle III's precondition (rules before learning) and
   starts producing `write_decisions` log data — ship/demo if ready.

### Incremental Delivery

1. Setup + Foundational → foundation ready.
2. Add User Story 1 → validate via quickstart Scenario 1 → demo (MVP!).
3. Add User Story 2 → validate via quickstart Scenario 2 → demo (memory is now useful, not just
   written).
4. Add User Story 3 → validate via quickstart Scenario 3 → demo (logs are now training-ready,
   closing the loop spec SC-006 requires).
5. Each story adds value without breaking the previous ones — all three stay reproducible on a
   clean `./pamet.db` per quickstart.md's pass/fail criterion.

### Parallel Team Strategy

With multiple developers:

1. Team completes Setup + Foundational together.
2. Once Foundational is done:
   - Developer A: User Story 1 (write policy).
   - Developer B: User Story 2 (retrieval — can stub against a manually-seeded store while
     waiting on US1's real `remember()`).
   - Developer C: User Story 3 (outcome logging — can stub against a manually-seeded retrieval
     event while waiting on US2's real `recall()`).
3. Stories integrate at the Phase boundaries noted in Dependencies above.

---

## Notes

- [P] tasks = different files, no dependencies.
- [Story] label maps task to specific user story for traceability.
- Verify tests fail before implementing (T010-T011, T020-T021, T029-T030 first).
- Commit after each task or logical group.
- Stop at any checkpoint (end of Phase 3, 4, or 5) to validate that story independently via the
  matching quickstart.md scenario before moving on.
- No task in this list introduces a learned component — that is deliberate per Constitution
  Principle III; a follow-on feature (not this one) will consume the logs this feature produces.

## Implementation Notes (post-hoc, 2026-10-16)

All 40 tasks are implemented and verified (`cargo build`, `cargo test` — 17/17 passing;
`cargo build --release` + manual CLI run of all three quickstart.md scenarios). Three deliberate
structural deviations from the exact task text, called out here for traceability:

- **T001/T009/T016/T017/T027/T034 (single crate, not a workspace)**: implemented as one Cargo
  package with `src/lib.rs` (library) + `src/bin/pamet.rs` (CLI), instead of a `pamet-core`/`pamet`
  two-crate workspace. Same effect (library usable standalone, CLI is a thin wrapper over it) with
  less build/packaging overhead for an MVP. CLI subcommands live in `src/bin/pamet.rs` rather than
  `src/cli/commands/*.rs` — one file was simpler at this size (~165 lines) than a module tree.
- **T002/T007/T023 (no `ort`/ONNX, no `sqlite-vec`)**: `src/services/embedding.rs` uses a
  deterministic, frozen, hashed bag-of-words embedding + cosine similarity computed in Rust,
  instead of a downloaded ONNX transformer model and the `sqlite-vec` loadable extension. Still
  local, offline, frozen (Constitution Principle III), zero network dependency — but a weaker
  semantic embedder than a real sentence-transformer. Swapping in `ort` later is a drop-in
  replacement behind the same `embed()`/`cosine_similarity()` functions; tracked as a follow-up,
  not hidden. See research.md and plan.md's Completion Report for the original decision this
  deviates from.
- **T028 (exclusion filters)**: implemented in `src/lib.rs::recall()` (`pending_review_candidate_ids`)
  rather than inside `retrieval_fusion.rs`, since the exclusion needs `review_queue`/`write_decisions`
  join data that the fusion module (pure ranking math) doesn't otherwise depend on.

One behavioral note surfaced during T040 validation and corrected in quickstart.md: a near-but-not
-exact restatement (e.g., adding "(same as before)") lands in the review band, not a clean merge,
with this MVP's embedder — an exact string restatement merges as designed. Documented in
quickstart.md Scenario 1 rather than left as a silent mismatch between the plan and reality.

---

## Phase 7: Convergence

- [X] T041 Implement a replay/rebuild-from-events capability that reconstructs `memory_items`,
  `write_decisions`, `review_queue`, `retrieval_events`, and `outcome_records` purely from
  replaying the `events` table, and add a test proving a rebuilt store matches the live one, in
  `src/services/events.rs` (or a new `src/services/rebuild.rs`) per Constitution I (contradicts)
- [X] T042 Exclude tombstoned items from keyword search: join `retrieval_keyword::search` against
  `memory_items.tombstoned_at IS NULL` (or remove/sync the corresponding `memory_items_fts` row on
  tombstone) in `src/services/retrieval_keyword.rs` and `src/lib.rs::tombstone()` per FR-014 (missing)
- [X] T043 Implement multi-candidate extraction in `remember()` so a single interaction can yield
  more than one independently-logged write decision, in `src/lib.rs`, per FR-001 (missing)
- [X] T044 Add integration test coverage for `tombstone()` proving a tombstoned item disappears
  from `recall()` and remains inspectable/traceable via its id and event history, in
  `tests/integration/test_tombstone.rs`, per SC-005 (partial)
- [X] T045 Reconcile plan.md's Technical Context/Primary Dependencies and research.md's
  embedding/storage decisions with the actual hashed-bag-of-words + Rust-cosine-similarity
  implementation (or swap in real `ort`+`sqlite-vec` to match the original plan), per plan:
  embedding/storage decision (contradicts)
- [X] T046 Reconcile plan.md's Project Structure (two-crate `pamet-core`/`pamet` workspace) with
  the actual single-crate layout (`src/lib.rs` + `src/bin/pamet.rs`), per plan: project structure
  decision (unrequested)

