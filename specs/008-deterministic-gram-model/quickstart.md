# Quickstart: Deterministic Per-Cluster N-gram Model

Runnable validation scenarios proving this feature end-to-end. See
[data-model.md](./data-model.md) for field definitions and
[contracts/cli-contract.md](./contracts/cli-contract.md) for the full command
contract — not duplicated here.

## Prerequisites

```sh
cargo build --release
```

A store with at least one real consolidatable cluster — either a fresh store seeded
with near-duplicate facts, or an existing store from prior consolidation work (this
project's own `~/.hermes/pamet/memory.db`, post feature-004/005 migration, already
has real clusters from the live Hindsight corpus — see specs/004 and specs/005).

## Scenario 1 — US1: a gram model is built at consolidation time

```sh
DB=/tmp/pamet_gram_quickstart.db
rm -f "$DB"

# Seed near-duplicate facts that should cluster.
target/release/pamet --db "$DB" remember --text "The deploy pipeline failed on build step three" --explicit-remember
target/release/pamet --db "$DB" remember --text "The deploy pipeline failed again on build step three" --explicit-remember
target/release/pamet --db "$DB" remember --text "Deploy pipeline keeps failing at the build step three stage" --explicit-remember

target/release/pamet --db "$DB" consolidate
```

**Expected**: consolidation output reports a cluster formed from the three
near-duplicate items, and (new in this feature) a gram model built for it. Confirm
directly:

```sh
sqlite3 "$DB" "SELECT cluster_id, gram_order, vocab_size, training_token_count FROM cluster_gram_models;"
```

Expect exactly one row, `gram_order=3`.

## Scenario 2 — US1 idempotency: unchanged cluster, no rebuild

```sh
target/release/pamet --db "$DB" consolidate   # run again, nothing changed
sqlite3 "$DB" "SELECT count(*) FROM cluster_gram_models;"
```

**Expected**: still exactly `1` row — re-running consolidation on an unchanged
cluster does not duplicate or rebuild its gram model (FR-004).

## Scenario 3 — US2: deterministic generation

```sh
CLUSTER_ID=$(sqlite3 "$DB" "SELECT cluster_id FROM cluster_gram_models LIMIT 1;")

target/release/pamet --db "$DB" gram generate --cluster-id "$CLUSTER_ID" --rng-seed 42
target/release/pamet --db "$DB" gram generate --cluster-id "$CLUSTER_ID" --rng-seed 42
```

**Expected**: both invocations print byte-identical text (FR-006/SC-002). Every
token in the output appears in one of the three seeded source sentences — no
out-of-vocabulary token (SC-003).

## Scenario 4 — US2: missing model fails clearly

```sh
target/release/pamet --db "$DB" gram generate --cluster-id "not-a-real-cluster-id"
echo "exit code: $?"
```

**Expected**: non-zero exit, specific stderr message — never silent empty stdout
(FR-009).

## Scenario 5 — US3: inspect without generating

```sh
target/release/pamet --db "$DB" gram show --cluster-id "$CLUSTER_ID"
```

**Expected**: one line reporting `gram_order`, `vocab_size`, `training_tokens`,
`consolidation_run_id`, `created_at` — no sampling/PRNG involved (FR-007).

## Scenario 6 — US3: full replay equivalence (SC-004)

```sh
target/release/pamet --db "$DB" export-log > /tmp/pamet_gram_events.jsonl

DB_REBUILT=/tmp/pamet_gram_rebuilt.db
rm -f "$DB_REBUILT"
target/release/pamet migrate --from "$DB" --to "$DB_REBUILT"   # replays via rebuild.rs

diff \
  <(sqlite3 "$DB" "SELECT cluster_id, gram_order, vocab_size, training_token_count, table_json FROM cluster_gram_models ORDER BY cluster_id;") \
  <(sqlite3 "$DB_REBUILT" "SELECT cluster_id, gram_order, vocab_size, training_token_count, table_json FROM cluster_gram_models ORDER BY cluster_id;")
```

**Expected**: empty diff — every gram model is byte-for-byte identical after a full
rebuild from the event log alone (FR-008/SC-004), on real data, not only a synthetic
fixture — matching the standard this project already set for Postgres
replay-equivalence in feature 003.

## Scenario 7 — Edge case: cluster too small for a meaningful model

```sh
DB2=/tmp/pamet_gram_tiny.db
rm -f "$DB2"
target/release/pamet --db "$DB2" remember --text "x y" --explicit-remember
target/release/pamet --db "$DB2" remember --text "x y" --explicit-remember
target/release/pamet --db "$DB2" consolidate

sqlite3 "$DB2" "SELECT count(*) FROM cluster_gram_models;"
```

**Expected**: `0` — a cluster whose combined token count is below `gram_order` (3)
never gets a gram model, and consolidation does not error (FR-003/Edge Case).
