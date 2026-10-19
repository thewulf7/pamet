# Implementation Plan: Production-Grade Storage Backend

**Branch**: `003-postgres-storage-backend` | **Date**: 2026-10-19 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/003-postgres-storage-backend/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

Add Postgres (+ `pgvector`) as a second, selectable, production-grade storage backend
alongside Pamet's existing embedded SQLite store — enabling genuine concurrent connections
(closing the Two-Speed Memory Separation gap SQLite's single-connection design could only
approximate) and a native vector index (replacing the brute-force cosine scan), while keeping
the embedded SQLite backend as a first-class, unchanged, fully offline fallback. Includes a
`pamet migrate` subcommand that moves an existing store's complete history onto the new backend
by reusing the existing event-replay machinery (Principle I) rather than a bespoke copy tool.

## Technical Context

**Language/Version**: Rust 1.75+ (same crate as Phase 1/2 — no new language/toolchain)

**Primary Dependencies**: `postgres` (sync Postgres client) + `r2d2`/`r2d2_postgres`
(connection pooling) + `pgvector` (Rust crate, `postgres` feature) — all added as **optional**
dependencies behind a new `postgres-backend` Cargo feature, mirroring the existing `mcp` and
`baseline-hindsight` feature pattern so the default build stays fully offline/dependency-free
(FR-006, SC-004). See `research.md` Decisions 1-2 for the full rationale and alternatives
considered.

**Storage**: Two backends, selected at `Store::open()` time by the `--db` value's URL scheme
(`research.md` Decision 3): the existing SQLite file/`:memory:` path (unchanged), or a new
`postgres://`/`postgresql://` target using `pgvector` for embeddings and native `tsvector` for
keyword search (full Postgres DDL in `data-model.md`). `Store` gains a `backend: Backend` enum
field; existing SQL-touching functions each gain a Postgres-dialect branch, co-located with
their existing SQLite implementation rather than split into parallel module trees.

**Testing**: `cargo test` (existing default-feature suite, fully unaffected) plus new
`--features postgres-backend` tests that read `PAMET_TEST_POSTGRES_URL` and skip cleanly if
unset/unreachable (`research.md` Decision 5 — same established pattern as the existing
`baseline-hindsight` Hindsight-container tests).

**Target Platform**: Same as Phase 1/2 (developed/verified on macOS aarch64) plus any Linux
server target a self-hosted Postgres instance would run on — no new platform-specific code.

**Project Type**: CLI/library (single Rust crate, same as Phase 1/2) — not a new project, not a
web service.

**Performance Goals**: p95 live remember/recall latency differs by <5% with vs. without a
concurrently-running `consolidate()` on the Postgres backend (SC-001, genuinely concurrent
connections this time, not single-connection interleaving). Similarity search under 100ms at
10,000 items (SC-003).

**Constraints**: Default build remains network-free and dependency-free (sovereignty
constraint, carried over from Phase 1/2, extended here: adopting a production backend is
strictly opt-in). Migration always targets a freshly-initialized store and refuses to run
against a non-empty target (FR-004, clarified 2026-10-19) — no partial-resume state machine
(data-model.md "Migration Run"). Connection info (credentials included) is supplied via the
existing `--db`-style CLI argument, not a separate config mechanism (FR-007, clarified
2026-10-19) — a disclosed tradeoff, not a gap (spec Assumptions).

