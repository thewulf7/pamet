# Tasks: Write-Policy Near-Duplicate Pre-Filter

**Input**: Design documents from `/specs/005-fix-pamet-write/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, quickstart.md (all present)

**Tests**: Included — spec.md's US1/US2 acceptance scenarios are explicitly test-shaped
(a regression guard and a scale guard), so test tasks are in scope per the feature's own
intent, not bolted on separately.

**Organization**: Tasks are grouped by user story (US1 P1 "writes stay fast", US2 P1
"no quality loss", US3 P2 "measured on real data, not synthetic") per spec.md.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1/US2/US3)
- Exact file paths are included in every description

## Path Conventions

Single Rust crate (library + CLI) — `src/`, `tests/` at repository root, per plan.md's
Project Structure.

---

## Phase 1: Setup

**Purpose**: Scaffolding only — no new dependencies, no new project structure (plan.md:
"No new crate, module tree, or project layout").

- [X] T001 Create the empty scale-regression test file with module doc at
  `tests/integration/test_write_policy_scale.rs` (new `[[test]]` entry in `Cargo.toml`
  pointing at it, `required-features` omitted — runs on the default SQLite build)

**Checkpoint**: Project builds unchanged (no behavior yet) — `cargo build` clean.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: The one shared code change both US1 (speed) and US2 (correctness) test from
different angles — research.md Decisions 1-3. **Both user stories are blocked until
this lands.**

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

- [X] T002 In `src/services/write_policy.rs`, add a private helper that calls
  `crate::services::retrieval_keyword::search(store, &candidate.content, 200)` (research.md
  Decision 2's N=200 bound) and returns the candidate `item_id`s
- [X] T003 In `src/services/write_policy.rs`'s `evaluate()`, replace the unconditional
  `active_items(store)?` call with: (a) call T002's helper; (b) if it returns zero
  candidates, skip the cosine scan entirely and return `Decision::Store`/`"novel_store"`
  directly (research.md Decision 3 — mathematically equivalent to the old behavior for
  this embedder, not a shortcut that changes outcomes); (c) otherwise, load only those
  candidate ids via `memory_items::active_items_where(store, "id IN (...)")` (the
  `extra_where` parameter this function already supports) and run the existing exact
  cosine-similarity loop over that narrowed set only
- [X] T004 Confirm `src/services/memory_items.rs`'s `active_items_where()` dual-dialect
  `extra_where` SQL splice correctly accepts a literal `id IN ('a','b',...)` clause on
  both the SQLite and Postgres query-building branches (read-only verification task; if
  it already works unmodified — likely, since `extra_where` is designed for exactly
  this — this task is a confirmation, not a code change)

**Checkpoint**: `cargo build` and `cargo build --features postgres-backend` both clean;
the write path now takes the pre-filtered route but no test yet proves correctness or
speed — that's Phase 3/4.

---

## Phase 3: User Story 1 - Writes stay fast as the store grows on real data (Priority: P1) 🎯 MVP

**Goal**: Per-write cost stops growing in direct proportion to total corpus size on
real, topically-overlapping content.

**Independent Test**: Re-run feature 004's actual `pamet import-hindsight` migration
against the real Hindsight bank and watch per-batch progress lines stay flat instead of
climbing (4.4s → 101.9s/200 items, as originally measured).

### Tests for User Story 1

- [X] T005 [P] [US1] Write `tests/integration/test_write_policy_scale.rs` (the file
  T001 scaffolded): seed ~3,000 short, deliberately topically-overlapping synthetic
  items (templated sentences sharing a bounded vocabulary — research.md Decision 4's
  "synthetic-but-realistic" fixture, not low-overlap text that would hide the original
  problem), writing in batches of 200 via `pamet::remember()`, and assert the last
  batch's elapsed time is no more than 2x the first batch's (SC-002)

### Implementation for User Story 1

- [X] T006 [US1] Run `cargo test --release --test integration_write_policy_scale` and
  confirm T005 passes against the SQLite backend (depends on Phase 2 + T005)
- [X] T007 [US1] Run the same scale test against the Postgres backend: spin up
  `pgvector/pgvector:pg16` (same recipe as features 003/004), set
  `PAMET_TEST_POSTGRES_URL`, run
  `cargo test --release --features postgres-backend --test integration_write_policy_scale`,
  confirm equivalent pass/timing behavior (FR-005 — dual-dialect parity; depends on T006)
- [X] T008 [US1] Real-data verification per quickstart.md Scenario 5: re-run
  `./target/release/pamet --db <fresh> import-hindsight --bank hermes --ledger <fresh>`
  (feature 004's actual real Hindsight bank, `--features baseline-hindsight`) end-to-end
  or far enough to establish the real per-batch trend, and record the actual observed
  numbers (not a projection) — this is the number SC-001/SC-002 are ultimately judged
  against, same discipline feature 004 used

**Checkpoint**: User Story 1 is independently verified — real per-write cost on
high-overlap data no longer grows super-linearly, on both backends.

---

## Phase 4: User Story 2 - No loss of near-duplicate detection quality (Priority: P1)

**Goal**: The speedup introduces zero near-duplicate/contradiction/merge detection
regressions.

**Independent Test**: Run the pre-existing write-policy and eval-harness test suites
unchanged; every previously-passing assertion must still pass.

### Tests for User Story 2

- [X] T009 [P] [US2] Run `cargo test --release --test unit_write_policy_rules` and
  confirm 100% of existing assertions still pass unchanged (SC-003) — file:
  `tests/unit/test_write_policy_rules.rs` (not modified by this feature)
- [X] T010 [P] [US2] Run `cargo test --release --test integration_write_policy` and
  confirm 100% of existing assertions still pass unchanged (SC-003) — file:
  `tests/integration/test_write_policy.rs` (not modified by this feature)
- [X] T011 [P] [US2] Run `cargo test --release --test integration_consolidation` and
  confirm 100% of existing assertions still pass unchanged (SC-003) — file:
  `tests/integration/test_consolidation.rs` (not modified by this feature; this is the
  suite containing the FACT_A/FACT_B near-duplicate-clustering assertions)

### Implementation for User Story 2

- [X] T012 [US2] Add one new, targeted assertion to
  `tests/integration/test_write_policy_scale.rs` (same file T005 wrote): after seeding
  the ~3,000-item corpus, plant one more item that is an exact near-duplicate (cosine
  similarity ≥ `MERGE_SIMILARITY_THRESHOLD`) of an early-seeded item and assert it is
  still correctly merged, not missed — direct proof of FR-003/research.md Decision 1's
  "provably exact for this embedder" claim, not just an assumption (depends on T005)

**Checkpoint**: User Stories 1 AND 2 both independently verified — speed improved, zero
decision-outcome regression, and the "can't miss a true near-dup" claim has a direct
test, not just a mathematical argument in research.md.

---

## Phase 5: User Story 3 - The fix is measured on real data, not just synthetic benchmarks (Priority: P2)

**Goal**: The claimed improvement is backed by the real feature-004 corpus's actual
numbers, not a synthetic stand-in — directly preventing a repeat of the mistake that
delayed diagnosing the original problem.

**Independent Test**: spec.md's Results section states the real, measured before/after
timing from the actual feature-004 Hindsight corpus.

### Implementation for User Story 3

- [X] T013 [US3] Using T008's real run output, append a "Results" section to
  `specs/005-fix-pamet-write/spec.md` reporting the actual observed per-batch timing
  trend and total/partial elapsed time, explicitly compared against feature 004's
  original real numbers (4.4s → 101.9s/200 items; 4+ hour full-corpus projection) —
  same section-naming convention features 003/004 already established
- [X] T014 [US3] In that same Results section, explicitly compare T005's synthetic
  scale-test numbers against T008's real-corpus numbers and note any divergence,
  directly documenting whether a synthetic benchmark would have been sufficient this
  time (addressing US3's stated purpose, not leaving it implicit)

**Checkpoint**: All three user stories independently verified; the fix, its correctness
guarantee, and its real-world payoff are all backed by actual measurements.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Final regression sweep and documentation, matching every prior feature's
closing discipline.

- [X] T015 [P] Run `cargo fmt` and `cargo clippy --all-targets --features postgres-backend`
  on the touched files (`src/services/write_policy.rs`, the new
  `tests/integration/test_write_policy_scale.rs`) and confirm no new warnings
- [X] T016 Full regression sweep: `cargo build` (default), `cargo build --features
  postgres-backend`, `cargo test --release` (default, all pre-existing suites) — confirm
  zero regressions anywhere outside the new scale test
- [X] T017 Run every scenario in `specs/005-fix-pamet-write/quickstart.md` end-to-end and
  confirm each matches its documented expected outcome
- [X] T018 Update `specs/005-fix-pamet-write/spec.md`'s **Status** field from `Draft` to
  reflect the real implementation outcome (consistent with features 003/004's
  Draft→Implemented convention), and commit + push

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately.
- **Foundational (Phase 2)**: Depends on Setup — **BLOCKS both US1 and US2** (both test
  the exact same T002/T003 code change from different angles; there is no way to make
  them independently startable without duplicating the core change, which research.md
  explicitly rejected as unnecessary complexity).
- **User Story 1 (Phase 3)**: Depends on Foundational only. No dependency on US2/US3.
- **User Story 2 (Phase 4)**: Depends on Foundational only. T012 depends on T005 (reuses
  the same seeded corpus) but is otherwise independent of US1's speed assertions.
- **User Story 3 (Phase 5)**: Depends on T008 (US1's real-data run) existing — this is
  the one genuine cross-story dependency, because US3 is explicitly about *reporting on*
  US1's real measurement, not a separate code path.
- **Polish (Phase 6)**: Depends on all three user stories being complete.

### Parallel Opportunities

- T009, T010, T011 (US2's three "run existing suite unchanged" tasks) can run in parallel
  — different test binaries, no shared state.
- T006 (US1 SQLite scale run) and T009/T010/T011 (US2 existing-suite runs) can run in
  parallel once Phase 2 lands — different test files, no conflict.
- T007 (Postgres scale run) requires a Docker container up but does not conflict with
  any SQLite-only task running concurrently.

---

## Parallel Example: Phase 2 → Phases 3 & 4

```bash
# After T002-T004 land:
cargo test --release --test integration_write_policy_scale &          # T006 (US1)
cargo test --release --test unit_write_policy_rules &                 # T009 (US2)
cargo test --release --test integration_write_policy &                # T010 (US2)
cargo test --release --test integration_consolidation &                # T011 (US2)
wait
```

---

## Implementation Strategy

### MVP First (User Story 1 only)

1. Complete Phase 1: Setup (T001).
2. Complete Phase 2: Foundational (T002-T004) — **the actual fix**.
3. Complete Phase 3: User Story 1 (T005-T008) — proves the fix works, including on real
   data.
4. **STOP and VALIDATE**: at this point the core problem feature 004 hit is already
   fixed and measured. US2/US3 harden the correctness guarantee and the reporting
   discipline, but T008 alone already answers "does the real migration still take 4
   hours?" — if time is tight, this is the single task result that matters most.

### Incremental Delivery

1. Setup + Foundational → the mechanism exists.
2. US1 → proven fast on real data (MVP).
3. US2 → proven not to have regressed correctness, with a direct test of the
   provably-exact claim (not just an argument in research.md).
4. US3 → the real numbers are written down in spec.md where the next reader (including
   a future session of this same work) can find them without re-deriving them.

---

## Notes

- [P] tasks = different files/binaries, no shared dependency.
- [Story] label maps every user-story-phase task to US1/US2/US3 for traceability;
  Setup/Foundational/Polish carry no story label per the format rules.
- T002/T003 are the only tasks that touch production code — everything else is tests,
  verification runs, or documentation, reflecting how small the actual fix is (research.md:
  "the smallest-blast-radius option").
- Commit after each phase checkpoint, not after every individual task — consistent with
  how features 003/004 committed per logical milestone, not per task.

---

## Phase 7: Convergence

- [X] T019 Add a direct test to `tests/integration/test_write_policy_scale.rs` planting
  a true contradiction-band pair (shared tokens ≥2, a contradiction marker, scoring below
  `CONTRADICTION_SIMILARITY_FLOOR`) and asserting it is still caught
  (`Decision::Supersede`/`Decision::Review`, not silently missed) after pre-filtering,
  per FR-003 / US2-AC2 / the "contradiction path must never silently miss a true match"
  Edge Case (partial)
- [X] T020 Route `prefilter_never_misses_a_true_near_duplicate` (and T019's new
  contradiction test) in `tests/integration/test_write_policy_scale.rs` through the
  same `open_target()` helper `write_cost_stays_flat_on_overlapping_content` already
  uses, so both run against Postgres too when `PAMET_TEST_POSTGRES_URL` is set — FR-003's
  correctness guarantee has so far only been executably verified on SQLite, per FR-005
  (partial)
- [X] T021 Add a direct, isolated SC-004 timing assertion to
  `tests/integration/test_write_policy_scale.rs`: writing ~20 items to a fresh store
  completes within a small absolute bound (e.g. a few ms/item), as a standing regression
  guard rather than the inference-from-overall-CI-runtime spec.md's Results section
  currently relies on, per SC-004 (partial)
