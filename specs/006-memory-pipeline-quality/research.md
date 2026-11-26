# Research: Memory Pipeline Quality

No `[NEEDS CLARIFICATION]` markers were raised in spec.md. This research resolves
implementation decisions for the five improvements.

---

## Decision 1: Prefetch query filtering in Python vs Rust

**Decision**: Filter the prefetch query in the Python plugin layer (`plugin/__init__.py`),
not in the Rust binary.

**Rationale**: The stop-word list and minimum-token gate are operational tuning, not
memory semantics — they belong with the caller, not the store. Rust rebuild cycles
are costly for config-level changes. The existing `prefetch()` method already owns
the query string; inserting a filter there is a one-function change.

**Implementation**: Tokenize the query on whitespace, strip tokens in the stop-word
set (FR-010), require at least 3 remaining tokens. If fewer remain, return `""` early.

**Alternatives considered**: Rust `--min-tokens` flag on `recall` subcommand — adds
Rust rebuild for every tuning change, overkill for a config-level gate.

---

## Decision 2: Assistant content retention — truncation and dedup

**Decision**: Pass `assistant_content[:2000]` through the same `remember` path as
user content. The existing write policy (merge/review/skip) handles deduplication.

**Rationale**: The write policy's `is_low_value` and `extract_candidate_texts` already
filter noise and question fragments. The merge path (`MERGE_SIMILARITY_THRESHOLD = 0.85`)
handles the case where user and assistant both state the same fact — the second write
merges into the first. No new dedup logic is needed.

**Edge case**: Long assistant replies (code blocks, tables) truncated at 2000 chars.
The interesting facts are almost always in the first 2000 chars of a reply.

**Alternatives considered**: Extract only "summary sentences" from assistant replies —
would require an LLM call or heuristic parser, violating Constitution Principle III
(rules before learning). Keep it simple: same path, same gate.

---

## Decision 3: Review cluster commands — existing schema is sufficient

**Decision**: `review approve-cluster` and `review skip-cluster` resolve all
`review_queue` rows whose `write_decision_id` → `resulting_item_id` is a cluster
member's `summary_item_id`, using the existing `services::review::resolve()` function.

**Rationale**: The schema already has `cluster_members(cluster_id, summary_item_id,
source_item_id)` and `review_queue(write_decision_id)` → `write_decisions(resulting_item_id)`.
No schema migration is needed. The `approve-cluster` command:
1. Looks up the cluster's summary `review_queue` row via the join.
2. Calls `resolve(store, review_id, Decision::Store)` — promotes the summary.
3. The existing SC-005 suppression logic in `recall()` already hides source items once
   the summary review is resolved. No new logic needed on the recall side.

For `skip-cluster`, calls `resolve(store, review_id, Decision::Skip)` — the summary
stays tombstoned, source items remain active.

**Review list enrichment**: The current `review list` output (`{id} wd={} reason={} status={}`)
doesn't surface summary text or source count. Add a richer query that joins
`write_decisions → memory_items` to get summary text, and `cluster_members` for source count.

**Alternatives considered**: New `cluster-approve` top-level command — same logic,
just different naming. Keeping it under `review` subcommand preserves CLI ergonomics.

---

## Decision 4: `pamet_outcome` discipline — skill-level, not code-level

**Decision**: Document the obligation in `pamet-rust-development` skill (and in this
plan's quickstart) rather than auto-calling `pamet_outcome` from the plugin after
every tool use. The model must decide "was this item useful" — the plugin can't.

**Rationale**: The plugin doesn't know what the agent did with a recalled item. Auto-
calling `used=true` for every recall would reward noise equally. Auto-calling `used=false`
would punish correct recalls. The obligation lives at the agent reasoning layer, not
infrastructure.

**What changes**: Add a rule to `pamet-rust-development` skill (and system prompt addendum)
that recalled items must be followed by `pamet_outcome` before the session ends.

---

## Decision 5: Prefetch budget — config.yaml → `_DEFAULT_BUDGET` at plugin init

**Decision**: Read `plugins.pamet.prefetch_budget` in `initialize()` and store as
`self._prefetch_budget`. Use it in `prefetch()` and `queue_prefetch_all`. Default: 5.
Budget 0 = prefetch disabled (return `""` immediately).

**Rationale**: The existing `_DEFAULT_BUDGET = 5` module constant is a natural target
for a per-instance config override. The pattern already exists for `binary_path` and
`db_path` in the same plugin. Minimal code delta.

**Alternatives considered**: Env var `PAMET_PREFETCH_BUDGET` — less discoverable,
doesn't fit the existing config.yaml convention.
