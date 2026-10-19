# Quickstart: Production-Grade Storage Backend

Validation scenarios for this feature, numbered to match `spec.md`'s User Stories. Run against
a release build with `--features postgres-backend` and a reachable local Postgres instance with
the `pgvector` extension available.

**Status**: all 5 scenarios below were run for real against a local
`pgvector/pgvector:pg16` container on 2026-10-19. Actual observed output
replaces the originally-speculated output inline.

## Prerequisites

```bash
cargo build --release --features postgres-backend
# A local Postgres with pgvector, e.g.:
docker run -d --name pamet-pg -e POSTGRES_PASSWORD=pamet -p 5433:5432 pgvector/pgvector:pg16
docker exec pamet-pg psql -U postgres -c "CREATE EXTENSION IF NOT EXISTS vector;"
export PAMET_TEST_POSTGRES_URL="postgres://postgres:***@localhost:5433/postgres"
```

## Scenario 1 (US1 / SC-001): consolidation doesn't slow live traffic

```bash
BIN=./target/release/pamet
$BIN --db "$PAMET_TEST_POSTGRES_URL" remember --text "seed fact" --explicit-remember
# Run the e2e concurrency test directly rather than hand-driving two terminals:
cargo test --release --features postgres-backend --test e2e_postgres_concurrency -- --nocapture
```

Expected: test passes, confirming a live remember/recall workload and a `consolidate()` run on
a second, independent connection both make progress without deadlocking or blocking each other.

**Actual observed output:**
```
store rule=novel_store item=799f0335-480b-41ea-935c-4a3d292015b9
live path: 20 remember+recall pairs in 154.434292ms; consolidate: 3 items processed in 11.82025ms
test concurrent_remember_and_consolidate_both_make_progress ... ok
```

## Scenario 2 (US2 / SC-002): existing data migrates without loss

```bash
# Build up a representative existing SQLite store first.
$BIN --db /tmp/pamet-source.db remember --text "fact one" --explicit-remember
$BIN --db /tmp/pamet-source.db remember --text "fact two, a near-duplicate of fact one" --explicit-remember
$BIN --db /tmp/pamet-source.db consolidate
$BIN migrate --from /tmp/pamet-source.db --to "$PAMET_TEST_POSTGRES_URL"
```

Note: `migrate` takes `--from`/`--to` as its own flags, not the global `--db` flag — confirmed
by actually running it (the originally-drafted `--db <source> migrate --to <target>` form does
not parse: `error: the following required arguments were not provided: --from <FROM>`).
Corrected above.

Expected: `migration_complete items=<n> events=<n> reviews_pending=<n> reviews_resolved=<n>`
printed; counts match `pamet --db /tmp/pamet-source.db export-log | wc -l` and the equivalent
`review list` counts on the source. Re-running `pamet --db "$PAMET_TEST_POSTGRES_URL" recall ...`
returns the same items the source store would.

**Actual observed output:**
```
$ $BIN migrate --from /tmp/pamet-source.db --to "$PAMET_TEST_POSTGRES_URL"
migration_complete items=3 events=6 reviews_pending=1 reviews_resolved=0

$ $BIN --db /tmp/pamet-source.db export-log | wc -l
6

$ $BIN --db "$PAMET_TEST_POSTGRES_URL" recall --query "fact one"
532e777b-6287-4384-b1e9-39d227725c35 [fact] fact two, a near-duplicate of fact one
25f8e246-47a1-4b39-92c3-e44c13a19f02 [fact] fact one
event_id=7

$ $BIN --db /tmp/pamet-source.db review list
1 wd=3 reason=consolidation_cluster_promotion status=pending
```
(`items=3` because consolidation's cluster-summary promotion added a 3rd item beyond the two
`remember()` calls; `reviews_pending=1` is that same summary awaiting review — both carried over
from source to target exactly.)

## Scenario 3 (US3 / SC-004): offline mode is unaffected

```bash
# No PAMET_TEST_POSTGRES_URL, no network reachability assumed at all.
$BIN --db /tmp/pamet-offline.db remember --text "fully offline fact" --explicit-remember
$BIN --db /tmp/pamet-offline.db recall --query "offline"
$BIN --db /tmp/pamet-offline.db consolidate
$BIN --db /tmp/pamet-offline.db eval --fixtures tests/fixtures/eval_probes_v1.json
```

Expected: every command behaves exactly as it does on `main` today — this feature adds a
capability, it does not change the embedded path at all.

**Actual observed output:**
```
store rule=novel_store item=7c633f9e-1be7-4e33-967c-e4662d96b680
7c633f9e-1be7-4e33-967c-e4662d96b680 [fact] fully offline fact
event_id=2
consolidation_run=fd4d6212-1131-44c2-a883-3b87b0782e60 items=1 clusters=0 contradictions=0 decayed=0
report written to ./pamet-eval-report-20261019130426.json
```

## Scenario 4 (SC-003): vector search stays responsive at scale

```bash
cargo test --release --features postgres-backend --test integration_postgres_scale -- --nocapture
```

Expected: a similarity search against a 10,000-item Postgres-backed store returns in under
**200ms** per query (SC-003, relaxed from an original 100ms target — see spec.md
Clarifications: pgvector cannot index a 4096-dimension column at all, a verified hard vendor
limit at any implementation quality, not an implementation gap).

**Actual observed output** (10,000 items, bulk-seeded via direct SQL):
```
seeded 10000 items via bulk SQL in 4.826869792s
query "bulk scale-test fact about topic 5": 10 results in 175.176042ms
query "bulk scale-test fact about topic 42": 10 results in 138.769584ms
query "bulk scale-test fact about topic 90": 10 results in 127.130333ms
test similarity_search_against_10k_items_stays_under_200ms_sc003 ... ok
```

## Scenario 5 (SC-005): eval harness agrees across backends

```bash
$BIN --db "$PAMET_TEST_POSTGRES_URL" eval --fixtures tests/fixtures/eval_probes_v1.json --report-out /tmp/pg-report.json
$BIN --db /tmp/pamet-offline.db eval --fixtures tests/fixtures/eval_probes_v1.json --report-out /tmp/sqlite-report.json
diff <(jq .pamet /tmp/pg-report.json) <(jq .pamet /tmp/sqlite-report.json)
```

Expected: identical per-category pass/fail outcomes (per-probe latency numbers will legitimately
differ; the Edge Cases section's documented similarity-index tolerance is the only acceptable
source of a scoring difference, and the probe set is small/synthetic enough that none is
expected in practice).

**Actual observed output** — pass/fail outcomes identical on both backends, only
`p95_latency_ms` legitimately differs (Postgres vs. SQLite):
```
forgetting_abstention: passed=2/3 both | latency 0.126ms vs 0.117ms
general_precision:     passed=3/3 both | latency 0.262ms vs 0.250ms
new_fact_recall:       passed=3/3 both | latency 0.183ms vs 0.190ms
update_supersession:   passed=3/3 both | latency 0.198ms vs 0.199ms
```
