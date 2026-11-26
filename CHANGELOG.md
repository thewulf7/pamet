# Changelog

All notable changes to Pamet are documented here. Format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions are tagged on `master`.

## [0.6.0] — 2026-11-26

Feature 006: memory pipeline quality improvements. Fixes five issues found
during real-world use: noisy prefetch queries, missing assistant-turn retention,
inaccessible review queue, absent outcome feedback loop, and unconfigurable
prefetch budget. Also patches two correctness bugs caught by the converge loop
(duplicate review_id on cluster operations, missing `cluster_id` in list output).

### Added
- **Prefetch stop-word gate** (`~/.hermes/plugins/pamet/__init__.py`): short or
  noise-only queries (< 3 signal tokens after stop-word removal) now skip the
  `pamet recall` subprocess call entirely, eliminating false recall noise on
  messages like "who am i?", "go", "ok".  Stop-word list is a frozen set of 20
  common English function words; configurable at the Python layer without a Rust
  rebuild.
- **Configurable prefetch budget**: `plugins.pamet.prefetch_budget` in Hermes
  `config.yaml` controls how many results the prefetch call requests (default 5).
  Set to `0` to fully disable prefetch (useful during low-context sessions).
- **Assistant-turn retention**: `sync_turn()` now also writes `assistant_content`
  (up to 2 000 chars) through the same `remember` write path, so agent replies
  containing durable facts are captured, not only user messages.  Low-value gate
  applies: assistant content with < 3 tokens is silently skipped (no subprocess
  round-trip).
- **Enriched review list** (`pamet review list --limit N`): output now includes
  `cluster_id=<val>` alongside reason, source count, and summary preview so a
  human can feed the ID directly into `approve-cluster` / `skip-cluster`.
- **Batch cluster operations**: `pamet review approve-cluster <cluster_id>` and
  `pamet review skip-cluster <cluster_id>` resolve all pending review items for a
  cluster in a single command.  Verified at 287 ms on an 18 000-member cluster
  (SC-004 ≤ 1 s SLA).
- **`cluster_members(summary_item_id)` index** (`src/storage/schema.rs`): speeds
  up the `GROUP BY` subquery in `list_enriched` on stores with 1 000+ cluster
  members.  EXPLAIN confirms COVERING INDEX use on both the aggregate subquery and
  the direct join.

### Fixed
- **Duplicate review_id on cluster operations** (`src/services/review.rs`):
  `resolve_cluster_items` used a plain JOIN that produced N rows per cluster (one
  per `cluster_members` row), causing `resolve()` to error with `NotPending` after
  the first call and abort the entire approve/skip for any cluster with > 1 member.
  Fixed with `SELECT DISTINCT`.
- **`is_low_value` threshold** (`src/services/write_policy.rs`): inputs of < 3
  tokens (single word, two words) were not caught by the previous filler-word
  list; now rejected unconditionally before the filler check.
- **Question-fragment storage** (`src/lib.rs`): `extract_candidate_texts` now
  drops `?`-terminated segments so bare questions are never written as facts.
  A `hit_delimiter` flag prevents the whole-text fallback from re-emitting
  question-only input.

### Removed
- **Postgres branches in `review.rs`**: the five `#[cfg(feature = "postgres-backend")]`
  / `Backend::Postgres` blocks in `src/services/review.rs` were dormant dead code
  — `review.rs` only ever ran on SQLite.  Removed for clarity; the rest of the
  codebase continues to support the `postgres-backend` feature flag.

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
