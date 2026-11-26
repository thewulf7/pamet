# Feature Specification: Memory Pipeline Quality

**Feature Branch**: `006-memory-pipeline-quality`

**Created**: 2026-11-25

**Status**: Draft

## Background

Five deficiencies were identified through live session observation and manual store inspection:

1. The auto-inject prefetch query fires on short/question messages (≤2 tokens) and injects irrelevant noise.
2. `sync_turn` only retains user-turn text; assistant-stated facts (e.g. "Evgenii is T4-2 EM") are never persisted.
3. The review queue (1,396 items after the Nov-25 consolidation run) has no visibility or batch tooling — items can only be resolved one at a time.
4. `pamet_outcome` is never called after the agent uses recalled items, so `use_count`/`last_used_at` signals never update and the decay/RRF feedback loop is broken.
5. The prefetch budget is hardcoded at 5 items in the plugin; this is too small for identity/context queries and untunable without a code change.

---

## User Scenarios & Testing

### User Story 1 — Useful context surfaces on identity queries (Priority: P1)

The agent is asked "who am i?" After prefetch runs, the injected memory block contains the user's name, role, and team — not unrelated fragments or literal question text.

**Why this priority**: This is the root cause of the original bug reported in this session. Identity and context queries are the most frequent use case.

**Independent Test**: Send `pamet recall --query "who am i" --budget 8` against a populated store; the top results must include identity facts (name, role, team), not question fragments or single-token items.

**Acceptance Scenarios**:

1. **Given** a store with identity facts ("Evgenii is Engineering Manager", "Travel Payments team"), **When** recall is run with query `"who am i"`, **Then** identity facts rank in the top 3 results.
2. **Given** a message text of `"who am i?"`, **When** `extract_candidate_texts` is called, **Then** zero candidates are produced (question-only message).
3. **Given** a message with ≤2 meaningful tokens (e.g. `"go"`, `"ok"`), **When** prefetch is triggered, **Then** the prefetch call is skipped entirely and the memory block is empty.

---

### User Story 2 — Assistant facts are retained (Priority: P2)

When the agent states a fact in its response ("You are T4-2 EM on Travel Payments"), that fact is available in future session recalls.

**Why this priority**: Many durable facts are stated in assistant turns, not user turns. Retaining only user content leaves a large gap.

**Independent Test**: Write an assistant turn containing a clear fact, then in a fresh session recall a query relevant to that fact — it surfaces.

**Acceptance Scenarios**:

1. **Given** an assistant reply containing "Evgenii is T4-2 Engineering Manager at SAP Concur", **When** `sync_turn` runs, **Then** the assistant content is also passed through `remember`.
2. **Given** an assistant reply that is purely a question or ≤2 tokens, **When** `sync_turn` runs, **Then** the assistant content is NOT written (same low-value gate applies).
3. **Given** both user and assistant content contain the same fact, **When** `sync_turn` runs, **Then** the second occurrence is merged/skipped by write policy (no duplicate stored).

---

### User Story 3 — Review queue has visibility and batch approval (Priority: P2)

An operator (or the agent itself) can list pending review items, see the cluster groupings, and approve or skip batches of obvious near-duplicate summaries without reviewing each item individually.

**Why this priority**: After the Nov-25 consolidation, 1,396 items sit unreviewed. The system is effectively blocked on a human review step that has no tooling.

**Independent Test**: Run `pamet review list --limit 10` against the real DB; verify it returns pending items with cluster_id, source count, and summary text. Run `pamet review approve-cluster <id>` and verify the cluster summary becomes retrievable.

**Acceptance Scenarios**:

1. **Given** pending review items, **When** `pamet review list` is run, **Then** output includes item ID, cluster summary text, number of source items, and review reason.
2. **Given** a cluster ID, **When** `pamet review approve-cluster <id>` is run, **Then** all source items in that cluster are suppressed from recall and the summary item becomes active.
3. **Given** a cluster ID, **When** `pamet review skip-cluster <id>` is run, **Then** the cluster is marked skipped and source items remain active/retrievable.
4. **Given** a `--limit` and optional `--reason` filter, **When** `pamet review list` is run, **Then** output is bounded and filterable by review reason (low_confidence / unresolved_contradiction / consolidation_cluster_review).

---

### User Story 4 — Retrieval feedback loop closes (Priority: P3)

When the agent recalls items and uses them (or discards them), `pamet_outcome` is called so `use_count` and `last_used_at` update, and the decay/RRF scoring reflects actual utility.

**Why this priority**: Without this, the retrieval signal is static — items that are always useful never rise in ranking, items that are always noise never decay faster.

**Independent Test**: Call `pamet recall`, then `pamet outcome --event-id <id> --item-id <id> --used true`, then recall again — the used item's `use_count` increments and it ranks higher on the next query.

**Acceptance Scenarios**:

1. **Given** a recalled item, **When** `pamet_outcome(used=true)` is called, **Then** the item's `use_count` increments by 1 and `last_used_at` is updated.
2. **Given** a recalled item never reported as used, **When** sufficient time passes and consolidation runs, **Then** its decay multiplier reduces its RRF score below items with recent positive outcomes.
3. **Given** an agent that recalled items in a session, **When** the session ends, **Then** `pamet_outcome` has been called at least once for items that were used in a response.

