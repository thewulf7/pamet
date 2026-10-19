# Feature Specification: Production-Grade Storage Backend

**Feature Branch**: `003-postgres-storage-backend`

**Created**: 2026-10-19

**Status**: Draft

**Input**: User description: "Migrate Pamet's storage backend off the embedded single-file store onto a production-grade, concurrent-capable backend, because the current embedded store cannot run consolidation and live remember/recall truly concurrently and has no native vector index. Keep the embedded single-file mode available as a sovereign/offline fallback. This is a prerequisite for a separate, later migration of existing data out of Hindsight into Pamet."

## Clarifications

### Session 2026-10-19

- Q: FR-004 requires migration to be "safely re-runnable without producing duplicate items" — what does safe re-run mean? → A: Migration MUST refuse to run against a target that already contains any data; a re-run requires a fresh target (no idempotent per-row merge, no destructive auto-wipe).
- Q: How is the production backend's connection info (credentials included) supplied? → A: Connection string via CLI argument/flag (same `--db` convention as the embedded store), not a separate config mechanism.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Consolidation and live traffic run truly concurrently (Priority: P1)

As the operator of a Pamet deployment, I want consolidation (the "sleep" pass) and live
remember/recall traffic to run against the store at the same time without one blocking or
measurably slowing the other, so that I can run consolidation on a real schedule without
degrading the live agent's experience.

**Why this priority**: This is the architectural reason for this feature. Phase 2 already
requires this (Constitution Principle IV) and tested it as closely as the embedded store's
single-connection design allowed; this story is what makes that test actually test
concurrency instead of fine-grained interleaving on one connection.

**Independent Test**: Run a live remember/recall workload continuously; start a
consolidation run partway through from a second, independent connection; confirm the live
workload's p95 latency is not measurably affected and the consolidation run completes
successfully.

**Acceptance Scenarios**:

1. **Given** a store on the new backend receiving continuous live remember/recall calls,
   **When** a consolidation run starts from a separate connection, **Then** the live calls'
   p95 latency does not measurably change and the consolidation run completes.
2. **Given** a consolidation run in progress, **When** a live remember or recall call is
   issued, **Then** it completes without waiting on the consolidation run's transaction.

---

### User Story 2 - Existing Pamet data moves over without loss (Priority: P1)

As the operator of an existing Pamet deployment on the embedded store, I want a reliable,
verifiable way to move all of my existing memory data onto the new backend, so that
switching backends does not mean starting over or losing history.

**Why this priority**: Equal priority to Story 1 — a backend swap that loses data or breaks
the event-sourced replay guarantee (Constitution Principle I) is not a safe migration, it is
data loss with extra steps.

**Independent Test**: Take an existing embedded-store database with a representative mix of
facts, consolidated summaries, pending reviews, and resolved reviews; migrate it; confirm
every item, decision, and review status is present and the event log exported from the new
backend is replay-equivalent to the original.

**Acceptance Scenarios**:

1. **Given** an existing embedded store containing memory items, write decisions, a pending
   review, a resolved review, and at least one consolidation run, **When** migration runs,
   **Then** the new backend contains the same counts of each, with no duplicates.
2. **Given** a successfully migrated store, **When** the event log is exported from the new
   backend and replayed into a fresh store, **Then** the replayed state matches the migrated
   state exactly (Constitution Principle I, unchanged by the backend swap).
3. **Given** a store with an interrupted (never-finished) consolidation run at the time of
   migration, **When** migration runs, **Then** that run's existing "interrupted" handling is
   preserved, not silently dropped or treated as completed.

---

### User Story 3 - Offline, dependency-free mode still works (Priority: P2)

