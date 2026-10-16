# Implementation Plan: Memory Store MVP (Write Rules + Hybrid Retrieval + Outcome Logging)

**Branch**: `001-memory-store-mvp` | **Date**: 2026-10-16 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-memory-store-mvp/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

Build the Pamet MVP: a local, self-hosted memory store that (1) extracts candidate memory items
from interactions and files a rule-based write decision (store/merge/supersede/skip) with no
learned component, (2) serves hybrid retrieval (vector similarity + SQLite FTS5 keyword search,
fused with Reciprocal Rank Fusion) bounded to a configurable result budget, and (3) logs every
write decision and retrieval event with enough detail to train a future reranker/decision model
without re-instrumentation. Implemented as a native Rust library + CLI over a single SQLite file,
using a locally-run embedding model (no network calls, no Python runtime) — consistent with
Constitution Principles I-III and VI and the user's stated preference for native, offline-capable
implementations over interpreted-language wrappers.

## Technical Context

**Language/Version**: Rust (stable, edition 2021, MSRV 1.75+)

**Primary Dependencies**: `rusqlite` (bundled SQLite, FTS5 enabled) for storage + keyword search;
`serde`/`serde_json` for event/log schemas; `clap` for the CLI; `uuid`, `chrono`, `thiserror`.
**Implementation deviation (resolved 2026-10-16, T045)**: the embedding/vector-search path does
NOT use `ort`/ONNX or the `sqlite-vec` extension as originally planned below. It uses a
deterministic, frozen, hashed bag-of-words embedding (`src/services/embedding.rs`) with cosine
similarity computed directly in Rust over `memory_items.embedding_json` — still local, offline,
and frozen (Constitution Principle III), but without a network-fetched model file or a loadable
C extension. This keeps the MVP buildable/testable without external model assets; swapping in a
real sentence-transformer via `ort` later is a drop-in replacement behind the same
`embed()`/`cosine_similarity()` functions. See research.md's per-decision notes and
`specs/001-memory-store-mvp/tasks.md`'s "Implementation Notes" for the full rationale.

**Storage**: SQLite (single file), with FTS5 for keyword search and Rust-computed cosine
similarity over stored embedding vectors for vector search (not the `sqlite-vec` extension —
see the deviation note above) — matches Constitution's "Store: SQLite or Postgres" constraint;
Postgres is deferred until a multi-process/multi-tenant need appears.

**Testing**: `cargo test` (unit + integration), with fixture-seeded SQLite databases per test;
contract tests against the CLI and library API documented in `contracts/`.

**Target Platform**: Local macOS/Linux process (library + CLI), self-hosted — no cloud-only
dependency, satisfying the Constitution's sovereign/on-prem deployment constraint.

**Project Type**: Single project — implemented as **one Cargo crate** (`src/lib.rs` as the
`pamet-core`-equivalent library surface, plus a thin `src/bin/pamet.rs` CLI binary over it),
rather than the two-crate (`pamet-core`/`pamet`) workspace this section originally specified
(resolved 2026-10-16, T046). Same effect — the library is usable standalone and the CLI is a
thin wrapper — with less build/packaging overhead for an MVP at this size. No web/mobile surface
in this MVP.

**Performance Goals**: Retrieval p95 < 200ms and write-decision p95 < 100ms for a store of up to
50,000 memory items on a single developer machine (no distributed-scale target for the MVP).

**Constraints**: Fully offline-capable (no required network call for write or retrieval); single
SQLite file must remain portable (no server process required); every mutation must be replayable
from the event log per Constitution Principle I.

**Scale/Scope**: Single-scope memory (one agent/user context, per spec Assumptions) up to ~50,000
memory items for this MVP; multi-tenant partitioning is explicitly out of scope.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Status | How this plan satisfies it |
|---|-----------|--------|------------------------------|
| I | Event-Sourced Spine (NON-NEGOTIABLE) | PASS | Every write decision and retrieval event is appended to an `events` log table first; `memory_items`/`write_decisions`/`review_queue`/`retrieval_events`/`outcome_records` are rebuildable by replaying `events` alone via `services::rebuild::rebuild_from_events()` (T041), verified by `tests/integration/test_rebuild.rs`. See data-model.md. |
| II | Provenance & Reversibility by Construction | PASS | `memory_items.provenance_event_id` always set; deletes are tombstones (`memory_items.tombstoned_at`), never row deletion; low-confidence/contradicting writes go to `review_queue` instead of auto-apply. |
| III | Rules Before Learning | PASS | Write policy (store/merge/supersede/skip) and retrieval fusion (RRF) are both deterministic, hand-written rules for this MVP — no GBT/LM/bandit component is introduced. Logging is wired from day one so a future learned component has training data (Principle III's explicit precondition). |
| IV | Two-Speed Memory Separation | PASS (by scope) | This MVP only implements the fast episodic write/read path. No consolidation ("sleep") job exists yet — explicitly out of scope per spec Assumptions — so there is nothing to couple to the hot path. Re-check required when consolidation is added in a later feature. |
| V | Eval-Gated Promotion | N/A for this MVP | No learned component is being promoted in this feature (Principle III keeps this MVP rule-based), so there is no promotion gate to pass yet. The success criteria (SC-002, SC-003) establish the baseline numbers a future learned component must beat. |
| VI | Open-Protocol, Build-Don't-Reinvent | PASS | No bespoke event bus is introduced — the event log is a local SQLite table for this MVP's single-process scope (no distributed bus needed yet); when a bus is introduced in a later feature it must stay Kafka-compatible per the constitution. Uses existing, open components (SQLite, SQLite FTS5, sqlite-vec, ONNX Runtime) rather than building a vector index or tokenizer from scratch. |

No violations requiring Complexity Tracking justification.

## Project Structure

### Documentation (this feature)

```text
specs/001-memory-store-mvp/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
├── contracts/           # Phase 1 output (/speckit-plan command)
└── tasks.md             # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
# Single crate: library (src/lib.rs) + CLI binary (src/bin/pamet.rs) — see
# the "Implementation deviation" note in Technical Context above; the
# module layout below matches what was originally planned.
src/
├── models/      # MemoryItem, WriteDecisionRecord, RetrievalEvent, OutcomeRecord, ReviewQueueEntry
├── services/     # write_policy (rules), retrieval_* (hybrid search + RRF fusion), events
│                 # (append/export), rebuild (T041 replay), outcome, review, embedding
├── storage/      # Store (SQLite connection + schema/migrations)
├── bin/pamet.rs  # pamet CLI binary: remember, recall, outcome, review, forget, export-log
└── lib.rs        # pamet-core public library surface (contracts/library-api.md)

tests/
├── contract/    # CLI + library API contract tests
├── integration/ # end-to-end: interaction -> write decision -> retrieval -> outcome logging;
│                # tombstone (T044); rebuild-from-events equivalence (T041)
└── unit/        # write_policy rules, RRF fusion, tombstone/supersede logic
```

**Structure Decision**: Single Rust crate (`src/lib.rs` as the library surface, `src/bin/pamet.rs`
as the CLI) rather than a separate `pamet-core`/`pamet` two-crate workspace (resolved 2026-10-16,
T046 — see the Technical Context note above). This still matches the project's "library-first,
CLI exposes it" direction and keeps the MVP deployable as either an embedded library (future
agent integration) or a standalone CLI (for manual testing per quickstart.md), with no web/mobile
surface needed for this feature.

## Complexity Tracking

> No entries — Constitution Check reported no violations requiring justification.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| — | — | — |
