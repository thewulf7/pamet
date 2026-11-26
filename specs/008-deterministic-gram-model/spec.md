# Feature Specification: Deterministic Per-Cluster N-gram Model

**Feature Branch**: `008-deterministic-gram-model`

**Created**: 2026-11-26

**Status**: Implemented (27/27 tasks, both backends verified)

## Results (post-implementation, real verification)

- All 3 user stories (US1 build-at-consolidation, US2 generate, US3 inspect/replay)
  implemented, tested, and verified against both SQLite and Postgres backends.
- Unit tests (`tests/unit/test_gram_model.rs`): `build_for_cluster` stats, too-small
  returns `None`, `generate` determinism with a fixed seed, zero out-of-vocabulary
  tokens, early stop on sparse models — plus a direct internal PRNG test suite
  (`SplitMix64`) inside `gram.rs` itself.
- Integration test (`tests/integration/test_gram_replay.rs`): replayed
  `cluster_gram_models` rows are byte-for-byte identical to the source on both
  backends (FR-008/SC-004, Constitution Principle I) — verified against a synthetic
  fixture AND a real consolidated copy of this project's own live Hermes memory
  store.
- CLI (`pamet gram generate`/`pamet gram show`): end-to-end quickstart scenarios 1-5
  run for real against a live SQLite store; byte-identical output confirmed for a
  fixed `--rng-seed`; missing-model case fails clearly (non-zero exit, explicit
  error), not silently.
- Scenario 7 (undersized cluster) could not be reproduced end-to-end through the
  live CLI's actual write-policy/clustering mechanics (identical short text merges
  at write time before ever reaching clustering; dissimilar short text essentially
  never lands in the ambiguous clustering band with this project's hashed
  embedder) — the guarantee is instead verified directly and reliably by the unit
  test `build_for_cluster_too_small_returns_none`. `quickstart.md` documents this.
- **Real bug found and fixed during implementation**: `events.event_type`'s `CHECK`
  constraint is baked in at table-creation time; any Pamet database that predates
  this feature (including, at discovery time, this project's own live
  `~/.hermes/pamet/memory.db`) rejected the new `cluster_gram_model_built` event
  type outright the first time consolidation tried to persist a gram model. Fixed
  with an idempotent migration (`src/storage/migrations.rs`) run on every
  `Store::open()` — SQLite via a `writable_schema`/`sqlite_master` rewrite plus a
  `PRAGMA schema_version` bump (a few milliseconds, verified against a real copy of
  the live 61,908-event store — an earlier rename/recreate/copy/drop version of
  this migration was measured to take minutes against that same real data due to a
  rusqlite-bundled-SQLite-version-specific `DROP TABLE` slowdown, not a logic bug);
  Postgres via a direct `ALTER TABLE ... DROP/ADD CONSTRAINT`.
