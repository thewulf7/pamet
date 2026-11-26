# Tasks: Deterministic Per-Cluster N-gram Model

**Input**: Design documents from `/specs/008-deterministic-gram-model/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/cli-contract.md,
quickstart.md (all present)

**Tests**: Included — spec.md's three user stories are explicitly test-shaped (determinism,
zero-OOV, byte-for-byte replay), matching this project's established convention (features
002/003/005 all included test tasks).

**Organization**: Tasks are grouped by user story (US1 P1 "build at consolidation time", US2
P2 "generate", US3 P3 "inspect and replay") per spec.md.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1/US2/US3)
- Exact file paths are included in every description

## Path Conventions

Single Rust crate (library + CLI) — `src/`, `tests/` at repository root, per plan.md's
Project Structure.

---

## Phase 1: Setup

**Purpose**: New test files and module scaffolding — no new Cargo dependency (plan.md:
"No new dependencies" / research.md Decision 3's hand-written PRNG).

- [X] T001 Create empty module file `src/services/gram.rs` with module doc explaining its
  scope (trigram frequency table, hand-written deterministic PRNG, no gradients — per
  plan.md Constitution Check's Principle III note), registered as `pub mod gram;` in
  `src/services/mod.rs`
- [X] T002 [P] Create empty test file `tests/unit/test_gram_model.rs` with a new
  `[[test]]` entry in `Cargo.toml` (no `required-features` — runs on the default SQLite
  build)
- [X] T003 [P] Create empty test file `tests/integration/test_gram_replay.rs` with a new
  `[[test]]` entry in `Cargo.toml`

**Checkpoint**: `cargo build` clean (no behavior yet, module is empty).

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Shared persistence layer every user story reads or writes — the
`ClusterGramModel` entity (data-model.md) and its storage, available before any story's
actual logic lands.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [X] T004 Add `cluster_gram_models` table to **both** dialect blocks in
  `src/storage/schema.rs` (data-model.md fields: `cluster_id` PK/FK, `consolidation_run_id`
  FK, `gram_order`, `vocab_size`, `training_token_count`, `table_json`, `created_at`) —
  following the existing dual-block pattern already used for every other table in this file
- [X] T005 Add `cluster_gram_model_built` to `VALID_EVENT_TYPES` in
  `src/services/events.rs` (Constitution Principle I — every mutation needs a logged event
  before/alongside the store write)
- [X] T006 [P] In `src/services/gram.rs`, define the `ClusterGramModel` struct
  (data-model.md fields) and the `GRAM_ORDER: usize = 3` constant (research.md Decision 1)
- [X] T007 [US1-and-US3-shared] In `src/services/gram.rs`, add `persist(store, &model,
  consolidation_run_id)` (INSERT, emits the T005 event first per Constitution Principle I)
  and `load(store, cluster_id) -> Option<ClusterGramModel>` (SELECT, returns `None` — not an
  error — if no row exists, so FR-009's "specific error" is the CLI layer's job, not this
  function's) — depends on T004, T005, T006

**Checkpoint**: `cargo build` and `cargo build --features postgres-backend` both clean; the
schema and persistence helpers exist but nothing calls them yet.

---

## Phase 3: User Story 1 - Build a cluster's n-gram model at consolidation time (Priority: P1) 🎯 MVP

**Goal**: `pamet consolidate` derives and persists a frozen trigram model for every
cluster it consolidates, skipping clusters too small to be meaningful, and never
rebuilding an unchanged cluster's model.

**Independent Test**: Run `pamet consolidate` against a store with a 3+-member cluster;
confirm a `cluster_gram_models` row exists (quickstart.md Scenario 1); re-run consolidation
unchanged and confirm no duplicate/rebuild (quickstart.md Scenario 2).

### Tests for User Story 1

- [X] T008 [P] [US1] In `tests/unit/test_gram_model.rs` (file T002 scaffolded): test
  `build_for_cluster` on a 3-item input with distinct shared vocabulary produces a model
  with the expected `vocab_size`/`training_token_count` (counting is simple enough to
  hand-verify in the assertion)
- [X] T009 [P] [US1] In the same file: test `build_for_cluster` on an input whose combined
  token count is below `GRAM_ORDER` returns `None` (FR-003/Edge Case — "too short to be
  meaningful")

### Implementation for User Story 1

- [X] T010 [US1] In `src/services/gram.rs`, implement `build_for_cluster(member_texts:
  &[String]) -> Option<ClusterGramModel>`: tokenize via the existing
  `embedding::tokenize()` (reuse, not reinvent — Constitution Principle VI), build the
  trigram frequency table (`(tok_a, tok_b) -> {tok_c: count}`, joined with a NUL separator
  per data-model.md), return `None` if total tokens < `GRAM_ORDER` (depends on T006, T008,
  T009)
- [X] T011 [US1] In `src/services/consolidation.rs`, after a cluster's summary is written
  (existing code path), call `gram::build_for_cluster()` with that cluster's member texts;
  if `Some`, call `gram::persist()` (T007) with the current `consolidation_run_id`; if
  `None`, increment a new `grams_skipped_too_small` counter (contracts/cli-contract.md)
  (depends on T007, T010)
- [X] T012 [US1] In `src/services/consolidation.rs`, before calling T011's build step, check
  whether the cluster's membership set is unchanged since its existing gram model (if any)
  was built via `gram::load()` (T007) — if unchanged, skip rebuild entirely (FR-004); add
  the comparison against the cluster's current `cluster_members` rows (depends on T007,
  T011)
- [X] T013 [US1] Add `grams_skipped_too_small: usize` (and any other new counter T011/T012
  need) to `ConsolidationSummary` in `src/services/consolidation.rs`, surfaced in the
  existing CLI output for `pamet consolidate` (depends on T011, T012)

**Checkpoint**: User Story 1 is independently verified — `pamet consolidate` builds,
persists, and correctly skips/dedupes gram models. Run quickstart.md Scenarios 1, 2, 7.

---

## Phase 4: User Story 2 - Generate a local paraphrase from a cluster's model (Priority: P2)

**Goal**: `pamet gram generate --cluster-id <id> [--rng-seed <n>]` samples a persisted
model deterministically, with zero out-of-vocabulary tokens.

**Independent Test**: quickstart.md Scenario 3 (determinism) and Scenario 4 (missing model
fails clearly).

### Tests for User Story 2

- [X] T014 [P] [US2] In `tests/unit/test_gram_model.rs`: test that `generate()` called
  twice with the same model and the same seed produces byte-identical output (FR-006/SC-002)
- [X] T015 [P] [US2] In the same file: test that every token in `generate()`'s output
  appears in the model's own trigram table (SC-003 — zero out-of-vocabulary tokens),
  across several different seeds
- [X] T016 [P] [US2] In the same file: test the hand-written PRNG itself — same seed
  produces the same sequence across repeated instantiations (the determinism guarantee
  FR-006 ultimately rests on, isolated from the gram-sampling logic)

### Implementation for User Story 2

- [X] T017 [US2] In `src/services/gram.rs`, implement the hand-written deterministic PRNG
  (splitmix64/xorshift-style, research.md Decision 3) as a small private struct/fn, no
  external `rand` crate dependency (depends on T006)
- [X] T018 [US2] In `src/services/gram.rs`, implement `generate(model: &ClusterGramModel,
  seed: Option<u64>, max_tokens: Option<usize>) -> String`: seed the T017 PRNG (deriving
  from current time if `seed` is `None`), walk the trigram table sampling weighted by
  count, stop early the moment the current `(tok_a, tok_b)` key has no entries (research.md
  Decision 4 — never falls back to a lower-order model) (depends on T014, T015, T016, T017)
- [X] T019 [US2] In `src/bin/pamet.rs`, add the `gram generate --cluster-id <id> [--rng-seed
  <u64>] [--max-tokens <n>]` subcommand: call `gram::load()` (T007), if `None` print the
  FR-009 specific error and exit non-zero (contracts/cli-contract.md's exit-code scheme),
  else call T018's `generate()` and print the result to stdout (depends on T007, T018)

**Checkpoint**: User Stories 1 AND 2 both independently verified. Run quickstart.md
Scenarios 3, 4.

---

## Phase 5: User Story 3 - Inspect and rebuild gram models from the event log (Priority: P3)

**Goal**: `pamet gram show` reports a model's stats without generating; a full event-log
replay reproduces every gram model byte-for-byte — proving this feature introduced no
second source of truth (Constitution Principle I).

**Independent Test**: quickstart.md Scenario 5 (`gram show`) and Scenario 6 (real-data
replay-equivalence, diff is empty).

### Tests for User Story 3

- [X] T020 [P] [US3] In `tests/integration/test_gram_replay.rs` (file T003 scaffolded):
  seed a store, run `pamet consolidate` to produce at least one real gram model, export the
  event log, rebuild via the existing `rebuild.rs` machinery into a fresh store, and assert
  every `cluster_gram_models` row is byte-for-byte identical between source and rebuilt
  stores (FR-008/SC-004) — reuse the `open_target()`/Postgres-aware helper pattern
  `tests/integration/test_write_policy_scale.rs` already established in feature 005, so
  this runs on both backends
- [X] T021 [P] [US3] In the same file: run this replay test against the real, already-
  consolidated `~/.hermes/pamet/memory.db` (copy, not the live file) in addition to a
  synthetic fixture — spec.md's own stated bar ("verified on a real consolidated store, not
  only a synthetic one")

### Implementation for User Story 3

- [X] T022 [US3] In `src/services/rebuild.rs`, add a branch for the new
  `cluster_gram_model_built` event type (T005): on replay, re-insert the
  `cluster_gram_models` row from the event's payload (the full model, including
  `table_json`, was already serialized into the event payload by T007's `persist()` — this
  is a pure re-insert, not a recomputation, which is what makes the replay byte-for-byte
  rather than "same shape") (depends on T005, T007, T020, T021)
- [X] T023 [US3] In `src/bin/pamet.rs`, add the `gram show --cluster-id <id>` subcommand:
  call `gram::load()` (T007), if `None` print the FR-009 error and exit non-zero, else
  print `gram_order=<n> vocab_size=<n> training_tokens=<n> consolidation_run_id=<id>
  created_at=<iso8601>` (contracts/cli-contract.md's exact output format) (depends on T007)

**Checkpoint**: All three user stories independently verified; the feature's own
constitution-compliance claim (FR-008) is backed by a real-data test, not an assumption.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Final regression sweep and documentation, matching every prior feature's
closing discipline (features 003/004/005 all ended this way).

- [X] T024 [P] Run `cargo fmt` and `cargo clippy --all-targets --features postgres-backend`
  on every touched file; confirm no new warnings
- [X] T025 Full regression sweep: `cargo build` (default), `cargo build --features
  postgres-backend`, `cargo test --release` (default, all pre-existing suites) — confirm
  zero regressions outside the new gram tests
- [X] T026 Run every scenario in `specs/008-deterministic-gram-model/quickstart.md`
  end-to-end and confirm each matches its documented expected outcome
- [X] T027 Update `specs/008-deterministic-gram-model/spec.md`'s **Status** field from
  `Draft` to reflect the real implementation outcome, append a Results section with actual
  observed numbers (SC-001 timing, SC-002/SC-003/SC-004 confirmation), commit + push

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately.
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS all three user stories** (the
  schema table and persistence helpers are read/written by every story).
- **User Story 1 (Phase 3)**: Depends on Foundational only. No dependency on US2/US3.
- **User Story 2 (Phase 4)**: Depends on Foundational (needs `gram::load()` from T007) —
  does **not** depend on US1's consolidation-integration tasks (T011-T013); it can generate
  from any model however it got persisted, including one inserted directly in a test.
- **User Story 3 (Phase 5)**: Depends on Foundational (T007). T020/T021/T022 additionally
  need at least one real model to exist, which in practice means running US1's consolidation
  path first — the one genuine cross-story sequencing, same as feature 005's US3 depending
  on US1's real-data run.
- **Polish (Phase 6)**: Depends on all three user stories being complete.

### Parallel Opportunities

- T002, T003 (empty test-file scaffolding) can run in parallel — different files.
- T008, T009 (US1 tests) can run in parallel with T014, T015, T016 (US2 tests) once
  Foundational lands — different test files, no shared state.
- T020, T021 (US3 replay tests) can run in parallel with each other (synthetic vs. real-data
  fixture) but both depend on T022 existing to pass.

---

## Parallel Example: Phase 2 → Phases 3 & 4

```bash
# After T004-T007 land:
cargo test --release --test unit_gram_model &       # T008/T009 (US1), T014-T016 (US2)
wait
```

---

## Implementation Strategy

### MVP First (User Story 1 only)

1. Complete Phase 1: Setup (T001-T003).
2. Complete Phase 2: Foundational (T004-T007) — the shared persistence layer.
3. Complete Phase 3: User Story 1 (T008-T013) — models are actually built and persisted at
   consolidation time.
4. **STOP and VALIDATE**: at this point `cluster_gram_models` rows exist for real
   consolidated clusters. That alone is inspectable via raw SQL even before `gram show`/
   `gram generate` exist — if time is tight, this is the single phase that proves the core
   idea works.

### Incremental Delivery

1. Setup + Foundational → the storage mechanism exists.
2. US1 → models are built automatically, for real, during consolidation (MVP).
3. US2 → the actual payoff: local, deterministic, zero-LLM paraphrase generation.
4. US3 → the constitution-compliance proof: inspectable, and provably replayable from the
   event log alone.

---

## Notes

- [P] tasks = different files, no shared dependency.
- [Story] label maps every user-story-phase task to US1/US2/US3 for traceability;
  Setup/Foundational/Polish carry no story label per the format rules.
- T007 is deliberately shared infrastructure (Foundational), not duplicated per-story,
  because both writing (US1) and reading (US2/US3) a `ClusterGramModel` need the identical
  serialization contract — splitting it would risk the two sides drifting.
- Commit after each phase checkpoint, not after every individual task — consistent with
  how features 003/004/005 committed per logical milestone.
