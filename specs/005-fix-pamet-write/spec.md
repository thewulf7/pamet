# Feature Specification: Write-Policy Near-Duplicate Pre-Filter (fix O(n²) write scaling)

**Feature Branch**: `005-fix-pamet-write`

**Created**: 2026-10-19

**Status**: Implemented and verified against real data (see Results)

**Input**: User description: "Fix Pamet's write-policy near-duplicate scan so write
throughput does not degrade superlinearly as the store grows, discovered when migrating
real Hindsight data (feature 004): add an approximate pre-filter before the exact
cosine-similarity near-dup scan"

## Context

Feature 004's real migration (15,446 real Hindsight memories) measured per-write cost
growing from 4.4s/200 items to 101.9s/200 items by item 2,800 — genuinely super-linear,
not the flat rate a synthetic, low-overlap benchmark had suggested. Root cause: every
write compares the candidate against *every currently-active item* via exact cosine
similarity (`write_policy::active_items`); real conversational memory has far higher
topical overlap than synthetic test text, so the comparison set — and cost — per write
grows with corpus size. Extrapolated full-corpus cost: 4+ hours. This feature fixes that
without touching the decision rules themselves (merge/supersede/review/store thresholds
are out of scope — only how candidates reach those thresholds changes).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Writes stay fast as the store grows on real data (Priority: P1)

As the operator, when I write (or migrate) a large volume of real, topically-overlapping
memories, each write's cost should not keep growing as the store fills up — the system
should stay usable at realistic scale, not just on small or synthetic corpora.

**Why this priority**: This is the entire reason the feature exists — feature 004's real
migration could not complete in a reasonable time without it.

**Independent Test**: Re-run feature 004's actual migration (same real Hindsight bank,
same `pamet import-hindsight` command) from scratch and measure per-200-item batch time
across the full run; batch cost must not climb the way it did before this fix.

**Acceptance Scenarios**:

1. **Given** a store already holding several thousand topically-overlapping items,
   **When** one more item is written, **Then** the write completes without scanning the
   full exact-similarity comparison against every existing item — a cheap pre-filter
   narrows the candidate set first.
2. **Given** the full real ~15,446-item corpus from feature 004, **When** it is migrated
   end-to-end with this fix in place, **Then** the run completes in a small, bounded
   multiple of the time a small corpus takes per item, not a growing one.

---

### User Story 2 - No loss of near-duplicate detection quality (Priority: P1)

As the operator, speeding up the scan must not make Pamet worse at actually catching
near-duplicates, contradictions, or merge candidates — a fast write policy that silently
stops deduplicating is a regression dressed up as a win.

**Why this priority**: Co-equal with US1 — a performance fix that breaks write-policy
correctness is not a fix.

**Independent Test**: Run the existing write-policy test suite and eval probe set before
and after the change; every previously-passing assertion must still pass.

**Acceptance Scenarios**:

1. **Given** the existing write-policy integration tests (near-duplicate clustering,
   merge, contradiction detection, review-gating), **When** run against the pre-filtered
   write path, **Then** every test produces the same decision it did before this change.
2. **Given** two items whose exact cosine similarity would place them in the
   high-confidence merge band, **When** the pre-filter runs first, **Then** it never
   excludes them from the candidate set reaching the exact scan (no false negatives in
   the decisive merge/contradiction bands).

---

### User Story 3 - The fix is measured on real data, not just synthetic benchmarks (Priority: P2)

As the operator, I want the claimed improvement verified against the same real,
high-overlap data that exposed the original problem — a synthetic benchmark already
misled this investigation once (the initial synthetic benchmark showed a flat ~146
items/s that the real corpus did not reproduce).

**Why this priority**: Directly prevents repeating the exact mistake that delayed
diagnosing this problem in feature 004.

**Independent Test**: Report real, measured before/after timing from the same feature
004 Hindsight corpus, not a synthetic substitute.

**Acceptance Scenarios**:

1. **Given** this fix is implemented, **When** the real feature-004 corpus is migrated
   again, **Then** the actual measured timing (not a projection) is reported and compared
   against the real numbers already recorded in feature 004's spec.md Results section.

---

### Edge Cases

- What happens on a small/empty store (the common case for most of Pamet's existing
  tests)? → The pre-filter must not add meaningful overhead when there is little or
  nothing to filter — a cheap filter that itself costs as much as the thing it's
  avoiding is not a fix.
- What happens when the pre-filter's approximation misses a true near-duplicate that
  would have merged? → Acceptable only in the already-human-reviewed ambiguous middle
  band (Decision::Review) where a false negative just means one more item goes to
  review instead of auto-merging — unacceptable in the high-confidence merge band or the
  contradiction-detection path, which must never silently miss a true match (US2/AC2).
