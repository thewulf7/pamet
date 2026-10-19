# Pamet

An open, learnable memory layer for AI agents (see `what-we-could-build-overview.md`).

This repo implements:
- **Feature 001: Memory Store MVP** — rule-based write decisions
  (store/merge/supersede/skip/review), hybrid retrieval (vector + keyword, fused with RRF), and
  full outcome logging. No learned component yet, by design (Constitution Principle III).
- **Feature 002: Consolidation & Eval Harness** — a "sleep" pass that clusters near-duplicate
  memories into reviewed summaries, flags contradictions, and an eval harness that scores
  Pamet (and optionally Hindsight, as a baseline) against a fixed probe set.
- **Feature 003: Production-Grade Storage Backend** — an optional Postgres+pgvector backend
  (behind the `postgres-backend` Cargo feature) for genuine concurrent connections, alongside
  the unchanged embedded SQLite backend as a permanent offline/sovereign fallback, plus a
  `pamet migrate` subcommand to move an existing store onto it.

## Build & run

```sh
cargo build --release
cargo test

# Optional: Postgres+pgvector backend (default build stays fully offline/dependency-free)
cargo build --release --features postgres-backend
cargo test --release --features postgres-backend   # needs PAMET_TEST_POSTGRES_URL reachable
```

Selecting a backend is a `--db` value, not a code change: a file path (or `:memory:`) uses the
embedded SQLite store exactly as before; a `postgres://`/`postgresql://` URL (postgres-backend
build only) uses Postgres. See `specs/003-postgres-storage-backend/quickstart.md` for a full
runnable walkthrough (consolidation under live load, migrating an existing store, offline mode,
scale, and cross-backend eval parity) with real observed output.

Test suite layout (`tests/`):
- `unit/` — pure logic (write-policy thresholds, RRF fusion math).
- `contract/` — library API contracts (`remember`/`recall`/`report_outcome`/`export_log`).
- `integration/` — library-level multi-step scenarios (write policy sequence, hybrid retrieval,
  outcome logging, tombstone, event-log replay/rebuild equivalence, Postgres scale).
- `smoke/` — fast process-level sanity check that the compiled `pamet` binary starts and a basic
  remember→recall roundtrip works (`cargo test --test smoke`).
- `e2e/` — process-level run of the real CLI binary through all of quickstart.md's scenarios plus
  forget/tombstone, consolidation, Postgres concurrency, and migration
  (`cargo test --test e2e_cli`, plus the feature-gated `e2e_postgres_concurrency`/`e2e_migration`).

See `specs/001-memory-store-mvp/quickstart.md` for feature 001's validation scenarios (write a
memory, retrieve it, attach outcome feedback, export the log) and
`specs/003-postgres-storage-backend/quickstart.md` for the Postgres backend's.

## Using it / connecting a real agent

See **`docs/GUIDE.md`** for the full launch guide: CLI reference, wiring the MCP server
(`pamet-mcp`, built with `--features mcp`) into Claude Code, and a reproducible walkthrough
proving cross-session memory actually works (with the gotchas we hit getting a clean comparison
against a true no-memory baseline).

## Spec-kit artifacts

- `.specify/memory/constitution.md` — project principles and governance.
- `specs/001-memory-store-mvp/` — spec, plan, research, data model, contracts, tasks, quickstart
  for the initial feature.
- `specs/002-consolidation-eval-harness/` — same artifact set for consolidation + eval harness.
- `specs/003-postgres-storage-backend/` — same artifact set for the Postgres backend + migration.
