# Research: Write-Policy Near-Duplicate Pre-Filter

No `[NEEDS CLARIFICATION]` markers were left in spec.md's Technical Context (none were
raised in the spec itself either) — this research resolves the *implementation*
decisions `/speckit-plan` is responsible for, building on the Assumptions spec.md
already recorded.

## Decision 1: Pre-filter mechanism

**Decision**: Use the existing keyword index (`services::retrieval_keyword::search()`,
SQLite FTS5 / Postgres `tsvector`, same tokenizer as the embedder) to produce a bounded
candidate-id list, then load and exact-cosine-compare only those rows — not a new
index structure.

**Rationale**:
- Constitution Principle VI (open-protocol, build-don't-reinvent): this index already
  exists, is already tested (`retrieval_keyword.rs`, used by `recall()`), and already
  excludes tombstoned rows.
- **Provable correctness for this embedder specifically**: `services/embedding.rs`'s
  `embed()` is a deterministic *hashed bag-of-words* over `tokenize()` — the exact same
  tokenizer `retrieval_keyword.rs`'s FTS/tsvector query already uses. Two texts with
  zero shared tokens produce dot-product-zero embeddings (barring a 1-in-4096 hash
  bucket collision, which is possible but does not change the argument's practical
  force). Therefore **any pair scoring at or above `MERGE_SIMILARITY_THRESHOLD` (0.85)
  or triggering `detect_contradiction` (which already requires `shared_tokens >= 2`)
  necessarily shares tokens** — a keyword pre-filter built on the same tokenizer cannot
  exclude such a pair from its candidate set. This is not a probabilistic approximation
  for this specific embedder; it is exact, which is why FR-003 (never miss a true
  near-dup in the decisive bands) can be satisfied with a simple top-N keyword lookup
  rather than a tunable recall/precision knob.
- Verified directly (not assumed): `tests/integration/test_consolidation.rs`'s own
  FACT_A/FACT_B near-duplicate pairs ("the quarterly report is due friday" / "remember
  the quarterly report deadline is this friday afternoon") share tokens
  (`quarterly`, `report`, `friday`) — consistent with the mathematical argument above,
  not contradicting it.

**Alternatives considered**:
- *LSH (locality-sensitive hashing) bucket index*: would work but is a new structure to
  build, test, and maintain for a guarantee the existing keyword index already provides
  for free given this embedder's construction — rejected per Principle VI.
- *Time-window bounding (only compare against the last K items)*: rejected — contradicts
  the whole point of near-duplicate detection, which must catch a repeated fact from
  months ago, not just a recent one; feature 004's real corpus spans years.
- *A smaller, separate in-memory embedding index (e.g., a coarse quantized sketch)*:
  rejected — duplicates work the keyword index already does, and would need its own
  correctness argument instead of inheriting one for free from the embedder's own
  construction.

## Decision 2: Candidate-set size bound (top-N from the keyword pre-filter)

**Decision**: N = 200 (a constant, not configurable in this feature). The keyword
search already ranks by BM25/`ts_rank`, so the exact scan still gets the
most-lexically-relevant candidates first if the true overlap count exceeds N.

**Rationale**: Bounds the pre-filter's own cost (SC-004: no measurable overhead on a
small store, where N=200 is simply "every item" anyway) while N=200 is far larger than
the number of items that could realistically share 2+ meaningful tokens with a single
new fact in a well-tokenized corpus — real near-duplicate clusters observed in feature
004 and feature 002's tests were pairs or small groups, not hundreds. If this bound ever
proves too tight in practice, it is a one-constant change, not an architecture change.

**Alternatives considered**: An unbounded candidate set (every keyword match, no cap) —
rejected because a single extremely common token (hash-bucket collision aside) could in
principle still return a large candidate set on pathological input; a fixed bound keeps
the worst case bounded and verifiable, consistent with SC-002's letter (≤2x batch-time
growth, not "exactly flat").

## Decision 3: Fallback when the keyword pre-filter returns zero candidates

**Decision**: Zero keyword candidates → the write proceeds directly to
`Decision::Store` (`novel_store` rule) without running any cosine comparison at all,
exactly as if the (now-empty) candidate set had been exact-scanned and found nothing
above any threshold.

**Rationale**: This is not a special case bolted on — it is the mathematically correct
outcome already: per Decision 1's argument, zero shared tokens implies the exact scan
against the full store would also have found `max_similarity` indistinguishable from 0
(no merge, no contradiction, no ambiguous-review band). Skipping the comparison entirely
in this case is a free optimization, not a behavior change — this is exactly the case
that was free before and stays free now, and it is also the single largest source of
feature 004's observed slowdown (every genuinely novel fact was still being compared
against the *entire* store before this fix).

## Decision 4: Regression verification strategy (FR-004/FR-006, SC-002/SC-003)

**Decision**: Two-part verification, not one:
1. **Correctness**: run the existing `tests/unit/test_write_policy_rules.rs`,
   `tests/integration/test_write_policy.rs`, and `tests/integration/test_consolidation.rs`
   unchanged — SC-003 requires byte-identical pass/fail outcomes, not new assertions.
2. **Performance, on real data**: a new `tests/integration/test_write_policy_scale.rs`
   seeds several thousand *topically-overlapping* synthetic-but-realistic items
   (short, templated, high-token-overlap sentences — not feature 004's actual personal
   data, which cannot live in a committed test fixture) and asserts per-batch cost stays
   within SC-002's 2x bound; separately, the *real* feature-004 Hindsight corpus is
   re-migrated once by hand (not as an automated/committed test, since it depends on a
   live local Hindsight instance with personal data) to produce the actual SC-001 number
   for the record, exactly as feature 004 itself did for the original diagnosis.

**Rationale**: A committed automated test cannot depend on a live personal Hindsight
instance (not reproducible in CI, not something to commit personal data into) — but
SC-001's claim must still be backed by one real measurement, not only a synthetic
fixture, learning directly from how a synthetic benchmark misled the initial
investigation in feature 004. The synthetic scale test's job is regression-guarding in
CI going forward; the one-time real re-migration's job is proving the actual numbers in
spec.md's Results section, same split feature 004 used between its committed tests and
its spec.md Results narrative.

**Alternatives considered**: Committing an anonymized sample of the real Hindsight
export as a test fixture — rejected for this feature (privacy: even anonymized, real
personal/work conversational content should not be committed to a public-facing repo
without separate, explicit review) in favor of the synthetic-but-overlapping fixture
plus a documented one-time real measurement.
