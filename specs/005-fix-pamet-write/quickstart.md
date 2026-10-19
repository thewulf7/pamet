# Quickstart: Verifying the Write-Policy Pre-Filter

## Prerequisites

- Rust toolchain already set up for this repo (`cargo build` works).
- For the Postgres-dialect check only: Docker, `pgvector/pgvector:pg16` (same container
  recipe feature 003/004 already used).
- For the real-data SC-001 number only (optional, not required for the automated
  suite): a reachable local Hindsight instance with the same `hermes` bank feature 004
  used (`--features baseline-hindsight`).

## Scenario 1 — Zero regression on existing write-policy correctness (SC-003)

```bash
cargo test --release unit_write_policy_rules integration_write_policy integration_consolidation
```

**Expected outcome**: all tests pass, with identical pass/fail results to before this
feature (no new assertions were added to these files by this feature — only
`write_policy.rs`'s internals changed).

## Scenario 2 — Per-batch write cost stays flat on overlapping synthetic data (SC-002)

```bash
cargo test --release --test integration_write_policy_scale -- --nocapture
```

**Expected outcome**: the test seeds several thousand topically-overlapping items and
asserts the last batch's per-item cost is within 2x of the first batch's — printed
batch timings are visible with `--nocapture` for manual inspection.

## Scenario 3 — No measurable overhead on a small store (SC-004)

Covered by the existing small-fixture tests in Scenario 1 running unmodified and fast —
if the pre-filter added meaningful overhead to a handful of items, those tests'
wall-clock time (already tracked by CI) would visibly regress.

## Scenario 4 — Postgres dialect parity (FR-005)

```bash
docker run -d --name pamet-pg-005 -e POSTGRES_PASSWORD=pamet -e POSTGRES_DB=pamet_test \
  -p 5433:5432 pgvector/pgvector:pg16
docker exec pamet-pg-005 psql -U postgres -d pamet_test -c "CREATE EXTENSION IF NOT EXISTS vector;"
export PAMET_TEST_POSTGRES_URL="postgres://postgres:pamet@localhost:5433/pamet_test"
cargo test --release --features postgres-backend --test integration_write_policy_scale
docker stop pamet-pg-005 && docker rm pamet-pg-005
```

**Expected outcome**: same pass/fail and the same 2x-growth bound as the SQLite run in
Scenario 2 — this feature must not fix scaling for one backend and leave the other
degraded.

## Scenario 5 — The real number (SC-001, manual, not CI-automated)

```bash
rm -f /tmp/pamet_hindsight_005.db /tmp/pamet_hindsight_005_ledger.db
cargo build --release --features baseline-hindsight
./target/release/pamet --db /tmp/pamet_hindsight_005.db import-hindsight \
  --bank hermes --ledger /tmp/pamet_hindsight_005_ledger.db
```

**Expected outcome**: per-200-item progress lines printed during the run show batch
time *not* climbing the way feature 004's run did (4.4s → 101.9s by item 2,800); the
full run against the real ~15,446-item bank completes in well under 30 minutes
(SC-001). Record the actual observed numbers in spec.md's Results section, the same way
feature 004 did — do not report a projection in place of a real run.
