# Feature Specification: Memory Store MVP (Write Rules + Hybrid Retrieval + Outcome Logging)

**Feature Branch**: `[001-memory-store-mvp]`

**Created**: 2026-10-16

**Status**: Draft

**Input**: User description: "Build the Phase 1 MVP of the Pamet memory layer, derived from
what-we-could-build-overview.md and the supporting design docs: a memory store that captures
items (episode, fact, preference, procedure) from interactions, makes rule-based write decisions
(store/merge/supersede/skip), serves hybrid (semantic + keyword) retrieval for queries, and logs
every write and retrieval decision with full outcome data so later learned components can be
trained from real usage."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Capture and write new memories automatically (Priority: P1)

As an agent/system using Pamet, every interaction that contains something worth remembering is
turned into a memory item and filed with a clear decision (store as new, merge into an existing
item, supersede an outdated one, or skip) — with zero manual curation required.

**Why this priority**: Nothing else in the system works without memory actually getting written.
This is the floor: without it there is no store to retrieve from and no data to log for later
learning (Constitution Principle III — rules before learning).

**Independent Test**: Feed a sequence of interactions (new fact, a restatement of an existing
fact, a contradiction of an existing fact, and small talk with nothing worth keeping) through the
system and confirm each produces exactly one write decision, each decision is attributable to a
named rule, and the resulting store state matches the expected store/merge/supersede/skip outcome
for each case.

**Acceptance Scenarios**:

1. **Given** an empty memory store, **When** an interaction contains a new fact, **Then** the
   system creates a new memory item and logs a "store" decision referencing the rule that fired.
2. **Given** a memory item already exists for a topic, **When** a new interaction restates the
   same information with no new detail, **Then** the system merges it into the existing item
   rather than creating a duplicate.
3. **Given** a memory item already exists, **When** a new interaction contradicts it, **Then**
   the system creates a new item marked as superseding the old one, and the old item remains
   retrievable in its history rather than being destroyed.
4. **Given** an interaction with nothing worth remembering, **When** it is processed, **Then**
   the system logs a "skip" decision and does not create a memory item.
5. **Given** a candidate write where confidence is low or a contradiction cannot be confidently
   resolved, **When** it is processed, **Then** the item is routed to a review queue instead of
   being silently stored or skipped.

---

### User Story 2 - Retrieve relevant memories for a query (Priority: P2)

As an agent/system, when working on a current query or task, I can ask the memory layer for
relevant memories and get back a ranked, size-bounded set of items — or an explicit "nothing
relevant" result when the store has nothing useful — so the main task can use memory without
needing to know how retrieval works internally.

**Why this priority**: Writing memory has no value until it can be read back usefully. This is
the second-most-critical piece after User Story 1, and depends on it (there must be something in
the store to retrieve).

**Independent Test**: Seed the store with a known set of items (including a known-unrelated item
and a known gap where no answer exists), run a fixed set of test queries, and confirm each query
returns the expected relevant item(s) within the configured result budget, and the query with no
answer returns an explicit "no relevant memory found" result rather than an irrelevant item.

**Acceptance Scenarios**:

1. **Given** a store containing a memory item relevant to a query, **When** the query is run,
   **Then** that item appears in the returned results, found by at least one of the two
   independent matching methods (semantic similarity and keyword match).
2. **Given** a store containing both a relevant item and several unrelated items, **When** a
   query is run, **Then** the returned, fused ranking places the relevant item ahead of the
   unrelated ones.
3. **Given** a query whose answer requires combining results from more than one matching method,
   **When** the query is run, **Then** the fused result list contains the relevant item even
   though no single method alone would have ranked it highly.
4. **Given** a result budget (maximum number/size of items returned), **When** more candidates
   exist than the budget allows, **Then** the system trims to the budget rather than returning an
   unbounded list.
5. **Given** a query with no relevant memory in the store, **When** it is run, **Then** the
   system returns an explicit "no relevant memory found" result instead of forcing irrelevant
   items into the response.

---

### User Story 3 - Log full outcome data for future learning (Priority: P3)

As the team building Pamet, every write decision and retrieval event is logged with enough
detail — including, after the fact, whether a retrieved item was actually used — that a future
learned component (reranker, write-policy model) can be trained and evaluated without having to
re-instrument the system first.

**Why this priority**: Per the project's own differentiator and Constitution Principle III
("rules before learning"), this MVP's logs are the only training data any future learned
component will have. It is lower priority than User Stories 1-2 because it rides on top of them
(nothing to log until writes and retrievals exist), but it must ship in the same phase or the
logs needed later will have gaps from day one.

**Independent Test**: Run the interaction and query sequences from User Stories 1-2, then inspect
the resulting logs and confirm every write decision and every retrieval event has a corresponding
log record containing its inputs, the decision/ranking produced, the rule/reason, and a field to
record later usage feedback — without needing any code change to add a missing field.

**Acceptance Scenarios**:

1. **Given** a write decision was made, **When** the log is inspected, **Then** it contains the
   candidate input, the decision type, and the specific rule/reason that produced it.
2. **Given** a retrieval event was served, **When** the log is inspected, **Then** it contains the
   query, the candidates each matching method returned, the fused ranking, and the items actually
   returned within budget.
3. **Given** a retrieved item was later cited or used by the downstream consumer, **When** that
   usage is reported back, **Then** the original retrieval log record is updated with the usage
   outcome rather than creating a disconnected new record.
4. **Given** a full day of mixed writes and retrievals, **When** the logs are exported, **Then**
   100% of decisions and retrievals are present and attributable, with no silent drops.

