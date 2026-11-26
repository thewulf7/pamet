# Data Model: Real Semantic Memory Search

## Entity: `memory_items_vec` (new, SQLite `vec0` virtual table)

Per Decision 5 (research.md): a `vec0` virtual table, one row per active (never
tombstoned-and-purged) `memory_items.id`.

| Column | Type | Notes |
|---|---|---|
| `item_id` | `TEXT` (vec0 rowid-aliasing primary key) | FK to `memory_items.id`, not enforced via SQL FK (vec0 virtual tables don't support ordinary FK constraints) — enforced at the application layer, same pattern already used for `cluster_members`/other derived tables that reference `memory_items`. |
| `embedding` | `float[384]` (vec0 vector column) | The MiniLM output vector. Dimension is fixed per Decision 3; a future model change requires a new table generation, not an in-place resize (vec0 vector columns are fixed-dimension). |

**Lifecycle**: A row is inserted/updated whenever `embedding_backfill` or the live
write path (`remember()`) produces a fresh embedding for an item. A tombstoned
item's row is **not** deleted immediately — same reversibility principle as
`memory_items` itself (Principle II) — but recall/search queries MUST join
against `memory_items` and exclude tombstoned rows, exactly as
`retrieval_keyword.rs`'s existing FTS5 query already does.

## Entity: `embedding_models` (new, ordinary table)

Per Decision 5 — tracks which frozen model/version produced which items'
vectors (FR-009), so a future model swap can be detected and handled
deliberately.

| Column | Type | Notes |
|---|---|---|
| `id` | `TEXT PRIMARY KEY` | A stable identifier for one frozen model+version, e.g. `all-MiniLM-L6-v2-onnx-fp32-v1`. |
| `dimension` | `INTEGER NOT NULL` | 384 for the initial model (Decision 3). Recorded explicitly rather than assumed, so a future model with a different dimension is caught by a check, not silently mismatched. |
| `introduced_at` | `TEXT NOT NULL` | Timestamp this model/version was first used in this store. |

## Modified Entity: `memory_items`

| Column | Type | Notes |
|---|---|---|
| `embedding_model_id` | `TEXT` (new, nullable, FK to `embedding_models.id`) | Which frozen model produced this item's row in `memory_items_vec`, if any. `NULL` for an item that predates this feature and has not yet been backfilled — this is the explicit signal `embedding_backfill` uses to find work remaining, and the explicit signal recall uses to know whether an item has a usable vector at all (never silently treat a missing/mismatched-model vector as comparable). |

The existing `embedding_json` column (the hashed-BoW vector) is **retained, not
dropped**, for one full release cycle after this feature ships — removing it
immediately would make this change impossible to roll back if a real problem
surfaces post-deployment (Principle II, reversibility). A follow-up cleanup task
(explicitly out of scope for this feature) removes it once the new path is
proven in production.

## State Transitions

```
new memory written
  -> embedding generated via frozen ONNX model (not hashed-BoW)
  -> memory_items.embedding_model_id set
  -> memory_items_vec row inserted
  -> (existing) memory_items_fts row inserted, unchanged by this feature

pre-existing memory item (embedding_model_id IS NULL)
  -> `pamet embedding backfill` processes it in batches
  -> embedding generated via frozen ONNX model
  -> memory_items.embedding_model_id set
  -> memory_items_vec row inserted
  -> idempotent: already-backfilled items (embedding_model_id already set to
     the current model id) are skipped, so the command is safe to re-run after
     an interruption without re-processing already-done work
```

## Event-Sourcing / Replay (Principle I — mandatory, not optional)

A new event type, `embedding_backfilled`, recorded once per item processed by
the backfill (payload: `item_id`, `embedding_model_id`), following the exact
precedent `cluster_gram_model_built` set in feature 008. `rebuild.rs` gains a
replay handler for this event type that re-inserts the `memory_items_vec` row
and sets `memory_items.embedding_model_id` verbatim from the event payload — a
pure re-insert during replay, consistent with how `insert_from_replay()` already
works for gram models, **not** a re-run of the ONNX model during replay (replay
must be fast and must not require the model file to be present at all — the
vector itself, not just the fact that embedding happened, must be in the event
payload or losslessly reconstructable from it).

Live writes reuse the existing `write_decision` event's payload (already
carries the full memory item state) rather than inventing a second redundant
event per live write — only the one-time backfill needs its own event type,
since it is the only path that processes already-existing items outside the
normal write flow.
