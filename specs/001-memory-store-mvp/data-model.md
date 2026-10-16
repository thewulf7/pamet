# Data Model: Memory Store MVP

Derived from spec.md Key Entities. All five entities are persisted in the single SQLite file
(research.md). `events` is the append-only source of truth (Constitution Principle I); the other
tables are materialized views that can be fully rebuilt by replaying `events`.

## events (append-only source of truth)

| Field | Type | Notes |
|---|---|---|
| id | INTEGER PK, autoincrement | monotonic, ordering = replay order |
| occurred_at | TEXT (ISO-8601) | wall-clock time the event was recorded |
| event_type | TEXT | `write_decision` \| `retrieval` \| `outcome_update` \| `review_resolution` \| `tombstone` |
| payload_json | TEXT (JSON) | full structured event body (see per-type shapes below) |
| schema_version | INTEGER | for safe replay across future schema changes |

**Validation**: `event_type` MUST be one of the enumerated values; `payload_json` MUST deserialize
against that type's schema or the write is rejected (fail closed, never partially applied).

## memory_items (materialized view)

| Field | Type | Notes |
|---|---|---|
| id | TEXT (UUID) PK | stable identity across merges/supersessions |
| item_type | TEXT | `episode` \| `fact` \| `preference` \| `procedure` (FR-001) |
| content | TEXT | normalized textual content |
| embedding | BLOB | vector from the local embedder (research.md); nullable until computed |
| entities | TEXT (JSON array) | extracted entity tags, optional |
| confidence | REAL (0.0-1.0) | from the write policy that created/last-touched it |
| provenance_event_id | INTEGER FK → events.id | required; NEVER null (FR-012, Principle II) |
| supersedes_id | TEXT FK → memory_items.id, nullable | set when this item replaces another (FR-013) |
| created_at | TEXT (ISO-8601) | |
| last_used_at | TEXT (ISO-8601), nullable | updated on outcome "used" events |
| use_count | INTEGER, default 0 | |
| tombstoned_at | TEXT (ISO-8601), nullable | set on removal; row is never hard-deleted (FR-014) |

**Validation rules**:
- `provenance_event_id` is required on insert — enforced at the service layer and by a NOT NULL
  column constraint.
- A tombstoned item (`tombstoned_at` set) MUST NOT be returned by retrieval (edge case: deleted
  items stop appearing) but MUST remain selectable by id for history/audit (SC-005).
- `supersedes_id`, if set, MUST point to an existing (possibly tombstoned) `memory_items.id`.

**State transitions**: `active` → `superseded` (another item's `supersedes_id` points at this one;
the superseded item itself is NOT tombstoned automatically — it stays inspectable, per spec User
Story 1 Scenario 3) → `tombstoned` (explicit removal, reversible via the event log, not via
restoring the row — Principle II).

## write_decisions (materialized view, 1:1 with a `write_decision` event)

| Field | Type | Notes |
|---|---|---|
| id | INTEGER PK | |
| event_id | INTEGER FK → events.id | the originating event |
| candidate_content | TEXT | the extracted candidate before any decision |
| decision | TEXT | `store` \| `merge` \| `supersede` \| `skip` \| `review` (FR-002, FR-004) |
| rule_name | TEXT | identifier of the specific rule that fired (FR-009) |
| rule_inputs_json | TEXT (JSON) | novelty score, contradiction score, explicit-remember flag, thresholds compared against |
| resulting_item_id | TEXT FK → memory_items.id, nullable | null when decision = `skip` or `review` (pending) |
| created_at | TEXT (ISO-8601) | |

**Validation**: exactly one `write_decisions` row per processed candidate (spec SC-001); `decision
= review` rows MUST have a corresponding `review_queue` row.

## retrieval_events (materialized view, 1:1 with a `retrieval` event)

| Field | Type | Notes |
|---|---|---|
| id | INTEGER PK | |
| event_id | INTEGER FK → events.id | |
| query_text | TEXT | |
| vector_candidates_json | TEXT (JSON) | ranked list from vector search, pre-fusion |
| keyword_candidates_json | TEXT (JSON) | ranked list from FTS5/BM25, pre-fusion |
| fused_ranking_json | TEXT (JSON) | post-RRF ranked list (FR-006, FR-010) |
| result_budget | INTEGER | configured max items/size for this call (FR-007) |
| returned_item_ids_json | TEXT (JSON array) | budget-bounded final result (empty array = "no relevant memory found", FR-008) |
| created_at | TEXT (ISO-8601) | |

**Validation**: `returned_item_ids_json` length MUST be `<= result_budget`; an empty array is a
valid, expected value (not an error) when nothing clears the relevance bar.

## outcome_records (materialized view, updates an existing `retrieval_events` row via an
`outcome_update` event — never a disconnected new record, per FR-011)

| Field | Type | Notes |
|---|---|---|
| id | INTEGER PK | |
| retrieval_event_id | INTEGER FK → retrieval_events.event_id | |
| item_id | TEXT FK → memory_items.id | which returned item this feedback is about |
| used | BOOLEAN | whether the downstream consumer actually used/cited it |
| reported_at | TEXT (ISO-8601) | |

**Validation**: `item_id` MUST be one of the ids in the referenced retrieval event's
`returned_item_ids_json`; reporting an outcome for an item never returned is rejected.

## review_queue (materialized view; entries created/resolved via `write_decision`/
`review_resolution` events)

| Field | Type | Notes |
|---|---|---|
| id | INTEGER PK | |
| write_decision_id | INTEGER FK → write_decisions.id | |
| reason | TEXT | `low_confidence` \| `unresolved_contradiction` (FR-004) |
| status | TEXT | `pending` \| `resolved_store` \| `resolved_merge` \| `resolved_supersede` \| `resolved_skip` |
| created_at | TEXT (ISO-8601) | |
| resolved_at | TEXT (ISO-8601), nullable | |

**Validation**: a `pending` entry MUST be excluded from normal retrieval candidates (edge case:
unreviewed items don't leak into retrieval) until `status` moves off `pending`.

**State transitions**: `pending` → one of the four `resolved_*` states (mirrors the normal write
decision outcomes once a human/operator resolves it); no other transitions are valid.

## Relationships summary

```text
events (append-only, source of truth)
  └─< write_decisions (1 event -> 1 decision)
         └─< review_queue (0..1, only when decision = review)
         └── resulting_item_id -> memory_items (0..1, null for skip/review)
  └─< retrieval_events (1 event -> 1 retrieval)
         └─< outcome_records (0..n, added later via outcome_update events)
memory_items
  └── supersedes_id -> memory_items (0..1, self-referential history chain)
  └── provenance_event_id -> events (1..1, required)
```