---

### Edge Cases

- What happens when the store is empty and a query is run? → Must return "no relevant memory
  found", not an error and not an unrelated item.
- How does the system handle two near-simultaneous interactions that would both create a memory
  item about the same fact? → Must not create two independent, unlinked duplicates.
- What happens when a write candidate is flagged for review but no one reviews it within a
  reasonable time? → It stays in the review queue and is excluded from normal retrieval until
  resolved, rather than defaulting to silently stored or silently discarded.
- What happens when a memory item is deleted (e.g., on a removal request)? → It is tombstoned,
  not hard-deleted; it stops appearing in retrieval but its history remains auditable.
- What happens when a query's result budget is smaller than the number of genuinely relevant
  items? → The system returns the top-ranked items up to budget and does not error.
- What happens when the same source interaction yields more than one memory candidate (e.g., two
  distinct facts in one message)? → Each candidate gets its own, independently logged write
  decision.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST extract candidate memory items (episode, fact, preference, or
  procedure) from each interaction it processes.
- **FR-002**: System MUST produce exactly one write decision per candidate — store, merge,
  supersede, or skip — using explicit, inspectable rules rather than a learned model.
- **FR-003**: Write rules MUST take into account, at minimum: similarity to existing memory
  items, whether the candidate contradicts an existing item, and an explicit "remember this"
  signal when present.
- **FR-004**: System MUST route any write candidate with low confidence or an unresolved
  contradiction to a review queue instead of applying store/merge/supersede/skip automatically.
- **FR-005**: System MUST support retrieving memories relevant to a query using at least two
  independent matching methods (at minimum: semantic similarity and keyword match).
- **FR-006**: System MUST fuse results from all matching methods into a single ranked list rather
  than returning per-method lists separately.
- **FR-007**: System MUST bound every retrieval result to a configurable maximum size/budget.
- **FR-008**: System MUST return an explicit "no relevant memory found" result when no candidate
  meets the relevance bar, rather than returning low-relevance items to fill the response.
- **FR-009**: System MUST log every write decision with its inputs, the decision produced, and
  the rule/reason that produced it.
- **FR-010**: System MUST log every retrieval event with the query, the per-method candidates,
  the fused ranking, and the items actually returned.
- **FR-011**: System MUST allow a retrieval log record to be updated after the fact with whether
  a returned item was actually used/cited, without creating a disconnected new record.
- **FR-012**: System MUST record provenance on every stored memory item, pointing back to the
  interaction(s) that produced it.
- **FR-013**: System MUST support superseding a memory item (marking it replaced while keeping it
  retrievable in history) rather than overwriting it in place.
- **FR-014**: System MUST support removing a memory item from normal retrieval via a reversible
  tombstone rather than a destructive delete.

### Key Entities

- **Memory Item**: A stored unit of memory — episode, fact, preference, or procedure — with its
  content, type, timestamps, confidence, provenance (source interaction), and an optional
  supersedes-link to the item it replaces.
- **Write Decision Record**: The outcome of processing one candidate — decision type
  (store/merge/supersede/skip), the rule/reason that fired, and a reference to the resulting
  memory item (if any).
- **Retrieval Event**: One served query — the query itself, the candidates each matching method
  returned, the fused/ranked result, and the final budget-bounded result set.
- **Outcome Record**: Usage feedback attached after the fact to a Retrieval Event — whether each
  returned item was actually used, cited, or ignored by the downstream consumer.
- **Review Queue Entry**: A write candidate held for review — the candidate, the reason it was
  flagged (low confidence / unresolved contradiction), and its resolution status.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of processed interactions produce exactly one write decision per candidate,
  and every decision is traceable to the specific rule that produced it.
- **SC-002**: On a labeled probe set of queries with a known relevant memory in the store,
  retrieval surfaces at least one relevant item within the result budget for at least 90% of
  queries.
- **SC-003**: On a labeled probe set of queries with no relevant memory in the store, retrieval
  correctly returns "no relevant memory found" at least 95% of the time.
- **SC-004**: 100% of stored memory items can be traced back to their originating interaction via
  provenance, verifiable at any later point.
- **SC-005**: 100% of tombstoned (removed) memory items remain inspectable in their history after
  removal — no removal destroys the audit trail.
- **SC-006**: A future training run for a learned reranker or write-policy model can be started
  using only the logs already produced by this MVP — zero new logging fields required to begin.

## Assumptions

- Single-scope memory (one agent/user context) is in scope for this MVP; multi-user/multi-tenant
  partitioning and access control are out of scope and will be a separate feature once the
  personal-assistant vs. team-agent decision (tracked as an open TODO in the project constitution)
  is made.
- "Interaction" means any agent/user exchange that could plausibly contain something worth
  remembering, not only messages explicitly tagged "remember this."
- All four memory types from the design docs — episodic, factual/semantic, preference, and
  procedural — are in scope for capture in this MVP; richer per-type handling (e.g., procedural
  skill extraction) can be refined in later features.
- "Hybrid retrieval" for this MVP means at least two independent matching methods (semantic +
  keyword); an additional entity/graph matching method is desirable but not required to meet
  this MVP's success criteria.
- A human review interface for the review queue is out of scope for this MVP; only the queue and
  its contents (candidate, reason, status) are required.
- No learned component (embedding projection, reranker, decision model) is part of this MVP, per
  Constitution Principle III — this feature exists specifically to generate the logs those
  components will later be trained and evaluated on.
