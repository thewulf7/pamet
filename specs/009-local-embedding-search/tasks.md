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

- [X] T003 `src/storage/schema.rs`: add the `memory_items_vec` `vec0` virtual
  table, the `embedding_models` table, and the `embedding_model_id` column on
  `memory_items` (data-model.md). **Pre-existing-DB migration gap found by
  `/speckit-converge` and fixed as T031.**
- [X] T004 `src/services/events.rs`: add `"embedding_backfilled"` to
  `VALID_EVENT_TYPES`. **Pre-existing-DB migration gap found by
  `/speckit-converge` and fixed as T030.**
- [X] T005 `src/storage/store.rs`: register `sqlite-vec` via
  `rusqlite::auto_extension::register_auto_extension` at connection-open time
  (research.md Decision 1's exact transmute pattern), so `vec0` is available on
  every opened connection without a separate load step. **Wiring bug (called
  too late, inside `init_sqlite` instead of before `Connection::open`) found by
  `/speckit-converge` and fixed as T029.**
- [X] T006 `src/services/embedding.rs`: replace the hashed bag-of-words `embed()`
  with an `ort`/ONNX + `tokenizers` implementation — same function signature
  (`embed(text: &str) -> Vec<f32>`), now returning the real 384-dim MiniLM
  vector; resolves the model/tokenizer file paths from the local model cache
  directory (research.md Decision 3) and fails clearly (FR-008) if they're
  missing, never silently falling back. **Signature changed to
  `Result<Vec<f32>>`** (not bare `Vec<f32>` as originally sketched — FR-008's
  "fail clearly" is better served by a real `Result` than a panic, matching
  this codebase's existing error-handling convention; updated all 9
  call sites in `src/` and `tests/`). Model: the exact artifact `ort`'s own
  official `sentence-transformers` example ships (cdn.pyke.io, paired
  `tokenizer.json`), not a generic HF export — already bakes mean-pooling
  into output[1] (see embedding.rs's module doc). **Verified end-to-end with
  the real downloaded model**: 384-dim output, correct semantic similarity
  ordering (sim(weather,sunny)=0.679 > sim(weather,stadium)=0.216),
  deterministic for identical input. Fixed 3 downstream test fixtures whose
  wording was calibrated to the old crude hashed-BoW embedder's coincidental
  scores and no longer landed in the write_policy threshold bands they were
  designed to exercise once a real semantic model scores them correctly
  (`test_consolidation.rs`'s contradiction fixture, `test_gram_replay.rs`'s
  synthetic cluster fixture, `test_eval_harness.rs`'s 1000-item scale
  fixture) — all re-verified against real measured similarity scores, not
  guessed. Loosened `test_write_policy_scale.rs`'s prefilter-overhead bound
  (20ms → 150ms) with a documented rationale: real ONNX inference has a real,
  unavoidable ~86ms/call cost no architecture change removes — this is the
  deliberate trade for correct semantic recall, not a regression.
- [X] T007 `pamet embedding fetch-model` CLI subcommand in `src/bin/pamet.rs`:
  downloads the model + tokenizer config into the cache directory, verifies a
  checksum, idempotent on re-run (contracts/cli-contract.md). **Downloads via
  a `curl` subprocess** (not an always-on `reqwest` dependency pulled into
  every other command that never touches the network) with real SHA256
  verification (new `sha2` dependency) — a corrupt/partial download is
  deleted and hard-errors, never silently accepted. Dispatched before
  `Store::open()` in `main()` (same precedent as `Migrate`) since it needs no
  `--db` at all. **Verified end-to-end for real**: fresh download (90MB
  model + tokenizer, real checksums match) and idempotent re-run (both files
  reported `already current`, no re-download).

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

- [X] T009 [US1] Rewrite `src/services/retrieval_vector.rs`'s `search()` to
  query `memory_items_vec` via `sqlite-vec`'s native KNN instead of scanning
  every `memory_items` row and scoring in Rust. **Real `vec0` `MATCH ... AND
  k = ...` query, `distance_metric=cosine` added to schema.rs's column def
  (defaults to L2 otherwise — caught via real sqlite-vec docs, not guessed);
  `zerocopy::IntoBytes` for the query-vector parameter binding (the official
  sqlite-vec Rust guide's own recommended pattern). Verified end-to-end**:
  3 real remembered facts, `recall("coffee machine problem")` correctly
  returns the 2 coffee/espresso items ranked first and excludes the unrelated
  revenue fact — genuine semantic KNN, not keyword matching.
- [X] T010 [US1] Update `src/services/write_policy.rs`'s candidate scoring to
  call the new `embedding::embed()`/`cosine_similarity()` over real vectors
  (feature 005's keyword-index prefilter stage is unchanged — only the scoring
  math underneath it now operates on real embeddings instead of hashed-BoW).
  **Automatic once T006 swapped `embed()`'s implementation** — no code change
  needed beyond the `?` already added for the `Result` signature change;
  verified via the full regression suite (write_policy's merge/contradiction
  tests all pass against real embeddings).
- [X] T011 [US1] Wire `src/lib.rs`'s `remember()` to write the
  `memory_items_vec` row and set `memory_items.embedding_model_id` on every new
  write (depends on T003, T006). **Added `embedding::ensure_embedding_model_row()`
  + `embedding::upsert_vector_row()` helpers; wired into `lib.rs::insert_item()`
  AND the two other real insert sites that write `memory_items` with an
  embedding — `consolidation.rs`'s summary-promotion insert and
  `rebuild.rs`'s two replay-handler inserts — so every path that creates an
  item with an embedding (not just live writes) populates `memory_items_vec`,
  not only the one `remember()` call site.** Verified end-to-end (same probe
  as T009).
- [ ] T012 [US1] Integration test: a stored memory is found by a genuine
  paraphrase/synonym query but an unrelated memory is not ranked ahead of it,
  in `tests/integration/test_vector_search.rs` (depends on T009, T011).
- [X] T013 [US1] Run the project's existing eval harness (`pamet eval` /
  `eval_harness.rs` probes) before and after this story's changes; record the
  real measured numbers in `spec.md`'s Results section for SC-001 — not
  estimated. **Real before/after measured via a git worktree at the pre-T006
  commit (still has the old hashed-BoW embedder) vs. current HEAD, same
  probe set (`tests/fixtures/eval_probes_v1.json`), both built `--release`:**
  - Before (hashed-BoW): `forgetting_abstention` 2/3, `general_precision`
    3/3, `new_fact_recall` 3/3, `update_supersession` 3/3 — **11/12 overall**.
  - After (real MiniLM): `forgetting_abstention` 2/3, `general_precision`
    3/3, `new_fact_recall` 3/3, `update_supersession` 1/3 — **9/12 overall**.
  - **This is a real regression, not an improvement** — `update_supersession`
    dropped from 3/3 to 1/3. Root cause confirmed (not guessed): the probe
    set's "Correction: ..." sentences are deliberately long and verbose
    (~40 words) vs. the ~5-word original fact they supersede. Real measured
    cosine similarity for one such pair = 0.594 — inside
    `write_policy::LOW_CONFIDENCE_THRESHOLD` (0.5) but below
    `MERGE_SIMILARITY_THRESHOLD` (0.85), so the real embedder correctly
    recognizes it as *related* but not similar enough to auto-supersede;
    `write_policy.rs`'s own logic (`max_similarity >= LOW_CONFIDENCE_THRESHOLD`
    → route to review, not auto-store/supersede) then sends the correction to
    the **review queue** instead of applying it, so `recall()` still returns
    the stale original fact. The old hashed-BoW embedder's cruder word-overlap
    scoring happened to push this same pair above 0.85 (coincidental, not a
    real signal) — see T041, a new task this finding surfaced.

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

- [X] T029 Call `register_sqlite_vec_once()` before `Connection::open()` in
  `Store::open()` and `Store::open_in_memory()` (not inside `init_sqlite`,
  which only ever sees an already-opened connection — SQLite's auto-extension
  hook only applies to connections opened *after* registration) in
  `src/storage/store.rs` per T005 (CRITICAL — contradicts: `Store::open()`
  currently fails unconditionally with `no such module: vec0`, confirmed via
  a live probe and the full test suite currently failing on the working tree).
  **Fixed and verified**: live probe now returns `OPEN OK`, full regression
  suite green on both SQLite and `postgres-backend`.
- [X] T030 Add `embedding_backfilled` to `migrations.rs`'s
  `NEW_EVENTS_TABLE_SQL` and its staleness check (`sql.contains(...)`) per T004
  (missing — reproduces feature 008's own discovered bug class for this
  feature's new event type). **Fixed in both the SQLite (`writable_schema`
  rewrite) and Postgres (`ALTER TABLE ... CONSTRAINT`) variants.**
- [X] T031 Add an idempotent `ALTER TABLE memory_items ADD COLUMN
  embedding_model_id` migration (guarded by a `PRAGMA table_info` existence
  check) in `src/storage/migrations.rs` per T003/data-model.md (missing — the
  inline `CREATE TABLE IF NOT EXISTS` only covers brand-new databases).
  **Done for both SQLite (`pragma_table_info` probe) and Postgres (`ADD
  COLUMN IF NOT EXISTS`, natively idempotent); verified idempotent via a
  double-open probe against the same real file.**
- [X] T032 Replace `src/services/embedding.rs`'s hashed bag-of-words `embed()`
  with the real frozen ort/ONNX MiniLM embedder per T006/research.md Decisions
  2-3 (missing). **Done — see T006's detailed notes above.**
- [X] T033 Implement `pamet embedding fetch-model` in `src/bin/pamet.rs` per
  T007/contracts/cli-contract.md (missing). **Done — see T007's detailed
  notes above.**
- [X] T034 Rewrite `src/services/retrieval_vector.rs` to query
  `memory_items_vec` via `sqlite-vec` KNN instead of the brute-force
  `embedding_json` scan per T009/FR-003/US2 (missing). **Done — see T009's
  detailed notes above.**
- [X] T035 Update `src/services/write_policy.rs`'s candidate scoring to the
  new real embedder per T010/FR-001 (missing — depends on T032). **Done —
  see T010's detailed notes above.**
- [X] T036 Wire `src/lib.rs`'s `remember()` to write a `memory_items_vec` row +
  `embedding_model_id` on every new write per T011/data-model.md (missing).
  **Done — see T011's detailed notes above.**
- [ ] T037 Implement User Story 3 (P3) in full — `src/services/
  embedding_backfill.rs`, the `pamet embedding backfill` CLI subcommand, and
  `rebuild.rs`'s `embedding_backfilled` replay handler — per T017-T021
  (missing — US3 is entirely unimplemented).
- [ ] T038 Write the feature's test suite (`tests/unit/test_embedding.rs`,
  `tests/integration/test_vector_search.rs`,
  `tests/integration/test_embedding_backfill.rs`) and run the real SC-001/
  SC-002 measurements per T008/T012-T016/T022 (missing — no new tests exist
  yet for this feature).
