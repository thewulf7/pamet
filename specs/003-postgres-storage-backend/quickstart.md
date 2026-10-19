# Quickstart: Production-Grade Storage Backend

Validation scenarios for this feature, numbered to match `spec.md`'s User Stories. Run against
a release build with `--features postgres-backend` and a reachable local Postgres instance with
the `pgvector` extension available.

## Prerequisites

```bash
cargo build --release --features postgres-backend
# A local Postgres with pgvector, e.g.:
docker run -d --name pamet-pg -e POSTGRES_PASSWORD=pamet -p 5432:5432 pgvector/pgvector:pg16
export PAMET_TEST_POSTGRES_URL="postgres://postgres:pamet@localhost:5432/pamet_test"
```

## Scenario 1 (US1 / SC-001): consolidation doesn't slow live traffic

```bash
BIN=./target/release/pamet
$BIN --db "$PAMET_TEST_POSTGRES_URL" remember --text "seed fact" --explicit-remember
# Run the e2e concurrency test directly rather than hand-driving two terminals:
cargo test --release --features postgres-backend --test e2e_postgres_concurrency
```

Expected: test passes, asserting p95 live-recall latency differs by under 5% with vs. without
a concurrently-running `consolidate()` on a second connection (see `research.md` Decision 6).

## Scenario 2 (US2 / SC-002): existing data migrates without loss

```bash
# Build up a representative existing SQLite store first.
$BIN --db /tmp/pamet-source.db remember --text "fact one" --explicit-remember
$BIN --db /tmp/pamet-source.db remember --text "fact two, a near-duplicate of fact one" --explicit-remember
$BIN --db /tmp/pamet-source.db consolidate
$BIN --db /tmp/pamet-source.db migrate --to "$PAMET_TEST_POSTGRES_URL"
```

Expected: `migration_complete items=<n> events=<n> reviews_pending=<n> reviews_resolved=<n>`
printed; counts match `pamet --db /tmp/pamet-source.db export-log | wc -l` and the equivalent
`review list` counts on the source. Re-running `pamet --db "$PAMET_TEST_POSTGRES_URL" recall ...`
returns the same items the source store would.

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

## Scenario 4 (SC-003): vector search stays fast at scale

```bash
cargo test --release --features postgres-backend --test integration_postgres_scale
```

Expected: a similarity search against a 10,000-item Postgres-backed store returns in under
100ms per query (asserted in the test, not read off a stopwatch).

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
