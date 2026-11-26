# Data Model: Deterministic Per-Cluster N-gram Model

## ClusterGramModel

A derived artifact — one row per consolidated cluster, replaced (not versioned) when
that cluster's membership changes and consolidation touches it again.

| Field | Type | Notes |
|---|---|---|
| `cluster_id` | TEXT (PK, FK → `cluster_members.cluster_id`) | Which cluster this model was built from. |
| `consolidation_run_id` | TEXT (FK → `consolidation_runs.id`) | Provenance per Constitution Principle I/II — the run that produced this exact model. |
| `gram_order` | INTEGER | Fixed at `3` for v1 (research.md Decision 1); stored explicitly so a future order change is detectable/migratable rather than silently assumed. |
| `vocab_size` | INTEGER | Count of distinct tokens observed in the cluster's combined member text. Reported by `pamet gram show` (FR-007). |
| `training_token_count` | INTEGER | Total tokens the model was built from. Reported by `pamet gram show` (FR-007). |
| `table_json` | TEXT (JSON) | The trigram frequency table: `{"tok_a\u0000tok_b": {"tok_c": count, ...}, ...}` — a two-token key (joined with a NUL separator, which cannot appear in a tokenized string) mapping to a next-token → count distribution. Mirrors `memory_items.embedding_json`'s existing JSON-column pattern (research.md Decision 2). |
| `created_at` | TEXT (ISO-8601) | Standard timestamp column, matching every other table in `schema.rs`. |

**Validation rules** (enforced in `src/services/gram.rs`, not the DB layer, matching
this project's existing pattern of validating in Rust before the SQL layer):
- `gram_order` MUST equal the module's `GRAM_ORDER` constant (3) — a mismatch would
  mean a replay-time version skew the project has no migration path for yet (out of
  scope for v1; the constant simply does not change without a new feature).
- A model is never persisted for a cluster with `training_token_count` < `gram_order`
  (FR-003/Edge Case: "too short to be meaningful") — `build_for_cluster()` returns
  `None` in that case, and consolidation logs the cluster as skipped-for-gram rather
  than erroring.

**State transitions**: Created once per consolidation run that first builds a model
for a cluster. Replaced wholesale (not patched) on a later run if that cluster's
membership changed (FR-004 — unchanged membership means no rebuild at all, so this
is the only case a row is overwritten). Never deleted directly — if the owning
cluster is ever dissolved, the gram model row becomes orphaned but inert; cleanup is
out of scope for v1 (same deferred-cleanup posture the project already takes with
`review_queue` rows per existing code).

## GramGenerationRequest (CLI-level only — not persisted)

| Field | Type | Notes |
|---|---|---|
| `cluster_id` | TEXT | Required. No persisted model ⇒ specific error (FR-009), never silent empty output. |
| `rng_seed` | Option\<u64\> | Optional. Omitted ⇒ seed derived from current time (non-deterministic by choice, since FR-006's determinism guarantee is explicitly conditioned on *supplying* a seed). |
| `max_tokens` | Option\<usize\> | Optional soft cap on generated length; generation may still stop earlier per research.md Decision 4. |

## Relationships

```text
consolidation_runs (existing)
        │ 1
        │
        │ N
cluster_members (existing) ──┐
        │ cluster_id         │ cluster_id (same column, not a new FK — a cluster's
        │                    │ identity already comes from cluster_members)
        ▼                    ▼
  [cluster's member      cluster_gram_models (NEW)
   memory_items,               │
   existing]                   │ consolidation_run_id
                                ▼
                        consolidation_runs (existing)
```

No new relationship to `memory_items` directly — a gram model is derived from a
cluster's member *texts* at build time, but (per FR-008/data model intent) it is a
snapshot, not a live join; rebuilding from the event log reproduces it from the same
inputs the original consolidation run saw, not from whatever `memory_items` rows
happen to exist at replay time.
