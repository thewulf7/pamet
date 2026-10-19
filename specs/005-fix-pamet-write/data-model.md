# Data Model: Write-Policy Near-Duplicate Pre-Filter

No new persisted entities, tables, or event types — this feature changes an in-memory
control-flow detail of `write_policy::evaluate()`, not the data model. Documented here
is the *changed flow*, since `data-model.md` is the place prior features used to make
internal data shape changes explicit.

## Changed flow: candidate-set assembly

**Before** (current `write_policy::evaluate()`):

```
1. embed(candidate.content)
2. active_items = active_items_where(store, "")   // ALL active items, full table scan
3. for item in active_items: cosine_similarity(embedding, item.embedding)  // O(n) per write
4. best_match = max by similarity
```

**After**:

```
1. embed(candidate.content)
2. candidate_ids = retrieval_keyword::search(store, candidate.content, limit=200)
                                                    // existing FTS5/tsvector index,
                                                    // excludes tombstoned rows already
3. if candidate_ids.is_empty():
     -> Decision::Store ("novel_store"), no cosine comparison run at all (Decision 3)
4. else:
     active_items = active_items_where(store, "id IN (...)")   // bounded by candidate_ids
     for item in active_items: cosine_similarity(embedding, item.embedding)  // O(≤200) per write
     best_match = max by similarity
```

Everything downstream of `best_match` (the merge/supersede/review/store decision rules
in `write_policy::evaluate()`) is **unchanged** — this is why FR-004/SC-003 (zero
decision-outcome regression) is achievable: the rules operate on `best_match` and
`active_items` exactly as before, just over a narrower, bounded set.

## Entities touched (no schema change)

- **`MemoryItem`** (`src/models/memory_item.rs`): unchanged shape; now loaded in a
  narrower batch (`id IN (...)` predicate via `active_items_where`'s existing
  `extra_where` parameter — no new function signature needed).
- **`Candidate`** (`write_policy.rs`): unchanged.
- **`RankedCandidate`** (`src/models/retrieval.rs`, used by
  `retrieval_keyword::search()`): reused as-is to get the candidate-id list; its `score`
  field (BM25/`ts_rank`) is not used by the pre-filter beyond ordering the top-N cutoff
  — the actual merge/contradiction decision still runs on exact cosine similarity, not
  the keyword score.

## State transitions

None — this feature does not add, remove, or change any state machine (write-decision
outcomes, review-queue status, consolidation-run status all remain exactly as before).
