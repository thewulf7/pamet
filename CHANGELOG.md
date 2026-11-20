# Changelog

All notable changes to Pamet are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions are tagged on `master`.

## [0.5.0] — 2026-10-20 (first tagged release)

First tagged release, covering everything built to date (features 001-005). No prior
tags exist for this repo, so this release's notes cover the full history rather than
a diff against a previous tag.

### Added
- **Feature 001 — Memory Store MVP**: rule-based write policy (store/merge/supersede/
  skip/review, no learned component by design — Constitution Principle III), hybrid
  retrieval (vector + keyword, fused with RRF), full outcome logging, embedded SQLite
  storage, `pamet` CLI, event-sourced spine (Constitution Principle I).
- **Feature 002 — Consolidation & Eval Harness**: a "sleep" pass that clusters
  near-duplicate memories into reviewed summaries and flags contradictions, plus an eval
  harness scoring Pamet (and optionally Hindsight as a baseline) against a fixed probe set.
- **Feature 003 — Postgres Storage Backend**: optional Postgres+pgvector backend (`postgres-backend`
  Cargo feature) for genuine concurrent connections, selected purely by the `--db` connection
  string (no code change) alongside the unchanged SQLite fallback; `pamet migrate` to move an
  existing store onto it; fixed during convergence: an r2d2 connection-pool default of 30s meant
  `Store::open()` against an unreachable Postgres host didn't actually fail fast (FR-007) — now
  an explicit 5s timeout.
- **Feature 004 — Hindsight → Pamet Migration + Comparison** (`baseline-hindsight` Cargo feature):
  `pamet import-hindsight` migrates a live Hindsight bank through the real `remember()` write
  path (not a bulk insert) so write-policy decisions apply to migrated data exactly as they would
  to a live write, with a resumable side-SQLite ledger; `pamet compare-hindsight` runs a fixed
  query set against both systems and reports latency/success per query. Run against a real
  production Hindsight bank (~15,400+ memories) as part of this release; see Known Issues below
  for what that run found.
- **Feature 005 — Fix Pamet's Write-Policy Near-Dup Scan**: feature 004's real migration exposed
  genuine superlinear write cost (4.4s → 101.9s per 200-item batch, climbing) on
  topically-overlapping real conversational data — every write compared the candidate against
  every active item via exact cosine similarity. Fixed with a keyword-index pre-filter
  (`candidate_items()`, reusing the existing BM25-style retrieval index, capped at 200
  candidates, zero-overlap short-circuits to empty) ahead of the exact scan. Does **not** change
  any write-policy decision threshold — only narrows the candidate set (Constitution Principle
  III: rules untouched). Verified flat-cost on the real migrated corpus and on a dedicated
  scale/correctness regression test, on both the SQLite and Postgres backends, including a
  planted contradiction-band case (not just the merge band) and a direct small-store timing
  assertion.

### Fixed (operational, not a code change)
- Repaired 5 corrupted Postgres TOAST chunks (contiguous OIDs) in the user's live Hindsight
  instance's own `memory_units.embedding` column that were causing every `recall()` call to fail
  with `DataCorruptedError`. This is a fix to the operator's Hindsight deployment data, not to
  Pamet's code — called out here because it's what made feature 004's comparison harness produce
  a real two-sided result instead of Hindsight failing 0/18.

### Known findings from the real-data runs (not current defects)
- The original feature-004 migration run against live data was deliberately stopped partway
  (2,936/~15,400+ items) once the superlinear trend was clearly confirmed, rather than letting it
  run for hours — this is exactly what feature 005 fixes. A full re-run to completion with the
  fix applied is a natural follow-up, not yet done as part of this release.
- The real Pamet-vs-Hindsight comparison (after the TOAST repair) shows Pamet responding ~2.7x
  faster (≈920ms vs. ≈2,510ms avg per query at the then-current corpus size) but returning a
  fixed top-5 vs. Hindsight's LLM-extracted 30-54 results per query — a genuine quality/speed
  tradeoff, not a bug on either side.
