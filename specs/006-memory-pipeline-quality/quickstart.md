# Quickstart: Memory Pipeline Quality

Validation guide for spec 006 — proves all five improvements work end-to-end.

## Prerequisites

- `~/Developer/pamet/target/release/pamet` built (`cargo build --release`)
- Python 3.x with `~/.hermes/plugins/pamet/__init__.py` updated
- Test DB: `export DB=/tmp/pamet_006_test.db && rm -f $DB`

---

## 1. Fix 1 — Question fragments and short text are not stored

```bash
PAMET=~/Developer/pamet/target/release/pamet

# Single-token: must skip
$PAMET --db $DB remember --text "go" --source-id test
# Expected: skip rule=low_value_skip item=-

# Bare question: must produce zero candidates (no output line)
$PAMET --db $DB remember --text "who am i?" --source-id test
# Expected: (empty — no candidates extracted)

# Two-token: must skip
$PAMET --db $DB remember --text "so what" --source-id test
# Expected: skip rule=low_value_skip item=-

# Substantive statement: must store
$PAMET --db $DB remember --text "Evgenii Utkin is Engineering Manager" --source-id test
# Expected: store rule=novel_store item=<uuid>
```

---

## 2. Fix 2 — Assistant content is retained by sync_turn

The plugin's `sync_turn` now passes both user and assistant text through `remember`.

```python
# Simulate sync_turn with assistant content
import sys; sys.path.insert(0, '/Users/I779886/.hermes/plugins/pamet')
from __init__ import PametMemoryProvider

p = PametMemoryProvider(config={
    'binary_path': '/Users/I779886/Developer/pamet/target/release/pamet',
    'db_path': '/tmp/pamet_006_test.db',
})
p.initialize('test-session')
p.sync_turn(
    user_content="tell me about myself",
    assistant_content="You are Evgenii Utkin, T4-2 Engineering Manager on Travel Payments at SAP Concur Prague.",
)
```

Then verify recall surfaces the assistant fact:

```bash
$PAMET --db $DB recall --query "Evgenii T4 Engineering Manager" --budget 5
# Expected: result containing "T4-2 Engineering Manager on Travel Payments"
```

---

## 3. Fix 3 — Review list shows summary text and source count

```bash
# Populate some near-dupes and run consolidation to create clusters
$PAMET --db $DB remember --text "Sylvie is the recruiter for requisition 456236 senior developer" --source-id test
$PAMET --db $DB remember --text "Sylvie Manasova recruiter for req 456236 senior role Prague" --source-id test
$PAMET --db $DB remember --text "Sylvie recruiter 456236 senior developer Prague job description" --source-id test
$PAMET --db $DB consolidate
# Expected: clusters=N (should be >=1 for these near-dupes if above LOW_CONFIDENCE)

# List pending reviews with enriched output
$PAMET --db $DB review list --limit 5
# Expected: each row shows review_id, reason, summary text, source_count
# Example:  12 reason=consolidation_cluster_review sources=2 "Sylvie Manasova is the recruiter..."
```

Approve a cluster:

```bash
# Get cluster_id from review list output, then:
$PAMET --db $DB review approve-cluster <cluster_id>
# Expected: ok  clusters_approved=1

# Verify: source items no longer surface in recall
$PAMET --db $DB recall --query "Sylvie recruiter 456236" --budget 10
# Expected: 1 result (the summary), not 3 raw originals
```

---

## 4. Fix 4 — pamet_outcome feedback loop

```bash
# Recall, note event_id from stderr
$PAMET --db $DB recall --query "Evgenii manager" --budget 5 2>err.txt
EVENT_ID=$(grep -o 'event_id=[0-9]*' err.txt | cut -d= -f2)
ITEM_ID=$($PAMET --db $DB recall --query "Evgenii manager" --budget 1 | awk '{print $1}')

# Report used=true
$PAMET --db $DB outcome --event-id $EVENT_ID --item-id $ITEM_ID --used true
# Expected: ok

# Recall again — item should rank first (use_count increased, last_used_at updated)
$PAMET --db $DB recall --query "Evgenii manager" --budget 5
# Expected: same item appears first
```

---

## 5. Fix 5 — Prefetch budget is configurable

Add to `~/.hermes/config.yaml`:

```yaml
plugins:
  pamet:
    prefetch_budget: 8
```

Then in a new Hermes session, verify the memory block injects up to 8 items (not 5):

```python
p = PametMemoryProvider()
p.initialize('test-session')
block = p.prefetch("Evgenii manager SAP Concur Prague team")
items = [line for line in block.splitlines() if line.startswith('- [')]
assert len(items) <= 8  # was capped at 5 before
print(f"Budget respected: {len(items)} items")
```

For budget=0 (disabled):

```yaml
plugins:
  pamet:
    prefetch_budget: 0
```

```python
block = p.prefetch("anything")
assert block == ""  # prefetch disabled
```

---

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I (Event-Sourced Spine) | ✅ | `review_resolution` events already emitted by existing `resolve()` |
| II (Provenance) | ✅ | Approve-cluster uses existing tombstone + suppression path |
| III (Rules Before Learning) | ✅ | Stop-word filter and min-token gate are deterministic rules |
| IV (Two-Speed Separation) | ✅ | Approve-cluster is an offline operation; recall path unchanged |
| V (Eval-Gated Promotion) | N/A | No embedding/model changes in this spec |
| VI (Build-Don't-Reinvent) | ✅ | All changes use existing schema, existing services |
