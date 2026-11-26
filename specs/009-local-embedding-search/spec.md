# Feature Specification: Real Semantic Memory Search

**Feature Branch**: `009-local-embedding-search`

**Created**: 2026-11-26

**Status**: Draft

**Input**: User description: "Also use SQLite with FTS5 and sqlite-vec?" — resolved
in conversation to: replace the MVP's interim hashed bag-of-words memory
representation with a real, frozen, local semantic representation, and replace
the brute-force full-scan similarity search with a real nearest-neighbor index,
so that memory recall actually understands meaning, not just shared words, and
stays fast as the store grows. (Keyword search via FTS5/BM25 is already shipped
and out of scope here — see Assumptions.)

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Recall finds memories by meaning, not just matching words (Priority: P1)

An agent (or the person it's working for) asks a question or makes a statement
using different words than the ones originally stored, but with the same
meaning (a paraphrase, a synonym, a related term). Today, because the system's
internal representation of "meaning" is a word-hash with no actual semantic
content, these queries miss memories a human would obviously consider relevant.
This story makes recall actually understand meaning.

**Why this priority**: This is the entire point of the feature. Without this,
nothing else here has value — a faster or better-indexed version of a
meaningless representation is still meaningless.

**Independent Test**: Can be fully tested by storing a memory, then querying
with a genuine paraphrase/synonym of it (not the same words) and confirming it
is returned — measured against the project's existing eval probe set, not a
single hand-picked example.

**Acceptance Scenarios**:

1. **Given** a stored memory about a specific fact, **When** a query uses a
   synonym or paraphrase of that fact's key terms, **Then** that memory
   appears in the top results.
2. **Given** two stored memories about unrelated topics, **When** a query is
   about one of them, **Then** the unrelated memory is not returned ahead of
   the relevant one.
3. **Given** the project's existing eval probe set, **When** it is re-run
   after this feature ships, **Then** recall quality is measurably better
   than the current keyword-literal baseline and no worse on cases the
   current system already handles correctly.

---

### User Story 2 - Recall stays fast as the memory store grows (Priority: P2)

Today, finding similar memories means scanning candidate items and scoring
each one individually in application code. As the number of stored memories
grows into the tens of thousands (this is not hypothetical — the project's own
live memory store already holds that many), this way of searching does not
scale: it gets proportionally slower as more memories are stored. This story
replaces that scan with a real search index so that recall speed does not
depend on how many memories have ever been stored.

**Why this priority**: Correct-but-slow recall eventually becomes unusable,
but it is still usable today, so this is important but strictly secondary to
Story 1 actually improving result quality.

**Independent Test**: Can be fully tested by measuring recall latency against
stores of increasing real size and confirming it does not grow in proportion
to store size.

**Acceptance Scenarios**:

1. **Given** a memory store with a realistic number of items, **When** a
   query is run, **Then** the time to get results does not noticeably depend
   on how many memories are stored.
2. **Given** a newly written memory, **When** a query is run immediately
   after, **Then** that memory is immediately eligible to be found (no
   separate manual indexing step required).

---

### User Story 3 - Existing memories keep working after the upgrade (Priority: P3)

The project already has a real, populated memory store. This story ensures
nothing already stored becomes unsearchable, silently degraded, or lost when
the underlying representation changes — every existing memory gets the new
semantic representation, not just newly written ones.

**Why this priority**: Without this, Stories 1 and 2 would only apply to
memories written after the upgrade, leaving the store in a permanently split,
confusing state — but it is a one-time migration concern, not the ongoing
value the feature delivers.

**Independent Test**: Can be fully tested by taking a full copy of an existing
real memory store, running the upgrade against the copy, and confirming every
previously-stored memory is still findable afterward with no data loss.

**Acceptance Scenarios**:

1. **Given** an existing memory store populated before this feature shipped,
   **When** the upgrade is applied, **Then** every memory that was findable
   before remains findable afterward.
2. **Given** the upgrade is applied to a store of realistic real-world size,
   **When** it completes, **Then** the time it took is known and recorded
   from an actual measured run, not estimated.

---

### Edge Cases

- What happens when a memory's text is empty or contains no meaningful
  content once normalized? (Must not crash, and must not pollute search
  results with a meaningless representation.)
- What happens when the semantic-representation model itself is missing or
  fails to load at startup? (Must fail clearly and immediately, never fall
  back to silently returning degraded or empty results.)
- What happens to a memory store that mixes items represented under two
  different model versions (e.g., mid-migration, or after a future model
  upgrade)? The system must know which representation each memory has and
  never silently compare incompatible representations as if they matched.
- What happens when a query itself has no meaningful content (e.g., empty
  string, only punctuation)? Must return no results, not an error and not an
  arbitrary "everything matches" fallback.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST generate a semantic representation for every stored
  memory's content using a representation that is frozen at the time it is
  introduced — it MUST NOT be updated, fine-tuned, or trained on the project's
  own usage data as an ordinary part of operation.
- **FR-002**: System MUST generate and query semantic representations entirely
  offline — no network call is required to write a new memory or to run a
  recall query during normal operation.
- **FR-003**: System MUST store semantic representations in a form that
  supports real nearest-neighbor search, not a full scan of every stored item
  scored one at a time in application code.
- **FR-004**: System MUST return memories that are semantically related to a
  query even when the query shares few or no exact words with the stored
  memory's text.
- **FR-005**: System MUST NOT regress on queries the current system already
  answers correctly via exact or near-exact word matches.
- **FR-006**: System MUST re-derive a semantic representation for every
  already-stored memory as part of adopting this feature — existing memories
  MUST NOT require a person or agent to manually resave them to remain fully
  searchable.
- **FR-007**: The semantic representation for every memory, and whatever index
  structure holds it, MUST be fully reconstructable by replaying the system's
  event log alone, with no information lost that could not be recovered that
  way.
- **FR-008**: If the semantic-representation model cannot be loaded (missing,
  corrupt, or otherwise unusable), the system MUST fail clearly and
  immediately at startup, and MUST NOT fall back to operating in a degraded
  mode that returns empty or meaningless results without saying why.
- **FR-009**: System MUST record, for every stored semantic representation,
  which model/version produced it, so that a future model change can be
  detected and handled deliberately rather than silently mixing incompatible
  representations.
- **FR-010**: Before any one-time migration of existing memories to the new
  representation is run against the project's real, live memory store, its
  real-world running time against a realistic-size copy of that same store
  MUST be measured and recorded — not estimated or assumed.

### Key Entities

- **Memory Item**: the existing stored-memory entity; gains a semantic
  representation of its content and a record of which model/version produced
  that representation, alongside everything it already carries (text,
  timestamps, provenance).
- **Representation Model**: the frozen, local, offline function that turns a
  memory's text into its semantic representation. Tracked by identity/version
  so the system always knows which memories were represented by which version.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On the project's existing eval probe set, queries that are
  paraphrases or synonyms of a stored memory return that memory in the top
  results at a measurably higher rate than today's word-matching-only
  approach achieves on the same probes.
- **SC-002**: Query response time does not measurably increase as the store
  grows from hundreds to tens of thousands of stored memories.
- **SC-003**: 100% of memories present in a real store before the upgrade are
  still findable after the upgrade, verified against a full real-data run
  (zero silently lost items).
- **SC-004**: The real-world time to migrate an existing store of realistic
  size to the new representation is known, documented, and based on an actual
  measured run before that migration is considered safe to run against the
  live store.
- **SC-005**: A missing or corrupted representation model produces a clear,
  immediate startup failure in 100% of attempts — never a silent degraded or
  empty-result mode.

## Assumptions

- The exact choice of representation model (its size, provider, and how it is
  packaged/distributed with the project) is a technical implementation
  decision for the planning phase, not fixed by this specification — the
  specification only requires that it be frozen, local, and offline.
- Re-deriving semantic representations for every existing memory in the
  project's real, live store is in scope for this feature (not deferred to a
  later feature) — without it, the store would be left in a permanently split
  state where only new memories benefit.
- Keyword/exact-word search (already shipped, using the project's existing
  full-text search capability with relevance ranking) is unaffected by this
  feature and is out of scope — this feature is about the semantic/meaning
  half of recall, not the keyword half.
- The project's storage is SQLite-only going forward (a separate, already-
  decided change) — this feature does not need to account for any other
  database backend.
- "Realistic size," per Story 2/3's acceptance criteria, means a size on the
  order of the project's own current real memory store (tens of thousands of
  items), not an arbitrary small test fixture.
