# Phase 0 Research: Consolidation Job & Eval Harness

## 1. Clustering algorithm for near-duplicate consolidation

- **Decision**: Threshold-graph clustering — compute pairwise cosine
  similarity between all active items' existing bag-of-words embeddings
  (`embedding::cosine_similarity`, already shipped); connect any pair whose
  similarity ≥ `MERGE_SIMILARITY_THRESHOLD` (0.85, the same constant the
  live write policy already uses); take connected components as clusters.
- **Rationale**: Reuses an existing, already-tuned, already-logged threshold
  instead of inventing a second tunable surface for the same concept
  ("how similar is similar"). Deterministic, auditable, and trivially
  rebuildable from the event log (Principle I) — no new model to version.
- **Alternatives considered**: k-means / HDBSCAN over the embedding space —
  rejected as overkill for sparse hashed bag-of-words vectors at MVP scale,
  and non-deterministic cluster counts would complicate SC-003's
  byte-identical-repeat-run requirement.

## 2. Summary generation for a cluster

- **Decision**: Deterministic rule, not an LLM/abstractive summary: the
  `ConsolidatedSummary`'s content is the longest member's content (ties
  broken by earliest `created_at`); every cluster member is recorded as
  provenance via a new `cluster_members` join table (one summary, many
  sources — `supersedes_id` stays 1:1 and is left untouched).
- **Rationale**: Constitution Principle III — a learned/LLM summarizer is
  exactly the kind of capability that needs a rule-based predecessor
  logging real data first. "Longest content wins" is crude but fully
  deterministic, testable, and gives the eval harness and Phase 3 something
  concrete to beat later.
- **Alternatives considered**: LLM-generated abstractive summary — explicitly
  deferred; this phase's job is to produce the rule-based baseline and the
  logged cluster/summary events that a future learned summarizer would be
  evaluated against.

## 3. Decay formula

- **Decision**: Binary threshold + fixed multiplier. An item with
  `use_count == 0` (since creation) or whose `last_used_at` is older than a
  configurable `DECAY_AFTER_DAYS` (default 30) gets a `decay_penalty`
  multiplier (default 0.5) applied to its score in the existing RRF fusion
  step (`retrieval_fusion.rs`). One more use resets the item to full
  priority. Decay never deletes or tombstones (Principle II; `forget`
  remains the only destructive-in-effect path, and even that is a
  tombstone).
- **Rationale**: Matches the existing threshold-based style of
  `write_policy.rs` (Principle III: simple, hand-written, fully logged
  rules). Fully reversible by construction, so there is no "data loss"
  review-queue implication like contradiction resolution has.
- **Alternatives considered**: Continuous Ebbinghaus-curve decay — rejected
  as unnecessary complexity for a rule-based MVP; harder to reason about
  and test than a fixed threshold + multiplier.

## 4. Contradiction detection during consolidation

- **Decision**: Reuse the existing `detect_contradiction()` heuristic
  unchanged, applied pairwise within the similarity band
  `[CONTRADICTION_SIMILARITY_FLOOR, MERGE_SIMILARITY_THRESHOLD)` among
  active items (the same band the live write policy already treats as
  "maybe the same thing, maybe not") rather than writing new logic.
- **Rationale**: One contradiction-detection rule, one place it's defined,
  used identically at write time and at consolidation time — avoids two
  heuristics silently drifting apart.
- **Alternatives considered**: A dedicated small-LM contradiction judge (as
  named in the original design docs) — explicitly out of scope for this
  phase per Principle III; this phase's rule-based version is itself the
  predecessor that would need to exist (and log outcomes) before that.

## 5. Eval harness scoring methodology

- **Decision**: Fully deterministic pass/fail per probe. Each probe
  specifies a `given` setup (a sequence of `remember`-equivalent
  operations), an `action` (a `recall` or `remember` call), and an
  `expected` outcome (exact item content, or "no relevant memory found" for
  abstention probes). The harness runs each probe against a fresh, isolated
  temp SQLite DB, compares actual vs. expected, and records pass/fail.
  Category score = passed / total for that category; no partial credit, no
  fuzzy/semantic grading.
- **Rationale**: SC-003 requires byte-identical scores on repeat runs — any
  fuzzy or LLM-graded scoring would violate that and would itself be a
  learned component needing its own Principle III predecessor.
- **Alternatives considered**: LLM-as-judge scoring — rejected for the same
  Principle III / determinism reasons; could be revisited once there is a
  rule-based judge logging data to train against.

## 6. Eval probe fixture format (FR-009)

- **Decision**: Versioned JSON file (`tests/fixtures/eval_probes_v1.json`):
  `{ "version": "v1", "probes": [{ "id", "category", "given": [...],
  "action": {...}, "expected": {...} }] }`. The `given` operations mirror
  the shape of real `remember` calls (same fields: text, source_id,
  explicit_remember) — the same shape the real event log already uses —
  so swapping in real usage-derived fixtures later (once the Hermes plugin
  has accumulated data) means writing a converter from `export-log` output
  into this same schema, not a harness rewrite.
- **Rationale**: Directly satisfies the resolved FR-009 clarification
  (synthetic now, additive swap to real data later).
- **Alternatives considered**: Adopting LoCoMo/LongMemEval's native fixture
  format directly — deferred; those benchmarks' richer formats are a
  reasonable future upgrade once Pamet needs to compare against literature
  baselines beyond Hindsight (out of scope per FR-007's resolution).

## 7. Hindsight baseline integration (FR-007)

- **Decision**: Direct HTTP calls from Rust via `reqwest` (blocking,
  `json` feature, new optional dependency behind a `baseline-hindsight`
  Cargo feature) to the local Hindsight REST API
  (`http://localhost:8888`), hitting the same routes the existing
  Python `hindsight_client` SDK uses under the hood (`POST /v1/memory` for
  retain, `POST /v1/memory/search` or `/v1/context/query` for recall) —
  verified reachable and working against the local `hindsight` Docker
  container earlier this session.
- **Rationale**: Keeps the integration fully native (no Python subprocess,
  no second runtime) — consistent with the project's standing "no
  interpreted-runtime dependency" preference. `reqwest` is feature-gated so
  the mandatory, network-free parts of this phase (consolidation, eval
  harness against Pamet alone) are completely unaffected if the feature is
  never enabled.
- **Alternatives considered**: Shell out to a small Python script using the
  official `hindsight_client` SDK — rejected; reintroduces exactly the
  interpreted-runtime wrapper pattern already rejected for this project.
  A Rust `hindsight-client`-equivalent SDK does not exist, so calling the
  documented REST routes directly is the lowest-complexity native option.

## 8. Latency-isolation measurement (SC-001)

- **Decision**: A dedicated e2e test (`tests/e2e/test_consolidation_e2e.rs`)
  spawns a background thread issuing `recall` calls at a fixed interval,
  starts a consolidation run concurrently, and compares p95 latency of the
  recall calls against a baseline run with no consolidation in progress.
  Implementation detail (not fixture-dependent) — full task breakdown in
  `tasks.md`.
- **Rationale**: SC-001 needs a measured comparison, not just an assertion
  that "it's architecturally separate" — a real concurrent test is the only
  way to actually prove Principle IV holds for this specific job.
- **Alternatives considered**: Static code-review-only verification (e.g.
  "consolidation never calls remember/recall internally") — rejected as
  insufficient; it doesn't catch lock contention on the shared SQLite file,
  which is the realistic failure mode here.