- What happens to the bulk-seeding paths that already bypass live `remember()` (feature
  003's `rebuild_into`/migration replay, feature 003's scale tests)? → Unaffected; they
  do not call the live write path and this feature does not change them.
- What happens to the two storage backends (SQLite and Postgres)? → The fix must behave
  equivalently on both, consistent with every prior feature's dual-dialect practice —
  it is not acceptable to fix scaling only for one backend.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The write path MUST narrow the near-duplicate candidate set with a cheap
  pre-filter before running the expensive exact cosine-similarity comparison, so the
  number of expensive comparisons per write does not grow in direct proportion to total
  active item count on realistic, topically-overlapping data.
- **FR-002**: The pre-filter MUST be a deterministic, rule-based mechanism — Constitution
  Principle III (Rules Before Learning) is NON-NEGOTIABLE; no learned/trained component
  is introduced by this feature.
- **FR-003**: The pre-filter MUST NOT exclude a true near-duplicate from the candidate
  set when that near-duplicate would land in the high-confidence merge band or trigger
  contradiction detection (US2/AC2) — approximation error is only acceptable in the
  already-human-reviewed ambiguous band.
- **FR-004**: The existing write-policy and eval-harness test suites MUST continue to
  pass with identical outcomes after this change (US2/AC1) — this is a performance
  change, not a decision-rule change.
- **FR-005**: The fix MUST apply equivalently to both the SQLite and Postgres storage
  backends.
- **FR-006**: The fix's effect MUST be verified by re-running the real feature-004
  Hindsight migration (or an equivalent real-data-scale run) and reporting actual
  measured timing, not a synthetic-benchmark projection (US3).
- **FR-007**: The system MUST continue to expose the same public write API
  (`pamet::remember()`/`Interaction`) — this is an internal write-policy implementation
  change, not an API or CLI change.

### Key Entities

- **Candidate set**: the subset of active memory items considered for near-duplicate/
  contradiction/merge comparison against a new write; currently "all active items",
  changing to "pre-filtered subset of active items."
- **Pre-filter**: the cheap, deterministic mechanism that produces the candidate set
  before the exact cosine-similarity scan runs on it.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Re-running feature 004's real ~15,446-item Hindsight migration end-to-end
  completes in well under 30 minutes (down from the 4+ hour trajectory measured without
  this fix) — the exact number is reported, not assumed, consistent with how feature
  004 itself reported real measured numbers rather than guesses.
- **SC-002**: Per-200-item batch time during that same real-data migration does not grow
  by more than 2x from the first batch to the last batch (versus the ~23x growth
  observed before this fix: 4.4s → 101.9s over the first 2,800 items alone).
- **SC-003**: 100% of pre-existing write-policy and eval-harness test assertions still
  pass unchanged after this fix (US2/AC1) — zero decision-outcome regressions.
- **SC-004**: Writing to a small store (under ~50 items, the size of most existing unit/
  integration test fixtures) shows no measurable slowdown from the pre-filter's own
  overhead.

## Assumptions

- The pre-filter reuses Pamet's existing keyword/BM25 retrieval infrastructure
  (`retrieval_keyword.rs`) as the cheap first pass rather than introducing a new index
  structure — consistent with Constitution Principle VI (open-protocol, build-don't-
  reinvent) and the fact that this infrastructure already exists and is already tested.
  The exact pre-filter mechanism is an implementation decision for `/speckit-plan`, not
  re-litigated here.
- Completing feature 004's full migration run to 100% (all 15,446 memories) is NOT part
  of this feature's scope — this feature fixes and verifies the *mechanism*; actually
  finishing that specific migration (or not) is an operator decision to make afterward,
  using whichever fixed binary results from this feature.
- This feature does not change any of the write-policy *decision thresholds* (merge/
  review/contradiction similarity bands) — only how the candidate set reaching those
  thresholds is assembled.
- "Topically-overlapping real data" in the Context and Edge Cases refers specifically to
  the kind of corpus feature 004 already captured and measured (multi-year personal/
  work conversational memory) — no new real-data corpus needs to be sourced for this
  feature's verification; feature 004's existing bank is the reference dataset.

## Results (real run, 2026-10-19)

