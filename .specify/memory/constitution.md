<!--
Sync Impact Report
Version change: [TEMPLATE] → 1.0.0 (initial ratification)
Modified principles: n/a (first filled version)
Added sections:
  - Core Principles (6): Event-Sourced Spine; Provenance & Reversibility by Construction;
    Rules Before Learning; Two-Speed Memory Separation; Eval-Gated Promotion;
    Open-Protocol, Build-Don't-Reinvent
  - Architecture & Technology Constraints
  - Development Workflow & Phasing
  - Governance
Removed sections: none (template placeholders only)
Templates requiring updates:
  ✅ .specify/templates/plan-template.md — Constitution Check gate is generic
     ("[Gates determined based on constitution file]"), no edits needed.
  ✅ .specify/templates/spec-template.md — no agent/principle-specific hardcoding found.
  ✅ .specify/templates/tasks-template.md — generic phase structure already compatible
     with Setup → Foundational → Eval-gate → Story phases implied by this constitution.
  ✅ .specify/templates/checklist-template.md — not principle-specific.
Follow-up TODOs:
  - TODO(STACK_DECISION): final choice of event bus (NATS JetStream vs Redpanda vs
    Kafka-compatible) and agent framework/base models deferred — see
    what-we-could-build-overview.md "Decisions needed". Principle VI constrains the
    choice (must stay Kafka-compatible) without picking a specific vendor.
  - TODO(DEPLOYMENT_MODEL): personal-assistant vs team/product-agent scope not yet
    decided; affects privacy/multi-user principles and may require a new principle
    once resolved.
-->

# Pamet Constitution
<!-- Pamet ("памет" = memory): an open, learnable memory layer for AI agents. -->

## Core Principles

### I. Event-Sourced Spine (NON-NEGOTIABLE)
Every memory decision and outcome (write, read, merge, supersede, forget, retrieval
score) MUST be emitted as an event before or alongside any store mutation. The
store (SQLite/Postgres + vectors + BM25 + light graph) is a derived/materialized
view of the event log, never the source of truth. Any new store, index, or model
MUST be rebuildable from replaying the event log alone.
Rationale: replay is how we rebuild with a new embedder/policy, debug sessions,
run offline counterfactual evaluation, and recover from corruption or poisoning —
none of that is possible if state lives only in a mutable store.

### II. Provenance & Reversibility by Construction
Every memory item MUST carry a pointer to the source event(s) that produced it.
Deletion MUST be implemented as tombstone + compaction/rebuild, never a destructive
in-place delete. Low-confidence writes (new facts, contradictions, supersessions)
MUST be flagged for human review before being promoted into consolidated knowledge.
Rationale: provenance and reversibility are what make an audit log, a deletion
request, or a poisoning investigation actually answerable after the fact.

### III. Rules Before Learning
A capability (retrieval depth, write decision, reranking, consolidation) MUST ship
as a deterministic rule-based or heuristic implementation, fully logged, before any
learned replacement (GBT, small LM, bandit/RL, LoRA/hypernetwork adapter) is
introduced for that same capability. A learned component MAY only replace a rule
once there is logged usage/outcome data to train and evaluate it against.
Rationale: the project's differentiator is training from real usage logs — that
data does not exist on day one, so the rule-based version is both the MVP and the
data generator for everything learned that follows.

### IV. Two-Speed Memory Separation
The fast/episodic write-and-read path and the slow consolidation ("sleep") path
MUST remain architecturally decoupled. Consolidation (clustering, summarization,
schema induction, contradiction resolution, decay) runs as a separate, schedulable
job over the event log and MUST NOT sit on, block, or add latency to the live
write/read path.
Rationale: mixing the two speeds either slows down live agent interaction or
starves consolidation of the batch time it needs to do a good job; keeping them
separate lets each be optimized and scaled independently.

### V. Eval-Gated Promotion
No change to embeddings, controller/decision model, reranker, or online adapter
MAY be promoted to the default path without passing the eval harness (LoCoMo,
LongMemEval, BEAM, plus the project's own probes for updates/abstention/forgetting)
and beating the currently deployed baseline on the metrics that matter for that
component. Online-adapting models additionally require a replay check and a
rollback path before promotion.
Rationale: "learned" only beats "rule-based" if we can prove it on held-out,
realistic probes — otherwise we are shipping regressions on faith.

### VI. Open-Protocol, Build-Don't-Reinvent
The event bus and any wire protocol MUST stay Kafka-compatible (e.g., NATS
JetStream, Redpanda, or Kafka itself) rather than inventing a bespoke bus or
protocol. Differentiation happens one layer up — agent-native memory semantics,
learning, and governance — not in reinventing streaming or transport. Build vs.
adopt-existing (e.g., Mem0, Graphiti) decisions MUST be justified in writing before
a from-scratch implementation is started.
Rationale: protocol-layer reinvention is a known failure mode (see Risks in
what-we-could-build-overview.md) and burns time that should go into the actual
differentiator.

## Architecture & Technology Constraints

- **Store**: items (episode, fact, preference, procedure) carry embeddings,
  entities, timestamps, provenance, and supersedes-links; persisted in
  SQLite or Postgres with a vector index and BM25 (graph is optional/light).
- **Event bus**: Kafka-compatible log (NATS JetStream or Redis Streams acceptable
  for early phases; Redpanda/Kafka acceptable at scale) carrying `agent.*`,
  `llm.*`, `memory.*`, `outcome.*`, `governance.*` topics with replay and
  dead-letter support.
- **Embeddings**: start from a frozen base embedder; trainable query/key
  projections (`Wq`, `Wk`) are the only learned component in Phase 3 — do not
  fine-tune the base embedder until projections are proven insufficient.
  TODO(STACK_DECISION): specific base embedder and bus vendor are not yet chosen.
- **Deployment**: MUST support a self-hosted / on-prem mode (sovereign-friendly);
  no feature may require a cloud-only dependency as its sole implementation.

## Development Workflow & Phasing

Work proceeds in the phases defined in `what-we-could-build-overview.md` and MUST
NOT skip ahead:
1. MVP (store + hybrid retrieval + rules + full outcome logging).
2. Nightly consolidation + eval harness + baselines (Mem0/Graphiti, long context).
3. Learned reranker/embedding projections; bandit for retrieval depth.
4. Learned write policy (Memory-R1 style) + small-LM judge for contradictions.
5. LoRA/hypernetwork adapter for preferences/skills; governance hardening; OSS
   release.

Every phase transition requires: (a) the previous phase's eval gate (Principle V)
passed, (b) outcome logging for the new capability wired before it goes live
(Principle I), and (c) a short written note of what baseline was beaten and by
how much. Feature flags and a rollback path are mandatory for anything in Phase 3
or later before it becomes the default.

## Governance

This constitution supersedes ad hoc practice for the Pamet project. Amendments
require: (1) a written rationale for the change, (2) an explicit MAJOR/MINOR/PATCH
version decision per the policy below, (3) updates to any dependent template or
doc found to reference the changed principle, and (4) the Sync Impact Report at
the top of this file kept current.

**Versioning policy** (semantic versioning applied to governance):
- **MAJOR**: backward-incompatible principle removal or redefinition.
- **MINOR**: a new principle or materially expanded guidance added.
- **PATCH**: clarification, wording, or non-semantic fix.

All plans and PR-level reviews MUST verify compliance with these principles;
any deviation MUST be recorded in the Complexity Tracking section of the
relevant `plan.md` with a justification. Use `AGENTS.md` and this constitution
together for runtime development guidance.

**Version**: 1.0.0 | **Ratified**: 2026-10-16 | **Last Amended**: 2026-10-16
