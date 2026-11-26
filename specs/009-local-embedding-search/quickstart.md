# Quickstart: Real Semantic Memory Search

Validation scenarios to run for real once implementation lands — matching
feature 008's discipline of actually running every scenario against a real
binary/real data, not describing them hypothetically.

## Prerequisites

```sh
cargo build --release
./target/release/pamet --db /tmp/pamet_009_quickstart.db embedding fetch-model
```

## Scenario 1 — Semantic match beats pure keyword overlap (US1/SC-001)

```sh
DB=/tmp/pamet_009_quickstart.db
rm -f "$DB"
pamet --db "$DB" embedding fetch-model
pamet --db "$DB" remember --text "My manager approved the budget increase" --explicit-remember
pamet --db "$DB" recall --query "did my boss sign off on more money"
```

**Expected**: the stored memory appears in results despite sharing almost no
exact words with the query ("manager"/"boss", "approved"/"sign off",
"budget increase"/"more money") — something the old hashed-BoW representation
structurally cannot do.

## Scenario 2 — Recall latency does not grow with store size (US2/SC-002)

```sh
# Against a copy of the project's real ~25,000-item store (never the live file).
cp ~/.hermes/pamet/memory.db /tmp/pamet_009_scale_test.db
pamet --db /tmp/pamet_009_scale_test.db embedding fetch-model
time pamet --db /tmp/pamet_009_scale_test.db embedding backfill
time pamet --db /tmp/pamet_009_scale_test.db recall --query "what model does pamet use"
```

**Expected**: the `recall` call's time is reported and compared against the
same query run on a small (hundreds-of-items) store — it must not scale
proportionally with item count the way the old brute-force scan did.

## Scenario 3 — Existing memories survive the upgrade with zero loss (US3/SC-003)

```sh
DB=/tmp/pamet_009_migration_check.db
cp ~/.hermes/pamet/memory.db "$DB"
BEFORE=$(sqlite3 "$DB" "SELECT count(*) FROM memory_items WHERE tombstoned_at IS NULL;")
pamet --db "$DB" embedding fetch-model
pamet --db "$DB" embedding backfill
AFTER_NULL=$(sqlite3 "$DB" "SELECT count(*) FROM memory_items WHERE tombstoned_at IS NULL AND embedding_model_id IS NULL;")
echo "before=$BEFORE still_unbackfilled=$AFTER_NULL"
```

**Expected**: `still_unbackfilled=0` — every active item got a real embedding,
confirmed by direct count, not assumed from the command's own summary line.

## Scenario 4 — Missing model fails clearly, never silently (SC-005)

```sh
DB=/tmp/pamet_009_no_model_check.db
rm -f "$DB" ~/.cache/pamet/models/*  # simulated fresh install, no model fetched
pamet --db "$DB" remember --text "this should fail clearly" --explicit-remember
echo "exit code: $?"
```

**Expected**: non-zero exit, a specific stderr message pointing at
`pamet embedding fetch-model` — never a silent fallback to a degraded or empty
vector.

## Scenario 5 — Replay reproduces vector state byte-for-byte (Principle I)

```sh
DB=/tmp/pamet_009_replay_check.db
rm -f "$DB"
pamet --db "$DB" embedding fetch-model
pamet --db "$DB" remember --text "replay check memory" --explicit-remember
# (rebuild path exercised the same way feature 008's test_gram_replay.rs did —
#  via the library's rebuild::rebuild_from_events(), not a separate CLI flag)
```

**Expected**: the rebuilt store's `memory_items_vec` row for this item is
byte-for-byte identical to the source — same pattern, same verification method
`tests/integration/test_gram_replay.rs` already established for feature 008's
gram models.