---

### User Story 5 — Prefetch budget is configurable (Priority: P3)

The number of items prefetched per turn can be set in `config.yaml` without a code change, with a sensible default.

**Why this priority**: Different use cases need different budgets. Identity-heavy sessions benefit from 8–10; task-focused sessions may need fewer.

**Independent Test**: Set `plugins.pamet.prefetch_budget: 8` in config.yaml; verify prefetch returns up to 8 items on the next session.

**Acceptance Scenarios**:

1. **Given** `plugins.pamet.prefetch_budget: 8` in config.yaml, **When** prefetch runs, **Then** up to 8 items are injected.
2. **Given** no `prefetch_budget` in config.yaml, **When** prefetch runs, **Then** the default of 5 is used.
3. **Given** `prefetch_budget: 0`, **When** prefetch runs, **Then** prefetch is disabled entirely (no memory block injected).

---

### Edge Cases

- What if `sync_turn` assistant content is very long (e.g., a multi-KB code block)? Truncate at 2000 chars, same as user content.
- What if `extract_candidate_texts` on assistant content produces only question fragments? No candidates — nothing written.
- What if `pamet review approve-cluster` is called on a cluster whose source items have already been tombstoned? It is a no-op with a clear status message.
- What if the prefetch query produces zero tokens after filtering? Skip prefetch entirely.

---

## Requirements

### Functional Requirements

- **FR-001**: `extract_candidate_texts` MUST drop all segments ending with `?` (questions are conversational, not facts). *(Already implemented in 005 — verify no regression.)*
- **FR-002**: `is_low_value` MUST reject any text with fewer than 3 tokens. *(Already implemented in 005 — verify.)*
- **FR-003**: The plugin's `prefetch()` MUST skip the recall call and return `""` when the query has fewer than 3 meaningful tokens after stripping stop words.
- **FR-004**: The plugin's `sync_turn()` MUST pass a truncated (≤2000 char) version of `assistant_content` to `remember` in addition to `user_content`, subject to the same low-value gate.
- **FR-005**: The `pamet` CLI MUST expose a `review list` subcommand that returns pending review items with ID, summary text, source count, cluster ID (if any), and review reason.
- **FR-006**: The `pamet` CLI MUST expose a `review approve-cluster <cluster_id>` subcommand that marks all source items in the cluster as suppressed and promotes the summary to active.
- **FR-007**: The `pamet` CLI MUST expose a `review skip-cluster <cluster_id>` subcommand that marks the cluster as skipped, leaving source items active.
- **FR-008**: The plugin's prefetch budget MUST be configurable via `plugins.pamet.prefetch_budget` in config.yaml, defaulting to 5.
- **FR-009**: When `prefetch_budget` is 0, prefetch MUST be disabled entirely.
- **FR-010**: The plugin's `prefetch()` stop-word list MUST include at minimum: `who, what, where, when, how, why, am, is, are, i, the, a, an, in, on, at, to, do, you`.

### Key Entities

- **Review item**: a pending decision in the review queue with fields: `id`, `cluster_id` (nullable), `source_item_ids` (list), `summary_item_id` (nullable), `review_reason` (low_confidence / unresolved_contradiction / consolidation_cluster_review), `created_at`.
- **Cluster**: a group of near-duplicate items formed by consolidation, with one generated summary item and N source items. Approve-cluster promotes the summary and suppresses sources.

---

## Success Criteria

- **SC-001**: For the query `"who am i"` against the real store, the top-3 recalled items contain the user's name, role, or team in at least 2 of 3 results.
- **SC-002**: After `sync_turn` with a substantive assistant reply, `pamet recall --query "<key fact from reply>" --budget 5` returns the fact in the top 3 results in a fresh session.
- **SC-003**: `pamet review list` returns all pending items with cluster summary text in under 2 seconds against a 25K-item store.
- **SC-004**: `pamet review approve-cluster` and `skip-cluster` complete in under 1 second per cluster regardless of cluster size.
- **SC-005**: After configuring `prefetch_budget: 8` and running a session, the memory block injected per turn contains up to 8 items (not 5).
- **SC-006**: The noise ratio in auto-injected memory (items with fewer than 3 tokens or pure question text) drops to zero for new writes after this spec ships.

---

## Assumptions

- FR-001 and FR-002 from spec 005 are already in `target/release/pamet`; this spec verifies and builds on them.
- The `review` subcommand additions are purely additive CLI surface; no schema changes are required (all data is already in `review_queue` / `cluster_members` tables from Phase 2).
- Assistant content retention (FR-004) writes through the same `remember` → write-policy pipeline; deduplication of near-identical user/assistant facts is handled by the existing merge/review rules.
- The stop-word filter for prefetch (FR-003/FR-010) is applied in the Python plugin layer, not in the Rust binary, to avoid a Rust rebuild for config changes.
- `prefetch_budget: 0` as a disable mechanism is consistent with how other budget/limit configs behave in the codebase.
