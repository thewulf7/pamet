# Implementation Plan: Write-Policy Near-Duplicate Pre-Filter

**Branch**: `005-fix-pamet-write` | **Date**: 2026-10-19 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/005-fix-pamet-write/spec.md`

## Summary

`write_policy::evaluate()` currently loads *every* active item and runs an exact cosine
comparison against each one on every write (`active_items_where(store, "")` with no
predicate). Feature 004's real migration measured this growing super-linearly on real
conversational data (4.4s/200 items → 101.9s/200 items by item 2,800; 4+ hour
extrapolated full-corpus cost). The fix: narrow the candidate set with the existing
keyword index (SQLite FTS5 / Postgres `tsvector`, already built for `recall()`'s keyword
path) before running the exact cosine scan, so the expensive comparison only runs
against candidates that could plausibly match — not the whole store.

**Key finding driving the design** (see research.md): Pamet's embedder
(`services/embedding.rs`) is a deterministic hashed bag-of-words over the *same*
tokenizer the FTS/tsvector index already uses. Two texts sharing zero tokens have
dot-product-zero embeddings (barring a 1-in-4096 hash collision) — so any pair scoring
above a meaningful cosine threshold *must* share tokens. A keyword pre-filter built on
the same tokenizer therefore cannot miss a true near-duplicate at the merge/contradiction
thresholds: for this specific embedder, the "approximate" pre-filter is provably exact,
not a probabilistic trade-off. This directly satisfies FR-003 without needing a tunable
recall/speed knob.

## Technical Context

**Language/Version**: Rust 1.75+ (matches existing `pamet` crate, `edition = "2021"`)

**Primary Dependencies**: None new — reuses `rusqlite` (FTS5, already a dependency) and
`postgres`/`tsvector` (already used by `retrieval_keyword.rs`), both already wired for
both backends.

**Storage**: SQLite (default) + Postgres (`postgres-backend` feature) — both already
have the keyword index this feature reuses (`memory_items_fts` / `content_tsv`).

**Testing**: `cargo test` (existing `tests/unit/test_write_policy_rules.rs`,
`tests/integration/test_write_policy.rs`, `tests/integration/test_consolidation.rs`
eval-harness probes) plus a new real-data timing verification using feature 004's actual
Hindsight export (not a synthetic benchmark — US3/FR-006).

**Target Platform**: Same as the rest of `pamet` — a CLI/library, macOS/Linux.

**Project Type**: Single Rust crate (library + CLI), no new project structure.

**Performance Goals**: Per-200-item batch cost during a real, high-overlap-content
migration must not grow by more than 2x from first batch to last batch (SC-002); full
real ~15,446-item corpus migration under 30 minutes (SC-001), down from the 4+ hour
trajectory measured without this fix.

**Constraints**: Zero decision-outcome regression on existing write-policy/eval-harness
tests (SC-003); no measurable overhead added for small stores (SC-004); no new learned
component (Constitution Principle III, NON-NEGOTIABLE); both backends behave
equivalently (FR-005).

**Scale/Scope**: Verified against the real ~15,446-item Hindsight corpus from feature
004 — the actual scale that exposed the problem, not an arbitrary synthetic number.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Principle I (Event-Sourced Spine)**: PASS — no event schema change; this only
  changes which rows `write_policy::evaluate()` loads before producing the same
  `write_decision` event it always has.
- **Principle II (Provenance & Reversibility)**: PASS — unaffected; no change to
  tombstoning, provenance pointers, or review-gating mechanics.
- **Principle III (Rules Before Learning, NON-NEGOTIABLE)**: PASS — the pre-filter is
  the existing deterministic FTS5/tsvector keyword index, not a learned ranker. No new
  model, no training data, no inference step introduced.
- **Principle IV (Two-Speed Memory Separation)**: PASS — the fast write path and the
  slow consolidation path remain decoupled; this change only speeds up the fast path's
  own internal candidate lookup, it does not merge the two paths or move work between
  them.
- **Principle V (Eval-Gated Promotion)**: PASS — this is not promoting a learned
  component, so eval-gating does not apply; the existing eval harness is instead used
  as a *regression guard* (FR-004/SC-003), which is a stronger bar, not a weaker one.
- **Principle VI (Open-Protocol, Build-Don't-Reinvent)**: PASS, and the direct basis for
  the design — reusing the already-built, already-tested FTS5/`tsvector` keyword index
  instead of inventing a new approximate-nearest-neighbor structure (e.g., LSH, a new
  bucket index) is exactly what this principle calls for.

No violations. Complexity Tracking table is not needed.

## Project Structure

### Documentation (this feature)

```text
specs/005-fix-pamet-write/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output (no new entities — documents the changed flow)
├── quickstart.md         # Phase 1 output
└── tasks.md             # Phase 2 output (/speckit-tasks — not created by this command)
```

No `contracts/` directory: this feature changes no public API, CLI command, or
cross-system contract (FR-007 — `pamet::remember()`/`Interaction` unchanged). It is a
purely internal implementation change to `write_policy::evaluate()`'s candidate-set
assembly.

### Source Code (repository root)

```text
src/
├── services/
│   ├── write_policy.rs       # evaluate() changes: candidate set narrowed before the
│   │                         # exact cosine scan, instead of active_items(store)
│   ├── memory_items.rs       # active_items_where() already supports an extra SQL
│   │                         # predicate — the pre-filter plugs in here, no new fn
│   │                         # signature needed at this layer
│   └── retrieval_keyword.rs  # existing FTS5/tsvector search — reused as-is for the
│                             # pre-filter's candidate-id lookup (new thin wrapper, if
│                             # any, stays in write_policy.rs itself)
tests/
├── unit/test_write_policy_rules.rs        # existing — must stay green (SC-003)
├── integration/test_write_policy.rs       # existing — must stay green (SC-003)
├── integration/test_consolidation.rs      # existing — must stay green (SC-003)
└── integration/test_write_policy_scale.rs # NEW — real-data-shaped regression test
                                            # proving per-write cost stays flat as the
                                            # store grows (SC-002), using the same kind
                                            # of high-overlap content that exposed the
                                            # original problem (not a low-overlap
                                            # synthetic fixture, which previously masked it)
```

**Structure Decision**: Single-crate change confined to `src/services/write_policy.rs`
(the only file whose logic changes) plus one new scale-regression test. No new crate,
module tree, or project layout — this is the smallest-blast-radius option and matches
every prior feature's practice of touching only what the task needs.

## Complexity Tracking

Not applicable — no Constitution Check violations.
