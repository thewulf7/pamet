# Quickstart: Validate Memory Store MVP

Validates the three user stories end-to-end via the CLI contract (contracts/cli-contract.md).
Does not duplicate schema/contract detail — see data-model.md and contracts/ for the full shapes.

## Prerequisites

- Rust stable toolchain installed (edition 2021, MSRV 1.75+).
- A clean working directory (the commands below create `./pamet.db` from scratch).
- No model download or network call is required: the embedding is a local, deterministic,
  frozen hashed bag-of-words vector (see `src/services/embedding.rs` and the "Deviation" note in
  research.md / plan.md Completion Report) — not the `ort`/ONNX path research.md originally named.

## Setup

```sh
cargo build --release
rm -f ./pamet.db   # start clean for the scenarios below
```

## Scenario 1 — User Story 1: write decisions (store / merge / supersede / skip / review)

```sh
./target/release/pamet --db ./pamet.db remember --text "The project is called Pamet."
# observed: store rule=novel_store item=<uuid-A>

./target/release/pamet --db ./pamet.db remember --text "The project is called Pamet (same as before)."
# observed: review rule=ambiguous_similarity_review item=-
# (the added phrase lowers cosine similarity into the ambiguous band rather than
#  the merge band with this MVP's hashed bag-of-words embedder — a confidently
#  identical restatement, e.g. the exact same string, merges instead; see
#  tests/contract/test_remember.rs and tests/integration/test_write_policy.rs)

./target/release/pamet --db ./pamet.db remember --text "Actually the project is now called Kafka-for-memory."
# observed: review rule=unresolved_contradiction_review item=-
# (contradiction marker "actually" detected, but similarity to item A is not low
#  enough to confidently auto-supersede, so it is held for review rather than
#  silently applied — this is FR-004 working as intended, not a failure)

./target/release/pamet --db ./pamet.db remember --text "lol ok"
# observed: skip rule=low_value_skip item=-
```

**Observed outcome** (verified 2026-10-16 against the release binary): item A is created and
stored; the two borderline candidates are routed to the review queue rather than guessed at
(`pamet review list` shows both as `pending`); the low-value message is skipped. Every decision
is traceable to a named rule in `write_decisions.rule_name`, satisfying spec SC-001.

## Scenario 2 — User Story 2: hybrid retrieval with a result budget

```sh
./target/release/pamet --db ./pamet.db recall --query "what is the project called" --budget 2
# observed: returns item A ("The project is called Pamet.") — fused vector+keyword ranking

./target/release/pamet --db ./pamet.db recall --query "completely unrelated query about weather" --budget 2
# observed: literal line "no relevant memory found"
```

**Observed outcome**: the first query's `retrieval_events` row has non-empty
`vector_candidates_json` (cosine similarity ≈0.45–0.8 depending on exact phrasing) and
`keyword_candidates_json`, fused into a non-empty `fused_ranking_json`; the second query's row
has empty `vector_candidates_json`/`keyword_candidates_json`/`returned_item_ids_json` (both
per-method searches apply a minimum-relevance floor before ranking — FR-008), and the CLI exit
code is `0` for both calls (an empty result is not an error).

## Scenario 3 — User Story 3: outcome logging closes the loop

```sh
# Capture the event-id printed to stderr by `recall` above (e.g. event_id=2), then:
./target/release/pamet --db ./pamet.db outcome --event-id 2 --item-id <uuid-A> --used true
# observed: ok

./target/release/pamet --db ./pamet.db export-log | tail -2
# observed: an `outcome_update` event present, with payload referencing the same
# retrieval_event_id and item_id passed above
```

**Observed outcome**: `outcome_records` gains a row for item A on retrieval event 2 (not a new,
disconnected retrieval event — FR-011, verified by `tests/contract/test_outcome.rs` and
`tests/integration/test_outcome_logging.rs`); `export-log` shows every event from Scenarios 1-3
with no gaps (verified by `full_day_of_mixed_writes_and_retrievals_has_no_gaps_in_the_log`),
satisfying spec SC-006.

## Pass/fail

**Last validated**: 2026-10-16, against `cargo build --release` + the manual command sequence
above, plus the full `cargo test` suite (17 tests: 4 lib unit, 5 contract, 5 integration, 3 unit
— all passing). This feature is considered validated: every step above is reproducible from a
clean `./pamet.db` using only the CLI contract, and the automated test suite covers the same
three scenarios without manual DB edits.