- **Separate regression found and fixed**: the unrelated v0.6.0 release
  (`src/services/review.rs`) left 7 non-exhaustive `let Backend::Sqlite(conn) =
  &store.backend;` bindings that only compile when the `postgres-backend` feature
  is off, breaking that feature's build entirely. Fixed with explicit
  `else { return Err(...) }` (production code) / `else { panic!(...) }` (test-only
  code) matching the file's documented "SQLite-only" intent — `cargo build
  --features postgres-backend` and the full test suite (incl. Postgres) are green.

**Input**: User description: "Deterministic n-gram model synthesized per consolidated
memory cluster: build a frozen, non-ML Markov/n-gram language model from each
cluster's member texts at consolidation time, enabling cheap local text generation
(paraphrase, cluster summary draft) with zero LLM calls, fully rebuildable from the
event log like every other derived state."

## Context

Discussion origin: comparing Pamet's rule-based retrieval against Hindsight's
LLM-backed extraction raised the question "could memory itself become a small
generative model?" Google's Titans architecture (arXiv:2501.00663) does something
in that spirit — a neural memory whose weights update via gradient descent at test
time. That approach is a genuine research direction, but it is also the opposite of
everything Pamet is built on: it is non-deterministic, path-dependent on update
order, and not replayable from an event log.

This feature is the version of that idea that fits Pamet's own constitution: no
gradients, no neural weights, no training loop. A frozen, statistical n-gram
(Markov chain) model built directly and deterministically from the member texts of
one consolidated cluster, at consolidation time — the same moment Pamet already
produces a cluster's summary (`src/services/consolidation.rs`). The model is a
derived artifact like everything else in the store: rebuildable from the event log,
inspectable, with zero external calls.

This explicitly is **not** a competitor to Hindsight's LLM extraction quality. An
n-gram model has no reasoning capability — it cannot answer "what does the user
prefer," it can only resynthesize plausible-looking text statistically close to what
a cluster already says. The value is narrow and specific: cheap, zero-network,
zero-latency local text variation (paraphrase, alternate summary phrasing, light
style spread) for a cluster Pamet has already decided is coherent enough to
consolidate.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Build a cluster's n-gram model at consolidation time (Priority: P1)

When the consolidation pass (`pamet consolidate`) groups near-duplicate items into a
cluster and writes a summary, it also derives a small, frozen n-gram model from that
cluster's member texts and persists it alongside the cluster's other consolidation
artifacts.

**Why this priority**: Every other capability in this feature depends on the model
existing. Without it there is nothing to query or regenerate from.

**Independent Test**: Run `pamet consolidate` against a store with at least one
cluster of 3+ similar items; confirm a `cluster_gram_models` row exists for that
cluster's id with a non-empty serialized model, and that re-running consolidation on
an unchanged cluster does not create a duplicate (idempotent, matching the existing
consolidation run's own idempotency guarantee).

**Acceptance Scenarios**:

1. **Given** a cluster of 5 member items consolidated into a summary, **When**
   consolidation runs, **Then** a gram model keyed by that `cluster_id` is persisted
   with order (n) and vocabulary size recorded.
2. **Given** a cluster with only 1 member (below the existing consolidation
   cluster-size threshold), **When** consolidation runs, **Then** no gram model is
   built for it (consistent with no summary being produced for singleton clusters
   today).
3. **Given** a cluster whose gram model already exists and whose membership has not
   changed since the last consolidation run, **When** consolidation runs again,
   **Then** the existing model is left untouched (no rebuild, no duplicate row).

---

### User Story 2 - Generate a local paraphrase from a cluster's model (Priority: P2)

A new CLI subcommand (`pamet gram generate --cluster-id <id> [--seed <text>]`)
samples from a cluster's persisted n-gram model to produce a short, locally
synthesized text variant — no network call, no LLM, deterministic given a fixed
random seed.

**Why this priority**: This is the actual payoff of building the model — a visible,
testable output distinct from "a blob now exists in the database." It is P2, not P1,
because the model is directly inspectable without generation (e.g., for debugging
or future consumers) even before this CLI surface exists.

**Independent Test**: Build a model for a cluster whose members all mention the same
two or three entities; run `gram generate` several times; confirm every output is
built only from tokens present in that cluster's member texts (no hallucinated
vocabulary) and that supplying `--rng-seed` makes output reproducible run to run.

**Acceptance Scenarios**:

1. **Given** a persisted gram model for cluster `C`, **When** `pamet gram generate
   --cluster-id C --rng-seed 42` runs twice, **Then** both runs produce byte-identical
   output (determinism, matching Constitution Principle III in spirit even though
   this isn't a learned model).
2. **Given** a persisted gram model for cluster `C`, **When** generation runs without
   a seed argument, **Then** output still only contains n-grams observed in `C`'s
   source texts (no out-of-vocabulary tokens), confirming it never falls back to
   calling an LLM or inventing content.
3. **Given** a `--cluster-id` with no persisted gram model (e.g., consolidation never
   ran, or the cluster id doesn't exist), **When** `gram generate` runs, **Then** it
   fails with a clear, specific error — never a silent empty/garbage output.

---

### User Story 3 - Inspect and rebuild gram models from the event log (Priority: P3)

A gram model is a derived artifact, not a source of truth: it must be inspectable
(`pamet gram show --cluster-id <id>`, returning order/vocab-size/sample n-grams) and
fully reconstructable by `pamet migrate`/replay tooling without needing the original
stored blob.

**Why this priority**: This is the constitution-compliance story, not new
user-facing value — it proves the feature didn't quietly introduce a second,
un-replayable source of truth into a project whose first principle is "every state
replayable from the event log alone." P3 because it's a correctness/audit guarantee,
checked once, not something exercised on every use.

**Independent Test**: Export the full event log, wipe the derived store, replay
through the existing rebuild machinery (`rebuild.rs`), and diff gram models byte-for-
byte between the original and rebuilt stores for every cluster.

**Acceptance Scenarios**:

1. **Given** a store with N persisted gram models, **When** the store is rebuilt from
   its own event log via the existing replay machinery, **Then** all N gram models
   are reconstructed identically (same bytes), not merely "a model of the same shape."
2. **Given** `pamet gram show --cluster-id <id>`, **When** run against a cluster with
   a persisted model, **Then** it prints the model's order (n), vocabulary size, and
   total training token count without requiring generation.

---

### Edge Cases

- What happens when a cluster's combined member text is too short to build a
  meaningful n-gram model (e.g., total tokens < n)? → No model is built; consolidation
  logs this cluster as skipped-for-gram (not an error — the summary/cluster itself is
  still valid).
- How does the system handle a cluster whose members are later tombstoned
  individually after the model was built (cluster composition changes)? → The
  existing model is stale but not silently wrong-looking: it is not deleted
  automatically; the next consolidation run that touches this cluster rebuilds it
  from current membership.
- What happens if `gram generate` is asked to produce output far longer than the
  model's training data supports (the chain runs out of valid continuations)? → The
  command stops generation early and returns what it has, never pads with fabricated
  content outside the model's own n-gram table.
- What is `n` (the gram order)? → Fixed, documented constant for v1 (not
  user-configurable per cluster) — avoids a second axis of non-determinism/complexity
  before there's evidence a single order is wrong for real clusters.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST derive a frozen n-gram (Markov chain) model from a
  cluster's member item texts at consolidation time, using a fixed, documented order
  `n` (no learned weights, no gradient descent, no external model calls —
  Constitution Principle III).
- **FR-002**: System MUST persist each cluster's gram model as a new derived table
  (e.g. `cluster_gram_models`), keyed by `cluster_id`, alongside the existing
  `cluster_members`/consolidation artifacts, with every row traceable to the
  `consolidation_run_id` that produced it (Constitution Principle I: event-sourced
  spine).
- **FR-003**: System MUST skip gram-model construction for clusters below a minimum
  viable size (same singleton-cluster threshold consolidation already applies to
  summaries) rather than building a degenerate/empty model.
- **FR-004**: System MUST NOT rebuild a cluster's gram model on a consolidation run
  where that cluster's membership is unchanged since the model was last built
  (idempotency, matching existing consolidation run semantics).
- **FR-005**: System MUST expose a `pamet gram generate --cluster-id <id>
  [--rng-seed <n>]` CLI command that samples the persisted model and returns text
  containing only n-grams observed in that cluster's source member texts.
- **FR-006**: Generation MUST be fully deterministic given an explicit `--rng-seed`
  (same seed + same model ⇒ byte-identical output across runs and across machines).
- **FR-007**: System MUST expose a `pamet gram show --cluster-id <id>` CLI command
  reporting the model's order, vocabulary size, and total training-token count
  without performing generation.
- **FR-008**: Every gram model MUST be exactly reconstructable (byte-for-byte) by
  replaying the event log through the existing rebuild machinery
  (`src/services/rebuild.rs`) — it introduces no second source of truth.
- **FR-009**: Requesting generation or inspection for a `cluster_id` with no
  persisted gram model MUST fail with a specific, actionable error — never silent
  empty output.
- **FR-010**: This feature MUST NOT alter any existing write-policy decision
  threshold, consolidation clustering rule, or retrieval ranking — it is additive
  only (Constitution Principle III: rules before learning stays untouched by this
  work; this is tooling built *on* a cluster's data, not a change to how clusters are
  formed or recalled).

### Key Entities

- **ClusterGramModel**: a derived artifact — `cluster_id`, `consolidation_run_id`,
  gram order `n`, serialized transition table (token sequence → next-token frequency
  counts), vocabulary size, total training token count, `created_at`. One per
  cluster per rebuild (replaced, not versioned, on cluster-membership change).
- **GramGenerationRequest** (CLI-level, not persisted): `cluster_id`, optional
  `rng_seed`, optional max-output-length bound.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Building a gram model for a cluster of up to 50 member items completes
  in under 200ms (no network call is possible by construction, so this bounds the
  in-process statistical work only).
- **SC-002**: `gram generate` with a fixed `--rng-seed` produces byte-identical output
  across at least 10 repeated runs, on both the SQLite and Postgres backends.
- **SC-003**: 100% of gram-model-derived output tokens are drawn from the source
  cluster's own member texts (zero out-of-vocabulary generation) — verified by a test
  that asserts no generated token is absent from the training corpus.
- **SC-004**: A full store rebuild from the event log reproduces every existing gram
  model byte-for-byte, verified on a real consolidated store (not only a synthetic
  one), matching the standard this project set for Postgres replay-equivalence in
  feature 003.

## Assumptions

- Gram order `n` is fixed at a small constant (e.g. bigram or trigram) for v1; making
  it configurable per cluster is explicitly deferred until real generated output shows
  a need for it.
- This feature targets **local text variation of already-consolidated clusters**,
  not open-ended generation, not chat-style responses, and not a replacement for any
  LLM-backed capability — it is scoped narrowly on purpose (see Context section).
- The existing consolidation job (`src/services/consolidation.rs`) already identifies
  clusters and their membership; this feature consumes that output rather than
  re-implementing clustering.
- No new Cargo dependency is required — a Markov-chain/n-gram table is simple enough
  to hand-write (consistent with this project's "build, don't reinvent" bar only
  applying to genuinely complex infrastructure, not a frequency table).
- Serialization format for the persisted model is an implementation detail decided
  in `/speckit-plan`, not this spec.