As an operator who wants to run Pamet entirely offline with no external database service
(matching Pamet's existing sovereign-friendly posture), I want to keep running the embedded
single-file mode, so that adopting a production backend is optional, not mandatory.

**Why this priority**: Lower priority than Stories 1-2 because it is preserving existing
behavior, not adding new behavior — but it is a hard constraint (Constitution: "no feature
may require a cloud-only dependency as its sole implementation"), so it must not regress.

**Independent Test**: Run Pamet with no production backend configured and no network
access at all; confirm every existing capability (remember, recall, consolidate, eval)
still works exactly as before this feature.

**Acceptance Scenarios**:

1. **Given** no production backend configured, **When** any existing Pamet command runs,
   **Then** it behaves exactly as it did before this feature, fully offline.

---

### Edge Cases

- What happens when the configured production backend is unreachable at startup? MUST fail
  fast with a clear, actionable error — MUST NOT silently fall back to the embedded store
  (that would be a surprising, undocumented backend switch).
- What happens when migrating a store that has a pending (unresolved) review entry? The
  migrated store MUST show that same entry still pending, not auto-resolved or dropped.
- What happens when migrating a store with an interrupted consolidation run? MUST carry over
  as interrupted — migration MUST NOT silently complete or discard it.
- What happens if migration is interrupted partway through? The target MUST be left in a
  state that still refuses further migration attempts into it (per FR-004, a safe re-run
  requires a fresh target, not a second attempt at the same one) — operators recover by
  discarding the partial target and starting over against a clean one.
- What happens when the new backend's similarity search and the embedded store's similarity
  search rank a borderline pair's similarity slightly differently (different vector-index
  implementations)? This is a documented, acceptable tolerance, not a correctness defect —
  exact-threshold rule behavior (merge/cluster/contradiction bands) MUST still be verified
  against the new backend's own eval-harness run, not assumed identical to the embedded
  store's.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST support a production-grade, concurrent-capable storage backend as
  an alternative to the existing embedded single-file store, selectable at deployment time.
- **FR-002**: The event-sourced architecture MUST be preserved unchanged on the new backend
  (Constitution Principle I) — the event log remains the sole, append-only source of truth,
  and all derived state remains rebuildable by replaying it alone, exactly as on the
  embedded store.
- **FR-003**: Consolidation and live remember/recall MUST be able to run concurrently
  against the new backend from independent connections without one blocking or measurably
  slowing the other (Constitution Principle IV) — the embedded store's single-connection
  design made this un-testable; the new backend MUST make it genuinely true and testable.
- **FR-004**: System MUST provide a one-time migration capability that copies a complete
  existing embedded store (every event, every derived row across all tables) onto the new
  backend without data loss and without duplication. Migration MUST refuse to run against a
  target that already contains any data (no idempotent per-row merge, no destructive
  auto-wipe) — a safe re-run means pointing at a fresh target, not re-running against the
  same one.
- **FR-005**: System MUST support a native similarity-search index for memory embeddings on
  the new backend, replacing the embedded store's full-table brute-force scan, without
  changing the documented rule-based thresholds (merge/cluster/contradiction bands) that
  depend on similarity scores.
- **FR-006**: System MUST continue to support a fully self-hosted, offline, single-file
  embedded mode as a selectable backend, with no capability gap relative to today —
  adopting a production backend MUST remain optional.
- **FR-007**: The storage backend MUST be a configuration choice, not a code change; an
  unreachable configured production backend MUST fail fast at startup with a clear error,
  never silently fall back to a different backend. The backend (file path or connection
  string, credentials included) is supplied via the same CLI argument/flag convention
  already used for the embedded store (e.g. `--db`), not a second, separate config
  mechanism — accepting that credentials may then appear in shell history or process
  listings is a disclosed tradeoff (see Assumptions), not an oversight.
- **FR-008**: Migration MUST be independently verifiable: exporting the event log from the
  migrated (new-backend) store and replaying it into a fresh store MUST produce state
  equivalent to the migrated store, exactly as Phase 1/2's existing replay guarantee already
  requires on the embedded store.
- **FR-009**: Migration MUST carry over review-queue status (pending and resolved),
  consolidation-run status (including interrupted runs), and baseline-report history with no
  loss or silent reinterpretation of state.
- **FR-010**: Existing Phase 1 and Phase 2 functional behavior (write policy, retrieval
  ranking, decay, consolidation, eval harness, CLI commands) MUST be unchanged from the
  operator's and the agent's point of view — this feature changes where data lives, not what
  Pamet does.

### Key Entities *(include if feature involves data)*

- **Backend Configuration**: the operator's choice of which storage backend a given Pamet
  deployment targets (embedded single-file, or the new production backend) and how to reach
  it — selected at startup, not per-call.
- **Migration Run**: a one-time operation that reads a complete source store and writes an
  equivalent target store — tracked well enough to detect and safely resume/re-run a partial
  or interrupted attempt.

(No new memory-domain entities — `memory_items`, `events`, `write_decisions`,
`review_queue`, `cluster_members`, `consolidation_runs`, `eval_results`, and
`baseline_reports` all carry over unchanged in shape; only their physical storage changes.)

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A consolidation run executing concurrently with continuous live
  remember/recall traffic on the new backend adds no measurable latency to that live
  traffic (p95 latency with and without a concurrent consolidation run differs by less than
  5%) — verified with genuine concurrent connections, not single-connection interleaving.
- **SC-002**: Migrating an existing store of up to 10,000 memory items completes in under 5
  minutes, with the item count, event count, and review-queue state identical before and
  after.
- **SC-003**: A similarity search against a 10,000-item store on the new backend returns in
  under 100ms per query.
- **SC-004**: An operator with no production backend configured can use every existing
  Pamet capability fully offline, with zero network calls, exactly as before this feature.
- **SC-005**: Running the existing eval harness against the new backend produces scores
  within the same pass/fail outcome per probe as the embedded store (any difference
  attributable only to the documented similarity-index tolerance from the Edge Cases
  section, never to a changed rule or a data-loss bug).

## Assumptions

- The production backend's exact technology and client library are an implementation
  decision for `/speckit-plan`, not this spec — but per the Pamet constitution's explicit
  "SQLite or Postgres" allowance and the operator's existing operational familiarity with
  Postgres (already running it for a comparable memory system), PostgreSQL with a native
  vector-index extension is the expected choice; this spec's requirements are written to be
  satisfied by that choice without naming it as a hard requirement.
- Migrating existing *Hindsight* data into Pamet is explicitly **out of scope** for this
  feature — that is a separate, later feature that depends on this one being done first (no
  point migrating external data into a backend about to be replaced).
- No multi-tenant or sharding requirements are introduced — this remains a single logical
  store per deployment, consistent with Phase 1 and Phase 2's scope.
- The embedded single-file mode is retained indefinitely as a first-class option, not
  deprecated or scheduled for removal.
- Existing Phase 1/Phase 2 schema shapes, event types, and rule-based thresholds carry
  forward unchanged; this feature is additive/substitutive at the storage layer only.
- Connection credentials for the production backend are accepted via the same CLI
  argument/flag already used for the embedded store's file path (FR-007) — this means
  credentials can appear in shell history and process listings on the machine running
  Pamet. This is a disclosed, accepted tradeoff for this feature (consistent with Pamet's
  single-operator, self-hosted posture), not a gap; a secrets-manager/env-var-only
  integration remains a candidate for a later feature if a multi-operator deployment model
  is ever adopted.
