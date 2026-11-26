# Implementation Plan: Real Semantic Memory Search

**Branch**: `009-local-embedding-search` | **Date**: 2026-11-26 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/009-local-embedding-search/spec.md`

## Summary

Replace `src/services/embedding.rs`'s frozen hashed bag-of-words vector (4096-dim
FNV-1a buckets, no real semantic content, by its own doc comment a deliberate MVP
deviation from the project's original plan) with a frozen local sentence-embedding
model loaded via ONNX Runtime (the `ort` crate). Replace the brute-force
full-table-scan cosine search in `src/services/retrieval_vector.rs` and the
near-duplicate scoring in `src/services/write_policy.rs` with `sqlite-vec`'s `vec0`
virtual table (statically linked, no runtime extension loading, no network
dependency). Re-embed every existing real memory item as a one-time, measured
migration — not a silent format assumption. No Postgres equivalent: the project is
SQLite-only (constitution v1.1.0).

## Technical Context

**Language/Version**: Rust 1.75+ (matches existing `Cargo.toml` edition 2021).

**Primary Dependencies** (new):
- `sqlite-vec` crate — official FFI bindings, embeds the `sqlite-vec` C source and
  statically links it via the `cc` crate at build time (no loadable extension, no
  network dependency at runtime). Registered once per connection via
  `rusqlite::auto_extension::register_auto_extension` (verified against the
  project's own upstream docs at alexgarcia.xyz/sqlite-vec/rust.html and a resolved
  compatibility issue, asg017/sqlite-vec#206, for the exact `RawAutoExtension`
  transmute pattern needed for current rusqlite).
- `ort` crate (currently 2.0.0-rc.15, wraps ONNX Runtime 1.31) — safe Rust wrapper,
  no Python/interpreted runtime, fits the project's native-only constraint.
- `tokenizers` crate (HuggingFace's official Rust tokenizer library) — loads a
  model's `tokenizer.json` directly; avoids hand-rolling WordPiece tokenization,
  which real sentence-transformer models require and a from-scratch
  implementation would be a correctness risk not worth taking for a frozen,
  off-the-shelf model.
- A **rusqlite version bump** (from the pinned `0.31` to at least a version where
  `auto_extension` is stable under its `loadable_extension` feature — confirmed
  present on current rusqlite via docs.rs; exact minimum version to be pinned
  during implementation after running the full existing test suite against it,
  since this is a real dependency upgrade across the whole crate, not scoped to
  this feature alone).

**Storage**: SQLite only (constitution v1.1.0 — the separate postgres-backend
deletion task removes Postgres entirely; this feature does not need a pgvector
equivalent). New `vec0` virtual table (`memory_items_vec`) alongside the existing
`memory_items` table, mirroring how `memory_items_fts` already sits alongside it
for keyword search.

**Testing**: `cargo test` — unit tests for the new embedding module (determinism:
same input text always produces the same vector; dimension matches the model's
real output; empty/degenerate text handled per FR edge cases) and an integration
test proving FR-004 (semantic match beats pure keyword overlap) against a small
fixed set of paraphrase pairs, plus a real run against the project's existing eval
probe set for SC-001 (measured, not estimated).

**Target Platform**: Same as the rest of Pamet — CLI binary, Linux/macOS.

**Project Type**: Single Rust library + CLI binary (existing `pamet` structure; no
new binary).

**Performance Goals**: SC-002 — query latency must not grow with store size. The
`vec0` virtual table's native KNN replaces the current `O(n)` per-query scan
(`retrieval_vector.rs`'s `SELECT id, embedding_json FROM memory_items WHERE
tombstoned_at IS NULL` then score every row in Rust) — this is the whole point of
adopting it, not an incidental improvement.

**Constraints**: Fully offline at query/write time (FR-002/FR-008) — the ONNX
model file must be present locally before the binary runs; no network fetch is
performed by the running binary itself. Model packaging (vendored into the repo as
a release asset vs. downloaded once by a setup script vs. baked into the Docker
image) is an open implementation decision to resolve in `research.md`, constrained
by the existing Dockerfile's "default build stays offline" principle (reused from
feature 008's Docker work).

**Scale/Scope**: The project's own live memory store already holds ~25,000 real
`memory_items` (verified this session via `sqlite3 ... "SELECT count(*) FROM
memory_items"`-equivalent checks during feature 008's work) — this is the
"realistic size" referenced throughout spec.md's Stories 2/3 and SC-002/SC-004,
not a hypothetical number.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Principle I (Event-Sourced Spine, NON-NEGOTIABLE)**: PASS, with a design
  obligation carried into Phase 1 — the new `memory_items_vec` table and whatever
  "which model version produced this vector" tracking FR-009 requires MUST be
  derivable by replaying the event log alone, following the exact precedent
  `src/services/rebuild.rs` already established for `cluster_gram_models`
  (feature 008) and consolidation state (feature 002). `data-model.md` and
  `tasks.md` must include an explicit replay-path task, not an afterthought.
