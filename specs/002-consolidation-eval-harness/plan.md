# Implementation Plan: Consolidation Job & Eval Harness

**Branch**: `002-consolidation-eval-harness` | **Date**: 2026-10-16 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/002-consolidation-eval-harness/spec.md`

**Note**: This template is filled in by the `/speckit-plan` command. See `.specify/templates/plan-template.md` for the execution workflow.

## Summary

Add a second, independently-schedulable path to Pamet alongside the existing
live remember/recall path (Phase 1): a **consolidation job** that clusters
near-duplicate memory items into summaries, flags contradictions for review,
and decays unused items — all event-sourced and non-destructive — plus a
**repeatable eval harness** (4 held-out probe categories) and a **versioned
baseline report comparing Pamet against Hindsight**, so Phase 3+ learned
components have a real, reproducible bar to beat (Constitution Principle V).

## Technical Context

**Language/Version**: Rust 1.75+ (same crate as Phase 1 — no new language/toolchain)

**Primary Dependencies**: Consolidation and the eval harness itself need no
new runtime dependencies (reuse `rusqlite`, `serde`/`serde_json`, `clap`,
`uuid`, `chrono`). The Hindsight-baseline comparison column needs an HTTP
client to call Hindsight's local REST API — `reqwest` (blocking, `json`
feature) added as a new **optional** dependency behind a `baseline-hindsight`
Cargo feature, mirroring the existing `mcp` feature pattern, so the default
build (and the mandatory parts of this phase) stay network-free per the
sovereignty constraint — only opting into the Hindsight comparison pulls in
a network-capable dependency at all.

**Storage**: Same SQLite file Phase 1 uses (`events` table as source of
truth). Additive schema only: new tables for cluster membership and decay
state, plus a new `summary` `ItemType` variant — detailed in `data-model.md`.
No message bus, no second database, no external service.

**Testing**: `cargo test` (existing harness: unit/contract/integration/
smoke/e2e). The eval harness itself is a new CLI subcommand with its own
fixture-driven tests, same pattern as Phase 1's `tests/integration/`.

**Target Platform**: Same as Phase 1 — any platform `rusqlite` + the Rust
std toolchain supports (developed/verified on macOS aarch64; no
platform-specific code introduced).

**Project Type**: CLI/library (single Rust crate, same as Phase 1) — not a
web service, not a new project.

**Performance Goals**: Consolidation run adds <5% p95 latency delta to
concurrent live remember/recall traffic (SC-001). Eval harness completes a
full run against a 1,000-item store in under 2 minutes (SC-002).

**Constraints**: No network access required for either the consolidation
job or the eval harness (sovereignty constraint carried over from Phase 1).
Eval harness runs MUST be deterministic/reproducible (SC-003). The baseline
report's Hindsight column is best-effort: if the local Hindsight instance
isn't reachable, the report still ships with that column marked unavailable
(spec Edge Cases) rather than failing the whole run.

**Scale/Scope**: Development/single-user scale, same as Phase 1 — up to
~1,000 memory items for the eval harness's own performance target; no
concurrency/multi-tenant requirements introduced in this phase.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check | Status |
|---|---|---|
| I. Event-Sourced Spine | Every consolidation decision (cluster formed, summary created, contradiction flagged, decay applied) is emitted as a new event type in the existing `events` table; derived tables remain rebuildable via `rebuild_from_events()` (extended, not replaced) | PASS |
| II. Provenance & Reversibility | Summaries carry provenance to every source item (new `cluster_members` table, not a destructive merge); decay adjusts a ranking field only, never deletes; low-confidence contradictions go to the existing `review_queue`, not auto-resolved | PASS |
| III. Rules Before Learning | Consolidation (clustering/decay/contradiction-flagging) and the eval harness are themselves rule-based/heuristic — no learned component is introduced in this phase; they exist specifically to produce the logged data and the eval gate Phase 3 needs | PASS |
| IV. Two-Speed Memory Separation | Consolidation runs as a separate CLI subcommand (`pamet consolidate`) over a read of the event log; it MUST NOT be invoked from, or block, the `remember`/`recall` code path — enforced by keeping it a fully separate binary entry point with its own DB connection/transaction scope | PASS |
| V. Eval-Gated Promotion | This phase's entire purpose is to BUILD the eval harness + baseline report that Principle V requires before Phase 3; nothing here is itself a promotion decision | PASS (this phase IS the gate-builder) |
| VI. Open-Protocol, Build-Don't-Reinvent | No event bus introduced (stays on SQLite per Assumptions); baseline comparison reuses the already-integrated Hindsight client rather than building a new adapter; no bespoke protocol invented | PASS |

No violations — Complexity Tracking section below is empty.

## Project Structure

### Documentation (this feature)

```text
specs/002-consolidation-eval-harness/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
├── contracts/           # Phase 1 output (/speckit-plan command)
└── tasks.md             # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
# Single project (same crate as Phase 1) — additive only, no new crate/project.
src/
├── models/
│   ├── memory_item.rs       # +ItemType::Summary variant
│   ├── cluster.rs           # NEW: Cluster, ClusterMember
│   ├── decay_state.rs       # NEW: DecayState
│   └── eval.rs              # NEW: EvalProbe, EvalResult, BaselineReport
├── services/
│   ├── consolidation.rs     # NEW: clustering, summary generation, decay, contradiction flagging
│   ├── eval_harness.rs      # NEW: fixture loading, probe execution, scoring, report writing
│   ├── baseline_hindsight.rs# NEW: thin HTTP client calling the local Hindsight API for the comparison column
│   └── rebuild.rs           # extended: replay the new consolidation event types
├── storage/
│   └── schema.rs            # extended: cluster_members, decay_state, eval_results tables
└── bin/
    └── pamet.rs              # extended: `consolidate` and `eval` subcommands

tests/
├── fixtures/
│   └── eval_probes_v1.json  # NEW: synthetic probe set (FR-009), versioned
├── integration/
│   ├── test_consolidation.rs    # NEW
│   └── test_eval_harness.rs     # NEW
└── e2e/
    └── test_consolidation_e2e.rs # NEW: concurrent consolidation + live recall, no latency regression
```

**Structure Decision**: Single existing crate, additive only (new modules
under `src/models/`, `src/services/`, new test files) — consistent with
Phase 1's single-crate decision (documented deviation already recorded in
`specs/001-memory-store-mvp/plan.md`). No new crate, no new project, no new
service boundary.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

*(empty — no violations)*
