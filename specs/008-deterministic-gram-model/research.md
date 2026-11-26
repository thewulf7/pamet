# Research: Deterministic Per-Cluster N-gram Model

Phase 0 output for `008-deterministic-gram-model`. No `NEEDS CLARIFICATION` markers
were left in `plan.md`'s Technical Context — the two open design questions below
were resolved by looking at how this project already solves structurally identical
problems, rather than by introducing new patterns.

## Decision 1: Gram order (n)

**Decision**: Trigram (n=3) as the fixed v1 constant.

**Rationale**: Spec FR-001/Assumptions require a fixed, documented order for v1.
Bigram (n=2) tends to produce locally-incoherent output (barely more than a
word-frequency shuffle); orders above trigram need proportionally more training
text per cluster to have any valid continuations at all, and real cluster sizes in
this project range as low as 2 members (consolidation's own `g.len() >= 2` filter,
`src/services/consolidation.rs:308`) — a 4-gram+ model would frequently have zero
valid continuations on the smallest real clusters, defeating the feature's own
Edge Case ("what happens when a cluster's combined text is too short"). Trigram is
the standard low-data-regime choice in classical NLP for exactly this reason.

**Alternatives considered**: Bigram (rejected — too locally incoherent, no real
benefit over trigram's training-data cost); configurable order per cluster
(rejected — spec's own Assumptions explicitly defer this until real generated
output shows a need; a second free parameter before any generated output exists to
evaluate against is premature).

## Decision 2: Serialization format for the persisted model

**Decision**: JSON (`serde_json::Value` / a `#[derive(Serialize, Deserialize)]`
struct), stored in a `TEXT`/`JSONB`-typed column — same approach `memory_items`
already uses for `embedding_json` and `entities_json` (`src/storage/schema.rs`).

**Rationale**: Matches an established, working pattern in this exact codebase. A
trigram frequency table is a nested map (`(token, token) -> {next_token: count}`)
that serializes cleanly to JSON without a custom binary format, and reusing the
existing pattern means no new serialization dependency and no new dialect-specific
column-type decision beyond what `schema.rs` already handles for the two existing
`_json` columns.

**Alternatives considered**: A custom compact binary encoding (rejected — no
measured need; SC-001's 200ms bound is about construction cost, not storage size,
and this project has not needed binary serialization anywhere else); `bincode`/
`postcard` crate (rejected — new dependency for a problem the existing JSON pattern
already solves at this scale).

## Decision 3: Deterministic sampling (the `--rng-seed` requirement)

**Decision**: A small hand-written splitmix64/xorshift-style PRNG inside
`src/services/gram.rs`, seeded directly from `--rng-seed`, rather than depending on
the `rand` crate.

**Rationale**: FR-006/SC-002 require byte-identical output given the same seed,
**across runs and across machines**. Depending on an external crate's PRNG
algorithm for this guarantee is riskier than it looks: a transitive dependency bump
could silently change `rand`'s default algorithm's output sequence for the same
seed between Cargo.lock updates, breaking SC-002 without any code change in this
feature. A ~15-line hand-written deterministic PRNG, version-locked by virtue of
living in this crate's own source, removes that failure mode entirely — the same
reasoning this project already applied when it hand-wrote FNV-1a hashing in
`embedding.rs` instead of pulling in a hashing crate.

**Alternatives considered**: `rand` crate with `StdRng::seed_from_u64` (rejected —
the crate's docs do not guarantee the seeded sequence is stable across `rand`
major versions, which is precisely the determinism risk this feature cannot
accept); `getrandom`/OS entropy (rejected outright — not deterministic by
definition, fails FR-006 on its face).

## Decision 4: Where generation stops early (Edge Case: chain runs out of valid continuations)

**Decision**: `generate()` returns as soon as the current `(token, token)` key has
no entries in the trigram table, rather than falling back to a lower-order model or
restarting from a random start state.

**Rationale**: Spec's Edge Case is explicit: "never pads with fabricated content
outside the model's own n-gram table." A fallback to a lower-order (bigram/unigram)
model is a legitimate technique in general-purpose language modeling (Katz
backoff), but it would mean occasionally emitting a transition that is *not* in the
cluster's actual trigram table — a smaller but real violation of SC-003's "100% of
output tokens are drawn from the source cluster's own member texts" if interpreted
at the transition level rather than just the token level. Stopping early keeps the
guarantee unambiguous and trivial to test.

**Alternatives considered**: Katz backoff to lower order (rejected — see above,
and it is unnecessary complexity for this feature's narrow scope of "cheap local
paraphrase," not general-purpose generation quality).
