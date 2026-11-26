# Implementation Plan: Deterministic Per-Cluster N-gram Model

**Branch**: `008-deterministic-gram-model` | **Date**: 2026-11-26 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/008-deterministic-gram-model/spec.md`

## Summary

Build a frozen, statistical n-gram (Markov chain) model per consolidated memory
cluster, derived at the moment `pamet consolidate` already produces that cluster's
summary. The model is persisted as a new derived table (`cluster_gram_models`),
exposed via two CLI subcommands (`pamet gram generate`, `pamet gram show`), and is
fully reconstructable from the event log like every other piece of Pamet's
materialized state. No gradients, no external model calls, no new Cargo dependency —
a hand-written frequency table, same spirit as the project's existing BM25-style
keyword index.

## Technical Context

**Language/Version**: Rust 1.75+ (matches existing `Cargo.toml` edition 2021)

**Primary Dependencies**: None new. Reuses `rusqlite`/`postgres` (existing storage
layer), `serde_json` (existing serialization pattern — gram tables are small enough
to store as JSON like `embedding_json`/`entities_json` already do), `rand` is
**not** a new dependency: a small deterministic PRNG (xorshift/splitmix64, same
hand-written style as `embedding.rs`'s FNV-1a hash) is implemented in-crate rather
than pulling in the `rand` crate, to keep `--rng-seed` reproducibility a one-file
guarantee instead of depending on an external crate's RNG algorithm staying stable
across versions.

**Storage**: SQLite (default) + Postgres (opt-in `postgres-backend` feature) — new
table added to both existing schema blocks in `src/storage/schema.rs`, following the
established dual-dialect pattern.

**Testing**: `cargo test` (unit tests in the gram-model module itself, matching
`write_policy.rs`'s `#[cfg(test)] mod tests` convention) + a new integration test
file for the CLI surface and replay-equivalence guarantee (FR-008/SC-004).

**Target Platform**: Same as the rest of Pamet — CLI binary, Linux/macOS, no
platform-specific code needed.

**Project Type**: Single Rust library + CLI binary (existing `pamet`/`pamet-mcp`
structure; no new binary needed — two new subcommands on the existing `pamet`
binary).

**Performance Goals**: SC-001 — build a gram model for a 50-item cluster in
<200ms. This is pure in-process string/hashmap work (no I/O beyond one SQL insert),
so the bound is generous relative to existing consolidation-pass costs already
measured in feature 002/004/005 work.

**Constraints**: Fully offline (no network call is even possible in this feature's
design — FR-001). Deterministic generation given a fixed seed (FR-006, SC-002) on
both backends.

**Scale/Scope**: One gram model per cluster; clusters in this project's real data
have ranged from 2 to 18,000+ members (the real production migration in feature
004/005 touched an 18,000-member cluster in the review-queue batch-op work from
v0.6.0). The model itself is a frequency table over that cluster's combined text,
so its size scales with *vocabulary*, not member count — bounded naturally.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Principle I (Event-Sourced Spine)** — PASS. New `consolidation_run_id` foreign
  key on `cluster_gram_models` ties every row to the event that produced it
  (FR-002). FR-008/SC-004 require byte-for-byte replay equivalence through the
  existing `rebuild.rs` machinery — no second source of truth introduced.
- **Principle II (Provenance & Reversibility)** — PASS. Model rows carry
  `consolidation_run_id` provenance; nothing is destructively deleted (a stale model
  is simply superseded by the next consolidation run touching that cluster, per
  spec Edge Cases).
- **Principle III (Rules Before Learning)** — PASS, with an explicit distinction
  recorded here because this principle is exactly the gate this feature could be
  mistaken for violating: an n-gram/Markov frequency table has **no training
  objective, no gradient descent, and no parameters fit to minimize a loss** — it is
  a deterministic counting procedure over fixed input text, structurally identical
  in kind to the BM25 keyword index this project already ships as a "rule," not a
  "learned component." It is explicitly NOT the "small LM" Principle III names as
  requiring a rule-based predecessor and logged outcome data first — that bar
  applies to a model with learned/fit parameters (e.g. a trained n-gram model
  smoothed against a held-out corpus with tunable hyperparameters), which this is
  not. No violation; no complexity-tracking entry needed.
- **Principle IV (Two-Speed Separation)** — PASS. Model construction happens inside
  the existing consolidation ("sleep") pass only (FR-001) — never on the live
  write/read path. `gram generate`/`gram show` are separate CLI invocations, not
  called from `remember()`/`recall()`.
- **Principle V (Eval-Gated Promotion)** — N/A. This feature does not touch
  embeddings, the write-policy decision model, reranking, or any online adapter — it
  adds a new, independent, opt-in capability. Nothing on the default retrieval/write
  path changes (FR-010).
- **Principle VI (Open-Protocol, Build-Don't-Reinvent)** — PASS. No event bus or
  wire protocol involved. "Build vs. adopt" justification: a frequency table is
  simple enough that adopting an external n-gram/Markov-chain crate would add a
  dependency for ~50 lines of hand-written logic — same reasoning the project
  already applied to its own hashed-bag-of-words embedder.

**Result: 0 violations. Complexity Tracking table not needed.**

## Project Structure

### Documentation (this feature)

```text
specs/008-deterministic-gram-model/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output (CLI command contract)
└── tasks.md             # Phase 2 output (/speckit-tasks — not this command)
```

### Source Code (repository root)

**Structure Decision**: Single Rust project (existing layout, Option 1 from the
template — this repo has no frontend/mobile split). New code slots into the
existing module boundaries; no new top-level directories.

```text
src/
├── services/
│   ├── consolidation.rs      # MODIFIED: call gram::build_for_cluster() per cluster
│   │                          # after summary write (US1)
│   └── gram.rs                # NEW: build_for_cluster(), generate(), the frequency
│                               # table type, the in-crate deterministic PRNG (US1/US2)
├── storage/
│   └── schema.rs              # MODIFIED: cluster_gram_models table, both dialect
│                               # blocks (SQLite + Postgres)
├── services/
│   └── rebuild.rs             # MODIFIED: replay cluster_gram_models rows from their
│                               # provenance event (US3/FR-008)
├── bin/
│   └── pamet.rs                # MODIFIED: `gram generate` / `gram show` subcommands
└── lib.rs                      # MODIFIED (maybe): re-export gram types if needed
                                 # by the CLI binary, matching existing pub use pattern

tests/
├── unit/
│   └── test_gram_model.rs     # NEW: frequency-table construction, determinism,
│                               # zero-OOV-token property (US1/US2, SC-003)
└── integration/
    └── test_gram_replay.rs    # NEW: full consolidate → rebuild → diff cycle
                                # (US3, FR-008, SC-004) — real consolidated store,
                                # not only synthetic, per spec's explicit requirement
```

## Complexity Tracking

> Fill ONLY if Constitution Check has violations that must be justified

No violations recorded — see Constitution Check above.
