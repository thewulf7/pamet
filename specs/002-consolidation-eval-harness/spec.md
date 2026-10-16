# Feature Specification: Consolidation Job & Eval Harness

**Feature Branch**: `002-consolidation-eval-harness`

**Created**: 2026-10-16

**Status**: Draft

**Input**: User description: "Phase 2 per the Pamet constitution: nightly consolidation + eval harness + baselines (Mem0/Graphiti, long context)."

## Clarifications

### Session 2026-10-16

- Q: Which external reference memory system(s) should the baseline report (FR-007) compare against for this phase? → A: Hindsight only — already integrated and benchmarked; Mem0/Graphiti/long-context deferred to a later phase.
- Q: Should the eval harness probe set (FR-009) be sourced from synthetic fixtures, real usage logs, or both? → A: Synthetic/seeded fixtures now, with the fixture format mirroring the real event-log shape so swapping in real logs later (once the Hermes plugin has accumulated enough live usage) is additive, not a rewrite.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Nightly consolidation without touching the live path (Priority: P1)

As the operator of a Pamet-backed agent, I want a separate, schedulable job that
periodically turns raw episodic memory into cleaner, consolidated knowledge
(clustering near-duplicates, writing summaries, resolving contradictions, aging
out unused items) — without that job ever slowing down or blocking a live
remember/recall call happening at the same time.

**Why this priority**: This is the architectural precondition (Constitution
Principle IV, Two-Speed Memory Separation) for everything else in this phase —
the eval harness and baselines are only meaningful once there is a
consolidation path to measure.

**Independent Test**: Seed a store with near-duplicate and contradictory
memory items, run the consolidation job, and verify (a) duplicates are
clustered/summarized, (b) contradictions are flagged for review rather than
silently resolved, (c) a concurrent `recall` issued during the run shows no
added latency, and (d) every consolidation decision appears in `export-log`.

**Acceptance Scenarios**:

1. **Given** a store with 3 near-duplicate facts about the same topic, **When**
   consolidation runs, **Then** they are clustered into one consolidated item
   with provenance pointers back to all 3 originals, and the originals remain
   recoverable via the event log (no destructive delete).
2. **Given** two facts that directly contradict each other with similar
   confidence, **When** consolidation runs, **Then** the contradiction is
   written to the existing review queue instead of being auto-resolved.
3. **Given** a `remember`/`recall` call issued while consolidation is running
   in the background, **When** both complete, **Then** the live call's latency
   is not measurably different from a run with no consolidation in progress.

---

### User Story 2 - Repeatable eval harness with held-out probes (Priority: P1)

As the maintainer of Pamet, I want a single command that runs a fixed set of
held-out probes (new-fact recall, fact updates/supersession, forgetting /
abstention, and general unrelated-query precision) against the current
pipeline and reports a pass/fail and a score per category — so any future
change (including a learned component in Phase 3+) can be checked against a
real, reproducible bar instead of "it felt better."

**Why this priority**: Constitution Principle V (Eval-Gated Promotion)
requires this harness to exist and be passed before ANY learned component may
ship. Nothing in Phase 3 onward is legally* buildable under our own
constitution until this exists. (*project governance, not actual law.)

**Independent Test**: Run the harness twice against an unchanged store and
confirm identical scores; introduce a deliberate regression (e.g. corrupt the
write policy so it skips everything) and confirm the harness's score visibly
drops and the run is marked failing.

**Acceptance Scenarios**:

1. **Given** a fixed probe set and an unchanged memory store, **When** the
   harness is run twice, **Then** both runs produce identical scores
   (deterministic).
2. **Given** a probe that stores a fact and later supersedes it, **When** the
   harness runs, **Then** recall for that fact after the update returns the
   new value, not the old one, or the probe is marked failing.
3. **Given** a probe designed to test abstention (a query with no relevant
   memory in the store), **When** the harness runs, **Then** the system
   reports "no relevant memory found" rather than fabricating or returning an
   unrelated item, or the probe is marked failing.

---

### User Story 3 - Versioned baseline report vs. an external system (Priority: P2)

As the maintainer of Pamet, I want the eval harness's probe set run against at
least one external reference memory system too, with both results saved to a
versioned, inspectable report — so every future "is the learned version
better?" question has a real number to beat, not just Pamet's own history.

**Why this priority**: Lower priority than P1 because the harness itself
(User Story 2) is the hard requirement; a comparison baseline sharpens the bar
but Phase 3 work can begin once the harness exists even before every external
baseline is wired up.

**Independent Test**: Run the baseline comparison command and confirm it
produces one report file containing both systems' scores and latencies on the
identical probe set, timestamped and diffable against the previous report.

**Acceptance Scenarios**:

1. **Given** the same probe set used in User Story 2, **When** the baseline
   comparison is run, **Then** it produces a single report with per-probe
   scores and latency for Pamet and for the external reference system.
2. **Given** two baseline reports from different points in time, **When**
   they are compared, **Then** a reader can tell which probes regressed,
   improved, or stayed flat without reading any code.

---

### Edge Cases

- What happens when consolidation runs against an empty event log (no items
  yet)? MUST complete as a no-op without error.
- What happens when consolidation is interrupted mid-run (process killed)?
  MUST leave the store in a valid, immediately-usable state and be safely
  re-runnable (idempotent), since every decision it makes is itself an event.
- What happens when two contradictory facts have exactly equal confidence and
  recency? MUST go to the review queue, never auto-pick one silently.
- What happens when the external baseline system (reference memory system) is
  unreachable or not running? The Pamet-only half of the report MUST still be
  produced; the external column is marked unavailable, not silently omitted.
