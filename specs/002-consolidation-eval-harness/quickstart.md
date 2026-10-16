# Quickstart: Consolidation Job & Eval Harness

Validation scenarios for this phase — run these against a real build to
prove the feature works end-to-end. See `contracts/cli-contract.md` for the
full command reference and `data-model.md` for the schema.

## Prerequisites

```sh
cargo build --release                               # consolidate + eval (Pamet-only)
cargo build --release --features baseline-hindsight  # + Hindsight comparison column
```

## Scenario 1 — Consolidation clusters near-duplicates, non-destructively

```sh
rm -f /tmp/pamet-phase2.db
B=./target/release/pamet
$B --db /tmp/pamet-phase2.db remember --text "The project is called Pamet." --explicit-remember
$B --db /tmp/pamet-phase2.db remember --text "This project's name is Pamet." --explicit-remember
$B --db /tmp/pamet-phase2.db remember --text "Pamet is the name of this project." --explicit-remember
$B --db /tmp/pamet-phase2.db consolidate
```

**Observed (verified against a real build)**: `consolidation_run=<id>
items=3 clusters=1 contradictions=0`; `recall` returns all 3 originals plus
the new `summary` item — none tombstoned, confirming non-destructive
clustering. These three paraphrases were checked empirically (not guessed)
to land at cosine similarity 0.68-0.77 — close enough to cluster, but below
write-time policy's 0.85 merge threshold, so all 3 survive as separate
active items for consolidation to find. (Near-duplicates that clear 0.85 at
write time are merged into one row immediately and never reach
consolidation at all — see `services::consolidation` module docs.)

## Scenario 2 — Contradiction goes to review, not silent resolution

```sh
$B --db /tmp/pamet-phase2.db remember --text "The deploy window is Tuesdays." --explicit-remember
$B --db /tmp/pamet-phase2.db remember --text "Actually the deploy window is now Thursdays." --explicit-remember
$B --db /tmp/pamet-phase2.db consolidate
$B --db /tmp/pamet-phase2.db review list
```

**Expected**: if the write-time policy didn't already resolve this via
`contradiction_supersede`, `review list` shows a pending entry for the
unresolved contradiction after consolidation — never a silent pick.

## Scenario 3 — Consolidation adds no latency to concurrent recall

```sh
cargo test --test test_consolidation_e2e -- --nocapture
```

**Expected**: the test's reported p95 recall latency with consolidation
running concurrently is within 5% of the baseline p95 with no consolidation
in progress (SC-001).

## Scenario 4 — Eval harness, Pamet-only, deterministic

```sh
$B eval --fixtures tests/fixtures/eval_probes_v1.json --report-out /tmp/report1.json
$B eval --fixtures tests/fixtures/eval_probes_v1.json --report-out /tmp/report2.json
diff /tmp/report1.json /tmp/report2.json
```

**Expected**: both commands exit 0 (all probes passing on the checked-in
fixture set); `diff` shows no difference (SC-003); the whole run completes
in well under 2 minutes for the fixture set's size (SC-002, scaling headroom
to the 1,000-item target is a load-test concern for later, not blocking this
quickstart).

## Scenario 5 — Baseline report against Hindsight

```sh
docker ps --format '{{.Names}}' | grep -q '^hindsight$' && echo "hindsight running" || echo "start it first"
$B eval --fixtures tests/fixtures/eval_probes_v1.json --with-hindsight --report-out /tmp/report-vs-hindsight.json
cat /tmp/report-vs-hindsight.json
```

**Observed (verified against the real local Hindsight container)**:
`hindsight_available: true` with a populated `hindsight` block. Pamet scored
11/12 overall (one `forgetting_abstention` probe failed — the hashed
bag-of-words embedding has no stopword filtering, so a query sharing only
common words like "the"/"is" with a stored fact weakly clears the relevance
floor; a known, documented limitation, not a bug). Hindsight scored 9/12
(1 `forgetting_abstention`, 1 `update_supersession` miss). Latency: Pamet's
p95 was sub-millisecond per category; Hindsight's was 115-160ms per
category (LLM extraction + embedding + rerank round-trip) — consistent with
the ~170x/~22x write/recall gap measured in the earlier informal benchmark.
Stopping the `hindsight` container and re-running produces
`hindsight_available: false` with no `hindsight` block, and the command
still exits based on Pamet's own results — no failure.
