# Contract: `pamet-core` Library API

This feature's public interface is a Rust library (`pamet-core`) that the CLI (and later, an
agent integration) calls directly — no network API for this MVP (research.md: single-process,
offline-capable). This contract documents the public function signatures and their behavior;
it is not implementation code.

## `remember(store: &mut Store, interaction: Interaction) -> Vec<WriteDecision>`

- **Input**: `Interaction { source_id: String, text: String, explicit_remember: bool, occurred_at: DateTime }`
- **Behavior**: Extracts zero or more memory candidates from `interaction.text` (FR-001). For each
  candidate, applies the write-policy rules (research.md) and returns one `WriteDecision` per
  candidate (spec SC-001 — exactly one decision per candidate, always).
- **Output**: `WriteDecision { decision: Decision, rule_name: String, resulting_item_id: Option<Uuid> }`
  where `Decision = Store | Merge | Supersede | Skip | Review`.
- **Side effects**: appends one `write_decision` event per candidate (and a `review_queue` row
  when `decision == Review`) before returning (FR-009, Principle I).
- **Errors**: returns `Err` only on storage/IO failure; a candidate with nothing worth
  remembering is a normal `Skip` decision, not an error.

## `recall(store: &Store, query: Query) -> RetrievalResult`

- **Input**: `Query { text: String, budget: usize }`
- **Behavior**: Runs vector search and FTS5/BM25 keyword search in parallel, fuses with RRF
  (research.md), excludes tombstoned items and pending `review_queue` candidates, trims to
  `budget` (FR-005, FR-006, FR-007).
- **Output**: `RetrievalResult { event_id: i64, items: Vec<MemoryItem> }` — `items` is empty
  (never an error) when nothing clears the relevance bar (FR-008).
- **Side effects**: appends one `retrieval` event capturing per-method candidates, fused ranking,
  and the final bounded result (FR-010), before returning.

## `report_outcome(store: &mut Store, event_id: i64, item_id: Uuid, used: bool) -> Result<()>`

- **Behavior**: Attaches usage feedback to an existing retrieval event (FR-011) by appending an
  `outcome_update` event; never creates a disconnected new retrieval record.
- **Errors**: `Err(UnknownItemForEvent)` if `item_id` was not among the items this retrieval event
  actually returned (data-model.md validation rule).

## `resolve_review(store: &mut Store, review_id: i64, decision: Decision) -> Result<()>`

- **Behavior**: Resolves a `pending` review-queue entry to one of `Store | Merge | Supersede |
  Skip`, applying the same effects `remember` would have applied directly, and appends a
  `review_resolution` event.
- **Errors**: `Err(NotPending)` if the entry is already resolved.

## `tombstone(store: &mut Store, item_id: Uuid) -> Result<()>`

- **Behavior**: Marks a memory item removed from normal retrieval (FR-014) by appending a
  `tombstone` event; the row and its full history remain queryable by id (SC-005), never deleted.

## `export_log(store: &Store, since: Option<DateTime>) -> impl Iterator<Item = Event>`

- **Behavior**: Streams the raw `events` table (optionally since a timestamp) in replay order, for
  external training/eval tooling — this is the "zero new logging fields required" interface named
  in spec SC-006.