- What happens when the probe set itself is changed (probes added/removed)?
  Reports generated against different probe-set versions MUST be labeled with
  the probe-set version so they are never silently compared apples-to-oranges.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST provide a consolidation job, independently
  triggerable (e.g. via a CLI subcommand), that reads the event log and
  produces clustering/summarization/decay decisions without adding latency to
  or blocking concurrent `remember`/`recall` calls (Principle IV).
- **FR-002**: Every consolidation decision (cluster formed, summary created,
  contradiction flagged, item decayed) MUST be emitted as its own event in the
  same append-only event log used by the MVP (Principle I), so the
  consolidated state remains rebuildable by replay.
- **FR-003**: Consolidation MUST flag contradiction resolutions below a
  defined confidence bar for human review via the existing review queue,
  rather than resolving them automatically (Principle II).
- **FR-004**: System MUST provide a decay mechanism that lowers an item's
  retrieval priority after a configurable period of disuse; decay MUST NOT
  perform a destructive delete — the item remains reconstructable via the
  event log (consistent with the existing tombstone pattern).
- **FR-005**: System MUST provide a repeatable eval harness, runnable as a
  single command with no required network access, that executes a fixed set
  of probes covering: new-fact recall, fact update/supersession, forgetting /
  abstention, and general unrelated-query precision.
- **FR-006**: The eval harness MUST produce the same scores on repeated runs
  against an unchanged store (deterministic, reproducible) and MUST clearly
  report pass/fail per probe category, not only an aggregate score.
- **FR-007**: System MUST produce a versioned, durable report (including a
  probe-set version identifier) comparing Pamet's current pipeline against
  Hindsight (the already-integrated, already-benchmarked reference system) for
  this phase. Mem0, Graphiti, and raw long-context baselines are explicitly
  deferred to a later phase and are out of scope here.
- **FR-008**: Eval harness results and consolidation run summaries MUST be
  persisted in a durable, inspectable form so results can be diffed across
  runs over time, and MUST be the artifact any future learned-component
  promotion (Phase 3+) is checked against per Principle V.
- **FR-009**: The probe set used by the eval harness MUST be sourced from
  synthetic/seeded test fixtures authored for this project (available
  immediately, not drawn from real usage) for this phase; swapping in real
  accumulated usage/outcome logs is explicitly deferred until the Hermes
  plugin has been running live long enough to accumulate meaningful data, and
  MUST be designed for (fixture format mirrors the real event-log shape) so
  that swap is additive, not a rewrite.
- **FR-010**: Consolidation MUST be safely re-runnable after an interrupted
  run (idempotent) without corrupting or duplicating consolidated state.

### Key Entities *(include if feature involves data)*

- **ConsolidationRun**: one execution of the consolidation job — start/end
  time, number of items processed, number of clusters formed, number of
  contradictions flagged, completion status.
- **Cluster**: a group of near-duplicate memory items identified during
  consolidation, with provenance links back to every original item.
- **ConsolidatedSummary**: the new, higher-confidence knowledge item produced
  from a Cluster — carries provenance to its source items per Principle II.
- **ContradictionFlag**: a pending review-queue entry raised when two items
  make conflicting claims with insufficient confidence to auto-resolve.
- **DecayState**: per-item retrieval-priority adjustment driven by disuse,
  reversible and non-destructive.
- **EvalProbe**: one held-out test case — category (new-fact / update /
  abstention / precision), input, and expected outcome.
- **EvalResult**: the outcome of running one EvalProbe in one harness run —
  pass/fail, score, probe-set version, timestamp.
- **BaselineReport**: a versioned, timestamped collection of EvalResults for
  Pamet and (where available) one or more external reference systems, on the
  same probe set.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A consolidation run executing concurrently with live
  remember/recall traffic adds no measurable latency to that live traffic
  (p95 latency with and without a concurrent consolidation run differ by less
  than 5%).
- **SC-002**: The eval harness completes a full run and produces a report in
  under 2 minutes for a store containing up to 1,000 memory items.
- **SC-003**: Running the eval harness twice against an unchanged memory
  store produces byte-identical scores both times.
- **SC-004**: A reviewer who has never read the implementation can determine,
  from a baseline report alone, which probe categories improved, regressed, or
  stayed flat between two runs.
- **SC-005**: On a seeded test set containing intentional near-duplicate
  facts, consolidation reduces the number of near-duplicate items surfaced per
  query by at least 50% compared to the same query before consolidation ran.
- **SC-006**: Zero consolidation decisions are unrecoverable — every
  consolidated item, decay adjustment, and contradiction flag can be traced
  back to its source event(s) in `export-log`.

## Assumptions

- Consolidation and the eval harness continue to operate directly against the
  existing SQLite event log — no message bus (NATS/Kafka/Redpanda) is
  introduced in this phase; that remains a later-phase/scale concern per the
  constitution's open TODO(STACK_DECISION).
- "Nightly" describes the intended *operational* cadence, not a requirement
  that Pamet itself implement a scheduler/daemon; the consolidation job is
  exposed as an independently triggerable command, and invoking it on a
  schedule (cron, launchd, CI) is a deployment concern outside this spec.
- The review queue introduced in the MVP (Phase 1) is reused as-is for
  contradiction flags raised during consolidation; this spec does not
  introduce a second review mechanism.
- Decay is purely a retrieval-ranking adjustment, consistent with Principle
  II's non-destructive-delete requirement — it is never a path to permanent
  removal (that remains `forget`/tombstone, already shipped in Phase 1).
