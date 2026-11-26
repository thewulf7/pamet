# Implementation Plan: Memory Pipeline Quality

**Branch**: `006-memory-pipeline-quality` | **Date**: 2026-11-25 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/006-memory-pipeline-quality/spec.md`

## Summary

Five improvements to the write/retrieval pipeline identified through live session observation:
(1) filter noise from prefetch queries; (2) retain assistant-turn facts in `sync_turn`;
(3) add `review list` enrichment + `approve-cluster` / `skip-cluster` CLI subcommands;
(4) document `pamet_outcome` discipline; (5) make prefetch budget configurable.
No schema changes. Changes split across the Rust binary (`src/`) and the Python plugin
(`~/.hermes/plugins/pamet/__init__.py`).

## Technical Context

**Language/Version**: Rust (edition 2021, stable), Python 3.9+

**Primary Dependencies**: rusqlite, clap (Rust CLI); subprocess, json (Python plugin)

**Storage**: SQLite (primary); Postgres (feature-gated). No schema migrations.

**Testing**: `cargo test --lib` for Rust unit tests; manual quickstart scenarios for plugin

**Target Platform**: macOS (developer workstation); CLI binary + Python plugin adapter

**Project Type**: CLI tool + embedded library + Python plugin adapter

**Performance Goals**: `review list` under 2s against 25K-item store (SC-003); `approve-cluster` under 1s per cluster (SC-004)

**Constraints**: No new external dependencies. No schema migrations. Constitution Principle III: all new logic is rule-based.

**Scale/Scope**: Real DB has ~25K items; 1,396 pending review clusters from Nov-25 consolidation run.

## Constitution Check

| Principle | Pre-Design Gate | Post-Design Check |
|-----------|----------------|-------------------|
| I — Event-Sourced Spine | ✅ No new mutations without events | ✅ `review_resolution` already emitted by `services::review::resolve()` |
| II — Provenance & Reversibility | ✅ No destructive deletes | ✅ Approve-cluster uses existing tombstone/suppression via SC-005 |
| III — Rules Before Learning | ✅ | ✅ Stop-word filter, min-token gate, and cluster approval are all deterministic rules |
| IV — Two-Speed Separation | ✅ | ✅ Approve-cluster is offline; `sync_turn` assistant content goes through the same hot-path `remember` as user content |
| V — Eval-Gated Promotion | N/A | N/A — no embedding or model changes |
| VI — Open-Protocol, Build-Don't-Reinvent | ✅ | ✅ All changes use existing schema, services, and CLI patterns |

No violations. Complexity Tracking section omitted.

## Project Structure

### Documentation (this feature)

```text
specs/006-memory-pipeline-quality/
├── plan.md              # This file
├── research.md          # Phase 0 ✅
├── data-model.md        # Phase 1 ✅
├── quickstart.md        # Phase 1 ✅
├── contracts/           # N/A — purely internal changes, no new external interfaces
└── tasks.md             # Phase 2 output (/speckit-tasks — not yet created)
```

### Source Code (repository root)

```text
src/
├── services/
│   └── review.rs         # enrich list(), add approve_cluster() / skip_cluster()
├── bin/
│   └── pamet.rs          # new CLI subcommands: review list --limit, approve-cluster, skip-cluster
└── lib.rs                # no changes (write path already complete from spec 005)

~/.hermes/plugins/pamet/
└── __init__.py           # sync_turn assistant retention; prefetch stop-word filter; budget config

tests/
└── (new unit tests in src/services/review.rs #[cfg(test)])
```

**Structure Decision**: Single-project layout, no frontend/backend split. Python plugin
is a separate file outside the Rust workspace.

## Phase 0: Research

**Status**: ✅ Complete — see [research.md](research.md)

Key decisions:
- Query filtering in Python plugin layer (not Rust binary)
- Assistant content through same `remember` path — existing write policy handles dedup
- `approve-cluster` / `skip-cluster` use existing `services::review::resolve()` — no schema changes
- `pamet_outcome` discipline is a skill/prompt concern, not infrastructure
- Prefetch budget via `plugins.pamet.prefetch_budget` config key

## Phase 1: Design & Contracts

**Status**: ✅ Complete — see [data-model.md](data-model.md), [quickstart.md](quickstart.md)

No new external contracts: all changes are internal to the Rust binary CLI and the Python
plugin. The enriched `review list` output format is documented in quickstart.md.

## Complexity Tracking

> No Constitution violations — section omitted per plan template instructions.