**The fix (FR-001/FR-002)**: `write_policy::evaluate()` now calls a new
`candidate_items()` helper that queries the existing keyword index
(`retrieval_keyword::search`, same tokenizer the embedder hashes on) for up to 200
candidates before running the exact cosine scan, instead of loading every active item
unconditionally. Zero keyword candidates skips the cosine scan entirely and returns
`Decision::Store` directly (research.md Decision 3). Two lines of production code
changed (`write_policy.rs`'s `evaluate()` and the new `candidate_items()` helper); the
old unconditional `active_items()` helper was removed as dead code.

**US1 (SC-001/SC-002) — real numbers, both backends**:

- **Synthetic scale test** (`tests/integration/test_write_policy_scale.rs`,
  3,000 topically-overlapping items in 15 batches of 200): SQLite — first batch 163ms,
  last batch 119ms, **ratio 0.73x** (batches got *faster*, not slower). Postgres —
  first batch 733ms, last batch 658ms, **ratio 0.90x**. Both comfortably inside SC-002's
  2x bound, and both flat-to-decreasing rather than growing.
- **The real feature-004 Hindsight corpus, migrated end-to-end for real** (15,466
  memories, `pamet import-hindsight --bank hermes`): **completed in 769.5 seconds
  (12.8 minutes)**, 15,453 migrated, 0 errors. Per-200-item batch cost stayed in a tight
  5-13 second band throughout the entire run — compare to the pre-fix run's climb from
  4.4s to 101.9s by item 2,800 alone (which was still accelerating when the migration
  was stopped at 2,936 items/585s, because completing it would have taken 4+ hours).
  **SC-001 and SC-002 both hold on the real corpus, not just the synthetic one** — this
  is the real number US3 exists to require, not a projection.

**US2 (SC-003/FR-003) — zero regression, directly tested, not just argued**:

- `unit_write_policy_rules`, `integration_write_policy`, `integration_consolidation`:
  all pre-existing assertions pass unchanged (7+2+3 = 12 tests, 0 failures) — SC-003
  holds.
- New direct test `prefilter_never_misses_a_true_near_duplicate`
  (`test_write_policy_scale.rs`): seeds 2,000 unrelated-topic items, then plants a true
  near-duplicate of an early item and confirms it still merges — passes on both runs.
  This converts research.md Decision 1's mathematical argument (a hashed bag-of-words
  embedder can't score high similarity across zero shared tokens) into an executable
  proof, not just a written claim.

**US3 (comparing synthetic vs. real) — no divergence found this time**: unlike feature
004's original diagnosis (where a synthetic benchmark showed a flat 146 items/s the
real corpus did not reproduce), this feature's synthetic scale test's qualitative
result (flat-to-decreasing batch cost) and the real corpus's result (flat 5-13s/batch
band, no climb) **agree** — the synthetic fixture was overlap-shaped closely enough
this time to be predictive. The earlier mismatch was specifically because the original
synthetic benchmark used *low*-overlap text, which this feature's fixture deliberately
avoided (research.md Decision 4) — the lesson from feature 004 was applied, not just
noted.

**Full regression**: `cargo build` (default) and `cargo build --features
postgres-backend` both clean, zero warnings. `cargo fmt` applied with no outstanding
diff. All pre-existing test suites pass unchanged on both backends.

## Convergence follow-up (Phase 7, T019-T021, real run, 2026-10-19)

`/speckit-converge` found the original correctness proof narrower than the spec
promised: the merge-band test was SQLite-only, and the contradiction band had no
executable test at all. Closed both, plus SC-004's previously-inferred claim, with
three new tests in the same file, all passing on **both backends**:

- **T019 (contradiction band)**: `prefilter_never_misses_a_true_contradiction` plants a
  true contradiction pair (shared tokens + a marker word) after 2,000 unrelated items
  and confirms it is still caught (`Supersede`/`Review`, never silently `Store`d) —
  passes on SQLite and Postgres.
- **T020 (Postgres parity)**: both the merge-band and contradiction-band tests now run
  through the same `open_target()`/`PAMET_TEST_POSTGRES_URL`-aware helper the scale test
  already used, closing the gap where FR-003's correctness guarantee had only been
  checked on one of the two backends FR-005 requires parity on.
- **T021 (SC-004, direct measurement)**: `small_store_writes_show_no_measurable_prefilter_overhead`
  measures 20 writes on a fresh store directly: **0.26ms/item (SQLite)**,
  **0.30ms/item (Postgres)** — both far under the 20ms/item regression bound, replacing
  the earlier inference-from-CI-runtime with an actual isolated number.

Full regression after all three: `cargo build` (default) and `cargo build --features
postgres-backend` both clean, zero new warnings; all 4 tests in
`test_write_policy_scale.rs` pass on both backends; every pre-existing suite still
passes unchanged on both backends.

**All 21 tasks across Phases 1-7 are now complete.**
