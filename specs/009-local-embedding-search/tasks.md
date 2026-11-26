# Tasks: Real Semantic Memory Search

**Input**: Design documents from `/specs/009-local-embedding-search/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/cli-contract.md, quickstart.md (all present)

**Tests**: Included — matches every prior Pamet feature's established practice of
real tests + real measured verification, consistent with this feature's own
Success Criteria being explicitly measurable (SC-001 through SC-005), not just
descriptive.

**Organization**: Tasks are grouped by user story (spec.md: US1 P1, US2 P2, US3 P3)
to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

---

## Phase 1: Setup

**Purpose**: Dependency changes shared by every story — isolated and
regression-tested on their own before any feature code touches them.

- [X] T001 Add `sqlite-vec`, `ort`, `tokenizers` to `Cargo.toml`; bump the pinned
  `rusqlite` version to one where `auto_extension`/`loadable_extension` is stable
  (research.md Decision 1) — exact version chosen by actually trying it, not
  guessed. **Resolved: rusqlite 0.32, WITHOUT the `loadable_extension` feature**
  — enabling it breaks ordinary `Connection::open` in the same binary (confirmed
  real regression in T002); `auto_extension`/`register_auto_extension` compile
  and work fine without it (confirmed via a standalone probe).
- [X] T002 Full regression sweep (`cargo build`, `cargo test --release`) on the
  `rusqlite` version bump **alone**, before any new feature code is added —
  isolates dependency-upgrade risk from this feature's own risk. **Caught a
  real regression**: enabling rusqlite's `loadable_extension` feature (which
  docs.rs's gating note on `auto_extension` suggested was needed) breaks
  `cargo test` with `SQLite API not initialized or SQLite feature omitted` —
  confirmed via rusqlite maintainer @gwenn (GitHub issue #1693): that feature
  is for building a `.so` loadable extension, the opposite direction; the fix
  is `auto_extension` WITHOUT `loadable_extension`. Full suite green after the
  fix (0 failures, rusqlite 0.32.1 in Cargo.lock).

**Checkpoint**: Dependencies in place and proven not to regress anything that
already exists.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure every user story needs. No story work starts
until this phase is done.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [ ] T003 `src/storage/schema.rs`: add the `memory_items_vec` `vec0` virtual
  table, the `embedding_models` table, and the `embedding_model_id` column on
  `memory_items` (data-model.md).
- [ ] T004 `src/services/events.rs`: add `"embedding_backfilled"` to
  `VALID_EVENT_TYPES`.
- [ ] T005 `src/storage/store.rs`: register `sqlite-vec` via
  `rusqlite::auto_extension::register_auto_extension` at connection-open time
  (research.md Decision 1's exact transmute pattern), so `vec0` is available on
  every opened connection without a separate load step.
- [ ] T006 `src/services/embedding.rs`: replace the hashed bag-of-words `embed()`
  with an `ort`/ONNX + `tokenizers` implementation — same function signature
  (`embed(text: &str) -> Vec<f32>`), now returning the real 384-dim MiniLM
  vector; resolves the model/tokenizer file paths from the local model cache
  directory (research.md Decision 3) and fails clearly (FR-008) if they're
  missing, never silently falling back.
- [ ] T007 `pamet embedding fetch-model` CLI subcommand in `src/bin/pamet.rs`:
  downloads the model + tokenizer config into the cache directory, verifies a
  checksum, idempotent on re-run (contracts/cli-contract.md).

**Checkpoint**: Foundation ready — `embed()` produces real vectors, `vec0` is
queryable, the model can be fetched. User story implementation can now begin.

---

## Phase 3: User Story 1 - Recall finds memories by meaning (Priority: P1) 🎯 MVP

**Goal**: Recall returns semantically related memories for paraphrase/synonym
queries, not just exact-word matches, without regressing on queries that
already work today.

**Independent Test**: Store a memory, query with a genuine paraphrase (not the
same words), confirm it's returned; re-run the project's existing eval probe
set and confirm a measurable improvement with no regressions.

### Tests for User Story 1

- [ ] T008 [P] [US1] Unit tests for the new `embedding.rs` (determinism — same
  text always produces the same vector; correct 384-dim output; empty/
  degenerate text handled per spec.md's edge cases) in
  `tests/unit/test_embedding.rs`.

### Implementation for User Story 1

- [ ] T009 [US1] Rewrite `src/services/retrieval_vector.rs`'s `search()` to
  query `memory_items_vec` via `sqlite-vec`'s native KNN instead of scanning
  every `memory_items` row and scoring in Rust.
- [ ] T010 [US1] Update `src/services/write_policy.rs`'s candidate scoring to
  call the new `embedding::embed()`/`cosine_similarity()` over real vectors
  (feature 005's keyword-index prefilter stage is unchanged — only the scoring
  math underneath it now operates on real embeddings instead of hashed-BoW).
- [ ] T011 [US1] Wire `src/lib.rs`'s `remember()` to write the
  `memory_items_vec` row and set `memory_items.embedding_model_id` on every new
  write (depends on T003, T006).
- [ ] T012 [US1] Integration test: a stored memory is found by a genuine
  paraphrase/synonym query but an unrelated memory is not ranked ahead of it,
  in `tests/integration/test_vector_search.rs` (depends on T009, T011).
- [ ] T013 [US1] Run the project's existing eval harness (`pamet eval` /
  `eval_harness.rs` probes) before and after this story's changes; record the
  real measured numbers in `spec.md`'s Results section for SC-001 — not
  estimated.

**Checkpoint**: User Story 1 fully functional and independently testable — new
writes get real semantic recall.

---

## Phase 4: User Story 2 - Recall stays fast as the memory store grows (Priority: P2)

**Goal**: Query latency does not grow in proportion to how many memories are
stored.

**Independent Test**: Measure recall latency against stores of increasing real
size and confirm it does not scale with item count.

### Tests for User Story 2

- [ ] T014 [US2] Real latency measurement across store sizes (hundreds vs. tens
  of thousands of real items — a copy of the project's own live store, never
  the live file directly) added to `tests/integration/test_vector_search.rs`
  (depends on T009; needs US3's backfill, T019-T020, to have real vectors to
  query at scale — note this cross-story dependency explicitly, see
  Dependencies section below).

### Implementation for User Story 2

- [ ] T015 [US2] If T014's real measurement surfaces a problem, tune `vec0`'s
  configuration (distance metric, any relevant indexing parameters) — this task
  is conditional on T014's actual finding, not assumed necessary in advance.
- [ ] T016 [US2] Record the real measured numbers for SC-002 in `spec.md`'s
  Results section.

**Checkpoint**: User Stories 1 and 2 both independently verified.

---

## Phase 5: User Story 3 - Existing memories keep working after the upgrade (Priority: P3)

**Goal**: Every memory stored before this feature shipped remains fully
findable afterward, with the real migration time known, not estimated.

**Independent Test**: Take a full copy of the real existing store, run the
upgrade against the copy, confirm every previously-stored memory is still
findable with zero data loss.

### Tests for User Story 3

- [ ] T017 [US3] Integration test: backfill idempotency (safe to re-run,
  already-backfilled items skipped) and zero-item-loss verification in
  `tests/integration/test_embedding_backfill.rs`.
- [ ] T018 [US3] Replay-equivalence test — a replayed `memory_items_vec` row is
  byte-for-byte identical to the source, same verification method
  `tests/integration/test_gram_replay.rs` established for feature 008 — in
  `tests/integration/test_embedding_backfill.rs` or a dedicated
  `test_embedding_replay.rs`.

### Implementation for User Story 3

- [ ] T019 [P] [US3] `src/services/embedding_backfill.rs`: batched,
  idempotent backfill logic — finds every `memory_items` row with
  `embedding_model_id IS NULL` (or stale), generates its real embedding, writes
  the `memory_items_vec` row, and emits an `embedding_backfilled` event per
  item (data-model.md).
- [ ] T020 [US3] `pamet embedding backfill` CLI subcommand in
  `src/bin/pamet.rs`: batched progress output + a final summary line
  (`backfilled=<n> already_current=<n> errors=<n> elapsed=<duration>`),
  matching the `pamet consolidate` convention (contracts/cli-contract.md;
  depends on T019).
- [ ] T021 [US3] `src/services/rebuild.rs`: replay handler for
  `embedding_backfilled` — re-inserts the `memory_items_vec` row and sets
  `embedding_model_id` verbatim from the event payload (pure re-insert, no
  re-running the ONNX model during replay — Principle I, data-model.md).
- [ ] T022 [US3] Real backfill run against a **copy** of the project's own live
  ~25,000-item store (`~/.hermes/pamet/memory.db`, never the live file
  directly); measure and record the real elapsed time for SC-003/SC-004/FR-010
  in `spec.md`'s Results section — same discipline feature 008 used before its
  own live-store schema migration.

**Checkpoint**: All three user stories independently functional and verified
against real data.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Final regression, documentation, and the live-store rollout.

- [ ] T023 [P] `cargo fmt` + `cargo clippy --all-targets` — zero new warnings in
  any file this feature touches.
- [ ] T024 Full regression sweep: `cargo build` (default) and
  `cargo test --release` (entire existing suite, not just this feature's new
  tests) — confirm nothing else broke.
- [ ] T025 Run every scenario in `specs/009-local-embedding-search/quickstart.md`
  for real against a built binary; document actual output, not expected output.
- [ ] T026 Update `spec.md`'s **Status** field and add a **Results** section
  with the real SC-001 through SC-005 numbers gathered in T013/T016/T022.
- [ ] T027 [P] Update `README.md`/`docs/GUIDE.md`: document the new
  `sqlite-vec`/`ort`/`tokenizers` dependencies, the `pamet embedding fetch-model`
  setup step, and the new CLI commands.
- [ ] T028 Apply the now-verified-safe backfill to the **live**
  `~/.hermes/pamet/memory.db` (only after T022 has proven it safe on a copy);
  confirm zero loss the same way feature 008 confirmed its live-db schema
  migration (direct row-count/`embedding_model_id IS NULL` check before/after,
  not just trusting the command's own summary line).

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately.
- **Foundational (Phase 2)**: Depends on Setup — BLOCKS all user stories.
- **User Story 1 (Phase 3)**: Depends on Foundational only. The MVP.
- **User Story 2 (Phase 4)**: Depends on Foundational (T009) **and** needs real
  data at scale to measure against (T014 is far more meaningful once User
  Story 3's backfill, T019-T020, has run against a real-size copy — note this
  is a measurement dependency, not a hard code dependency: T014 can run against
  a synthetic large fixture if US3 isn't done yet, but the real, representative
  measurement should be re-run once US3's real backfill exists).
- **User Story 3 (Phase 5)**: Depends on Foundational only (T006's real `embed()`
  is what the backfill actually calls per item).
- **Polish (Phase 6)**: Depends on all three stories being complete.

### Parallel Opportunities

- T008 (US1 unit tests) can run in parallel with T009-T010 (different files).
- T019 (US3 backfill service) can run in parallel with T009-T013 (US1) and
  T014-T016 (US2) — different files, no shared dependency beyond the
  Foundational phase.
- T023 and T027 (Polish) can run in parallel with each other.

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1 (Setup) and Phase 2 (Foundational).
2. Complete Phase 3 (User Story 1).
3. **STOP and VALIDATE**: run the eval harness, confirm SC-001's real
   improvement before continuing — this is the entire point of the feature; if
   it doesn't show up here, reconsider before investing in US2/US3.

### Incremental Delivery

1. Setup + Foundational → real `embed()` + `vec0` working.
2. User Story 1 → semantic recall works for new writes → validate against eval
   harness (MVP).
3. User Story 3 → existing ~25,000 real items backfilled and verified lossless.
4. User Story 2 → re-measure latency now that real data exists at real scale
   (see the Dependencies note above on why this ordering is more meaningful
   than running US2 before US3).
5. Polish → full regression, docs, live rollout.

---

## Phase 7: Convergence

Appended by `/speckit-converge` — assessed against spec.md/plan.md/tasks.md as
of the current codebase state. Ordered CRITICAL/HIGH first.

- [ ] T029 Call `register_sqlite_vec_once()` before `Connection::open()` in
  `Store::open()` and `Store::open_in_memory()` (not inside `init_sqlite`,
  which only ever sees an already-opened connection — SQLite's auto-extension
  hook only applies to connections opened *after* registration) in
  `src/storage/store.rs` per T005 (CRITICAL — contradicts: `Store::open()`
  currently fails unconditionally with `no such module: vec0`, confirmed via
  a live probe and the full test suite currently failing on the working tree).
- [ ] T030 Add `embedding_backfilled` to `migrations.rs`'s
  `NEW_EVENTS_TABLE_SQL` and its staleness check (`sql.contains(...)`) per T004
  (missing — reproduces feature 008's own discovered bug class for this
  feature's new event type).
- [ ] T031 Add an idempotent `ALTER TABLE memory_items ADD COLUMN
  embedding_model_id` migration (guarded by a `PRAGMA table_info` existence
  check) in `src/storage/migrations.rs` per T003/data-model.md (missing — the
  inline `CREATE TABLE IF NOT EXISTS` only covers brand-new databases).
- [ ] T032 Replace `src/services/embedding.rs`'s hashed bag-of-words `embed()`
  with the real frozen ort/ONNX MiniLM embedder per T006/research.md Decisions
  2-3 (missing).
- [ ] T033 Implement `pamet embedding fetch-model` in `src/bin/pamet.rs` per
  T007/contracts/cli-contract.md (missing).
- [ ] T034 Rewrite `src/services/retrieval_vector.rs` to query
  `memory_items_vec` via `sqlite-vec` KNN instead of the brute-force
  `embedding_json` scan per T009/FR-003/US2 (missing).
- [ ] T035 Update `src/services/write_policy.rs`'s candidate scoring to the
  new real embedder per T010/FR-001 (missing — depends on T032).
- [ ] T036 Wire `src/lib.rs`'s `remember()` to write a `memory_items_vec` row +
  `embedding_model_id` on every new write per T011/data-model.md (missing).
- [ ] T037 Implement User Story 3 (P3) in full — `src/services/
  embedding_backfill.rs`, the `pamet embedding backfill` CLI subcommand, and
  `rebuild.rs`'s `embedding_backfilled` replay handler — per T017-T021
  (missing — US3 is entirely unimplemented).
- [ ] T038 Write the feature's test suite (`tests/unit/test_embedding.rs`,
  `tests/integration/test_vector_search.rs`,
  `tests/integration/test_embedding_backfill.rs`) and run the real SC-001/
  SC-002 measurements per T008/T012-T016/T022 (missing — no new tests exist
  yet for this feature).
- [ ] T039 Confirm `ort`/`tokenizers`/`zerocopy`'s pinned versions in
  `Cargo.toml` actually fit the real embedding code once T032 lands; adjust if
  the API doesn't match what was guessed at dependency-add time (partial —
  these versions got a clean `cargo build` but are unexercised by any real
  embedding code yet).
- [ ] T040 Confirm `zerocopy` is actually required by `ort`'s tensor/IoBinding
  handling once T032 lands, or remove it from `Cargo.toml` — it was added
  without a research.md decision or tasks.md task justifying it (unrequested).
