# Data Model: Memory Pipeline Quality

No schema changes are required. All data already exists. This document maps the
existing tables to the new CLI operations and plugin behaviours.

---

## Existing Tables Used

### `memory_items`
| Column | Type | Relevant to |
|--------|------|-------------|
| `id` | TEXT PK | All operations |
| `content` | TEXT | Review list (summary text) |
| `use_count` | INTEGER | `pamet_outcome` feedback loop |
| `last_used_at` | TEXT (ISO8601) | Decay multiplier input |
| `tombstoned` | INTEGER | Approve-cluster suppression of originals (via SC-005) |

### `write_decisions`
| Column | Type | Relevant to |
|--------|------|-------------|
| `id` | INTEGER PK | `review_queue.write_decision_id` FK |
| `resulting_item_id` | TEXT | Resolve the summary item for a cluster |
| `decision` | TEXT | "review" for items in queue |

### `review_queue`
| Column | Type | Relevant to |
|--------|------|-------------|
| `id` | INTEGER PK | `review resolve` target |
| `write_decision_id` | INTEGER FK | Link to `write_decisions` |
| `reason` | TEXT | Filter by reason (low_confidence / unresolved_contradiction / consolidation_cluster_review) |
| `status` | TEXT | "pending" / "resolved_store" / "resolved_skip" |
| `created_at` | TEXT | Ordering |
| `resolved_at` | TEXT | Set on resolve |

### `cluster_members`
| Column | Type | Relevant to |
|--------|------|-------------|
| `cluster_id` | TEXT | Group key for `approve-cluster` / `skip-cluster` |
| `summary_item_id` | TEXT | The consolidated item to promote |
| `source_item_id` | TEXT | The originals to suppress (via SC-005) |
| `consolidation_run_id` | TEXT | Traceability |

### `outcome_records`
| Column | Type | Relevant to |
|--------|------|-------------|
| `retrieval_event_id` | INTEGER FK | Links to the `recall` event |
| `item_id` | TEXT | Which item was (not) used |
| `used` | INTEGER (bool) | Positive/negative signal |
| `reported_at` | TEXT | Timestamp |

---

## Enriched Review List Query

The current `review list` output omits summary text and source count. The enriched
query joins three tables:

```sql
SELECT
    rq.id          AS review_id,
    rq.reason,
    rq.status,
    rq.created_at,
    wd.resulting_item_id AS summary_item_id,
    mi.content     AS summary_text,
    cm.source_count
FROM review_queue rq
JOIN write_decisions wd ON wd.id = rq.write_decision_id
LEFT JOIN memory_items mi ON mi.id = wd.resulting_item_id
LEFT JOIN (
    SELECT summary_item_id, COUNT(*) AS source_count
    FROM cluster_members
    GROUP BY summary_item_id
) cm ON cm.summary_item_id = wd.resulting_item_id
WHERE rq.status = 'pending'
ORDER BY rq.id ASC
LIMIT ?
```

No new columns or indexes are needed. The `LEFT JOIN` on `cluster_members` correctly
returns `NULL` source count for non-cluster review items (low-confidence single
items have no cluster row).

---

## Approve-Cluster Resolution Path

```
approve-cluster <cluster_id>
  → SELECT id FROM review_queue rq
      JOIN write_decisions wd ON wd.id = rq.write_decision_id
      JOIN cluster_members cm ON cm.summary_item_id = wd.resulting_item_id
     WHERE cm.cluster_id = ? AND rq.status = 'pending'
  → resolve(store, review_id, Decision::Store)
     → UPDATE review_queue SET status = 'resolved_store'
     → emit 'review_resolution' event
  → SC-005 filter in recall() already suppresses source items
```

---

## Plugin Config Extension

The plugin reads two new optional fields from `plugins.pamet` in config.yaml:

| Field | Type | Default | Effect |
|-------|------|---------|--------|
| `prefetch_budget` | integer | 5 | Max items injected per turn; 0 = disabled |

No new DB columns. No schema migration.