**Scale/Scope**: Up to 10,000 memory items for this feature's own performance targets (SC-002,
SC-003) — an explicit step up from Phase 2's 1,000-item scale, since a production backend is
exactly what makes a larger scale target meaningful. Still single-logical-store, no
multi-tenant/sharding requirement (spec Assumptions, unchanged from Phase 1/2).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check | Status |
|---|---|---|
| I. Event-Sourced Spine | The `events` table and replay-from-events rule are unchanged by this feature on either backend; migration itself is implemented *as* a replay (research.md Decision 4), which is the strongest possible reaffirmation of this principle rather than a risk to it | PASS |
| II. Provenance & Reversibility | No change to provenance pointers, tombstone semantics, or the review-queue gate — only the physical storage of the same rows changes | PASS |
| III. Rules Before Learning | No learned component introduced or touched; the frozen hashed bag-of-words embedder (Phase 1) is unchanged — `pgvector` indexes its existing output, it does not replace it | PASS |
| IV. Two-Speed Memory Separation | This feature's entire purpose is to make this principle *actually testable* with real concurrent connections instead of Phase 2's documented single-connection-interleaving approximation | PASS (this feature strengthens the gate, it does not merely satisfy it) |
| V. Eval-Gated Promotion | No learned component is being promoted; SC-005 requires the existing eval harness to agree across backends, which is itself a (storage-layer) regression gate, consistent with this principle's spirit even though it does not literally apply to a non-learned storage change | PASS |
| VI. Open-Protocol, Build-Don't-Reinvent | Adopts a standard, self-hostable, widely-used database (Postgres) and its standard vector extension (pgvector) rather than inventing a bespoke store or protocol; the build-vs-adopt choice for the client library itself is justified in writing in research.md Decision 1 | PASS |

No violations — Complexity Tracking section below is empty.

## Project Structure

### Documentation (this feature)

```text
specs/003-postgres-storage-backend/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
├── contracts/           # Phase 1 output (/speckit-plan command)
└── tasks.md             # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
# Single project (same crate as Phase 1/2) — additive + in-place-extended, no new crate.
src/
├── storage/
│   ├── store.rs             # extended: `backend: Backend` enum (Sqlite | Postgres), Store::open() URL-scheme dispatch
│   ├── schema.rs             # extended: Postgres DDL alongside the existing SQLite DDL
│   └── postgres_pool.rs      # NEW: r2d2/r2d2_postgres pool setup, feature-gated
├── services/
│   ├── events.rs             # extended: Postgres-dialect insert/export alongside SQLite
│   ├── write_policy.rs       # extended: Postgres-dialect `active_items` query
│   ├── retrieval_vector.rs   # extended: pgvector similarity query alongside the SQLite brute-force scan
│   ├── retrieval_keyword.rs  # extended: Postgres `tsvector`/GIN query alongside the SQLite FTS5 query
│   ├── retrieval_fusion.rs   # extended: Postgres-dialect decay-lookup query
│   ├── consolidation.rs      # extended: Postgres-dialect queries (clustering/summary/contradiction logic itself is backend-agnostic, unchanged)
│   ├── review.rs             # extended: Postgres-dialect queries
│   ├── outcome.rs            # extended: Postgres-dialect queries
│   ├── eval_harness.rs       # extended: Postgres-dialect persistence queries
│   ├── rebuild.rs            # extended: target-backend-generic replay (reused directly by `migrate`)
│   └── migrate.rs            # NEW: `pamet migrate` implementation — export_log(source) + rebuild-style replay into a fresh target
└── bin/
    └── pamet.rs              # extended: `migrate` subcommand; `--db` now also accepts a postgres:// URL

tests/
├── integration/
│   └── test_postgres_scale.rs    # NEW: SC-003 10,000-item vector-search latency test, feature-gated
├── e2e/
│   ├── test_postgres_concurrency.rs  # NEW: SC-001 genuine two-thread concurrency test, feature-gated
│   └── test_migration_e2e.rs         # NEW: US2/SC-002 full migrate round-trip + replay-equivalence check, feature-gated
```

**Structure Decision**: Single existing crate, extended in place (each existing storage-touching
file gains a Postgres-dialect branch; two genuinely new files: `postgres_pool.rs` and
`migrate.rs`) — consistent with Phase 1/2's single-crate decision. No new crate, no new
project, no new service boundary. All new Postgres-specific code and dependencies are gated
behind the `postgres-backend` Cargo feature.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

*(empty — no violations)*