- **Principle II (Provenance & Reversibility)**: PASS — no new deletion semantics
  introduced; re-embedding is additive/replacing a derived column, not touching
  provenance pointers.
- **Principle III (Rules Before Learning, NON-NEGOTIABLE)**: PASS, with an
  explicit justification required (same pattern feature 008 used against Titans):
  a frozen, pretrained ONNX sentence-embedding model used purely as a fixed
  feature extractor is NOT "learning" in the sense this principle restricts — it
  is never fine-tuned, never updated from this project's own usage data, and
  produces the same output for the same input forever once chosen. This is
  explicitly anticipated by the constitution's own "Embeddings" architecture
  constraint (v1.1.0): "a frozen local sentence-embedding model ... never
  fine-tuned." The learned components the principle actually gates (query/key
  projections `Wq`/`Wk`) remain untouched and still deferred to Phase 3.
- **Principle IV (Two-Speed Memory Separation)**: PASS — embedding generation
  happens on the live write path (same as today), and the one-time backfill
  migration is explicitly a separate, offline, measured batch operation, not
  something that runs inline on the live write/read path.
- **Principle V (Eval-Gated Promotion)**: PASS, and load-bearing — SC-001
  explicitly requires measuring against the project's existing eval probe set
  before this is considered done, not merely "feels better."
- **Principle VI (Open-Protocol, Build-Don't-Reinvent)**: PASS — `sqlite-vec` and
  `ort` are both adopt-existing, not from-scratch reinvention; the "build vs.
  adopt" justification is this plan itself.

No violations requiring the Complexity Tracking table.

## Project Structure

### Documentation (this feature)

```text
specs/009-local-embedding-search/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md         # Phase 1 output
├── contracts/            # Phase 1 output
└── tasks.md              # Phase 2 output (/speckit-tasks)
```

### Source Code (repository root)

```text
src/
├── services/
│   ├── embedding.rs          # REPLACED: hashed-BoW -> ort/ONNX frozen model
│   ├── retrieval_vector.rs   # REPLACED: brute-force scan -> vec0 KNN query
│   ├── write_policy.rs       # UPDATED: candidate scoring queries vec0 instead
│   │                          of pulling embedding_json and scoring in Rust
│   ├── embedding_backfill.rs # NEW: one-time re-embed-all-existing-items migration
│   └── rebuild.rs             # UPDATED: replay support for vec0 rows (Principle I)
├── storage/
│   └── schema.rs              # UPDATED: new memory_items_vec virtual table
└── bin/
    └── pamet.rs                # UPDATED: new `pamet embedding backfill` CLI
                                 # subcommand (mirrors feature 008's `pamet gram`
                                 # pattern) to run + measure the migration for real

tests/
├── unit/
│   └── test_embedding.rs      # NEW
└── integration/
    ├── test_vector_search.rs  # NEW
    └── test_embedding_backfill.rs  # NEW
```

**Structure Decision**: Single Rust crate, same structure as every prior Pamet
feature. No new binary; one new CLI subcommand group (`pamet embedding ...`)
following the `pamet gram ...` precedent from feature 008.

## Complexity Tracking

> No Constitution Check violations — table intentionally omitted.