- [X] T039 Confirm `ort`/`tokenizers`/`zerocopy`'s pinned versions in
  `Cargo.toml` actually fit the real embedding code once T032 lands; adjust if
  the API doesn't match what was guessed at dependency-add time (partial —
  these versions got a clean `cargo build` but are unexercised by any real
  embedding code yet). **Confirmed: `ort = "2.0.0-rc.15"` and
  `tokenizers = "0.23"` work correctly with real embedding code (verified
  end-to-end against the real downloaded model). `Session::builder()`
  actually requires an explicit `&Environment` argument in this version
  (`ort::init().build()?`) — not the no-arg form a generic online example
  initially suggested; fixed via the real compiler error, not guessed.**
- [X] T040 Confirm `zerocopy` is actually required by `ort`'s tensor/IoBinding
  handling once T032 lands, or remove it from `Cargo.toml` — it was added
  without a research.md decision or tasks.md task justifying it (unrequested).
  **Removed — not needed. Also added `ndarray` (confirmed required by the
  real `ort` sentence-embedding usage pattern) and `sha2` (needed for
  `fetch-model`'s real checksum verification, T007).**
- [X] T041 **New, surfaced by T013's real before/after eval measurement —
  now resolved, see below.** `write_policy::LOW_CONFIDENCE_THRESHOLD` (0.5) and
  `MERGE_SIMILARITY_THRESHOLD` (0.85) were tuned against the old hashed-BoW
  embedder's similarity distribution (feature 005). The real MiniLM
  embedder's distribution is different enough that it caused a genuine
  regression on `update_supersession` (3/3 → 1/3, T013) — a verbose
  "Correction: ..." sentence scores real cosine similarity ~0.59 against the
  short original fact it should supersede, landing it in the
  review-gated zone instead of auto-superseding. Recalibrating these two
  thresholds for the real embedder is real tuning work with real tradeoffs
  (lowering them risks over-merging distinct facts elsewhere in the system)
  and needs its own deliberate pass: (1) measure real similarity
  distributions for a representative sample of true-duplicate,
  true-correction, and true-distinct pairs under the real embedder: (2)
  pick new thresholds from that real data, not guessed; (3) re-run the full
  eval harness + full `cargo test --release` to confirm no other probe/test
  regresses as a side effect; (4) update spec.md's Results section with the
  final before/after numbers once this lands. **Do not ship this feature as
  "done" without resolving T041** — SC-001 claims real embedding improves
  recall, and as currently tuned it demonstrably regresses one whole probe
  category.
  **RESOLVED.** Real measured similarities for 5 true-correction pairs
  (3 verbose, 2 terse), 2 true-near-duplicate pairs (no contradiction
  marker), and 3 true-distinct-topic pairs: distinct topics top out ~0.22;
  genuine corrections score 0.33-0.93 depending on phrasing terseness. Root
  cause confirmed: `CONTRADICTION_SIMILARITY_FLOOR`'s *polarity*
  (`sim < floor` = confident supersede) was a compensating hack for the old
  hashed-BoW embedder's unreliable scores — a real embedder's score is
  trustworthy, so "topically related (sim >= floor) + contradiction marker"
  is the actual confident-supersede signal. **Fixed**: flipped the
  comparison in `write_policy::evaluate()` and lowered the constant's value
  from 0.35 to 0.30 (with margin above the measured distinct-topic ceiling
  and below the lowest measured genuine-correction score) —
  `write_policy.rs`'s doc comment on the constant records the real measured
  data this value is grounded in. **Re-ran the same before/after eval**:
  `update_supersession` back to 3/3 (was 1/3), overall 11/12 — matching the
  old embedder's baseline exactly, confirming the regression is resolved,
  not just moved. Full `cargo test --release` re-run clean except one
  pre-existing, now-more-exposed flake — see T042.
  **`MERGE_SIMILARITY_THRESHOLD` (0.85) and `LOW_CONFIDENCE_THRESHOLD` (0.5)
  were deliberately left untouched** — the same probe data shows genuine
  near-duplicates (no contradiction marker) can score as low as 0.44,
  meaning these two thresholds likely also need lowering for the real
  embedder (a real, separate finding), but that recalibration has much
  broader blast radius (every merge/near-dup decision system-wide, not just
  the contradiction path) and needs its own dedicated empirical pass rather
  than a second threshold change bundled into this one — tracked as a new
  follow-up, not yet task-numbered.
- [ ] T042 **New, surfaced while verifying T041 — not this feature's bug,
  but exposed by this feature's change.** Feature 002's
  `tests/e2e/test_consolidation_e2e.rs::concurrent_consolidation_does_not_measurably_slow_recall_sc001`
  (T043/SC-001, 5% latency bound) is now measurably flaky — failed on 2 of
  ~6 real runs in this session (once mid-full-suite, once in isolation with
  `--test-threads=1`; passed cleanly on the other attempts). Root cause:
  `embed()` went from a near-free hash function to ~86ms of real CPU-bound
  ONNX inference (T006) — a 5% latency-bound test is far more exposed to
  ordinary scheduling/CPU-contention noise once the thing it's timing is
  CPU-bound real work instead of a hash. Not fixed here — belongs to
  feature 002, which owns SC-001's definition, and loosening someone else's
  already-shipped test bound without that feature's own review is exactly
  the kind of drive-by change this project's conventions avoid. Flagged
  here because feature 009 is the direct cause.
