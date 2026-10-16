# Pamet

An open, learnable memory layer for AI agents (see `what-we-could-build-overview.md`).

This repo currently implements **Feature 001: Memory Store MVP** — rule-based write decisions
(store/merge/supersede/skip/review), hybrid retrieval (vector + keyword, fused with RRF), and
full outcome logging. No learned component yet, by design (Constitution Principle III).

## Build & run

```sh
cargo build --release
cargo test
```

Test suite layout (`tests/`):
- `unit/` — pure logic (write-policy thresholds, RRF fusion math).
- `contract/` — library API contracts (`remember`/`recall`/`report_outcome`/`export_log`).
- `integration/` — library-level multi-step scenarios (write policy sequence, hybrid retrieval,
  outcome logging, tombstone, event-log replay/rebuild equivalence).
- `smoke/` — fast process-level sanity check that the compiled `pamet` binary starts and a basic
  remember→recall roundtrip works (`cargo test --test smoke`).
- `e2e/` — process-level run of the real CLI binary through all of quickstart.md's scenarios plus
  forget/tombstone, catching CLI wiring bugs the library-level tests can't see
  (`cargo test --test e2e_cli`).

See `specs/001-memory-store-mvp/quickstart.md` for the full, runnable validation scenarios
(write a memory, retrieve it, attach outcome feedback, export the log) — that is the canonical
"does it work" check for this feature.

## Using it / connecting a real agent

See **`docs/GUIDE.md`** for the full launch guide: CLI reference, wiring the MCP server
(`pamet-mcp`, built with `--features mcp`) into Claude Code, and a reproducible walkthrough
proving cross-session memory actually works (with the gotchas we hit getting a clean comparison
against a true no-memory baseline).

## Spec-kit artifacts

- `.specify/memory/constitution.md` — project principles and governance.
- `specs/001-memory-store-mvp/` — spec, plan, research, data model, contracts, tasks, quickstart
  for the current feature.
