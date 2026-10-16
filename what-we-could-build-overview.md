---
title: What we could build - short overview
date: 2026-10-16
tags: [ai-agents, memory, project, plan]
related: "[[transformer-style-agent-memory]], [[memory-embeddings-and-decision-model]], [[online-learning-memory-model]], [[learned-memory-controllers-prior-art]]"
status: draft
---

# What we could build

## Idea in one line
An open, learnable memory layer for AI agents: a **two-speed memory** (fast episodic store + slow consolidated knowledge) controlled by a **learned decision model** and **learned embeddings**, optionally with a small **online-adapting model** for skills/preferences.

## Components
1. **Store**: items (episode, fact, preference, procedure) with embeddings, entities, timestamps, provenance, supersedes-links. SQLite/Postgres + vectors + BM25 (+ light graph).
2. **Learned embeddings**: frozen base embedder + trainable query/key projections (`Wq`, `Wk`) trained from usage logs ("memory attention").
3. **Decision model (controller)**: write (store/merge/supersede/skip), read (retrieve? heads? k?), maintain (consolidate/forget), abstain. Rules -> GBT -> small LM -> bandit/RL.
4. **Consolidation ("sleep")**: nightly clustering, summaries, schemas, contradiction resolution, decay.
5. **Online adapter (later)**: small model/LoRA/hypernetwork for style, skills, preferences, with replay + eval gate + rollback.
6. **Governance**: audit log, reversible writes, deletion, poisoning checks, human review for low confidence.
7. **Eval harness**: LoCoMo, LongMemEval, BEAM + own probes (updates, abstention, forgetting).

## Differentiator vs existing (Mem0, Zep/Graphiti, Letta, Memory-R1, MemCon)
Unified, open, trained-from-usage system: learned embeddings + controller + consolidation + safe online adaptation + governance, built on logs from day one.

## Phases
1. **Week 1-2 MVP**: store + hybrid retrieval + rules + full outcome logging.
2. **Week 3-4**: nightly consolidation + eval harness + baselines (Mem0/Graphiti, long context).
3. **Month 2**: learned reranker/embedding projections; bandit for retrieval depth.
4. **Month 3**: learned write policy (Memory-R1 style) + small-LM judge for contradictions.
5. **Later**: LoRA/hypernetwork adapter for preferences/skills; governance hardening; open-source release.

## Adding the "Kafka for AI models" (event bus for agents) idea
Source: earlier chat "Event-based architecture for LLM systems" (March 2026): event-driven bus with LLM-native events (`llm.tool.invoked`, `agent.context_pressure`, `agent.handoff`, `memory.recalled`, `memory.stored`), replay, dead-letter, cost-aware backpressure, semantic routing/dedup; positioned as "Segment for agents", not "Confluent for agents".

**How it merges with memory: the event log is the spine, memory is a materialized view of it (event sourcing).**
```
agents / tools / users --> EVENT BUS (NATS JetStream / Redpanda / Kafka-compatible)
   topics: agent.*  llm.*  memory.*  outcome.*  governance.*
        |-> Extractor      (events -> memory candidates)
        |-> Controller     (decision model: write/read/forget, logs propensities)
        |-> Indexer        (embeddings, graph, BM25 = derived views)
        |-> Consolidator   (windowed stream jobs = "sleep")
        |-> Trainer        (outcome.* -> reranker / embedding / controller training)
        |-> Governance     (audit, ACLs, deletion via tombstone + compaction)
```
Why it is stronger together:
- **Training data comes free**: every retrieval, decision and outcome is already an event (what learned controllers like Memory-R1/MemCon need).
- **Replay = superpower**: rebuild the store with a new embedder/policy, migrate models, debug sessions, and do offline counterfactual evaluation of new decision models.
- **Provenance and audit by construction**: each memory points to its source events; deletion = tombstone + rebuild.
- **Multi-agent memory**: shared/scoped memory streams, handoffs carry memory pointers; fits agent-to-agent coordination and governance.
- **Cost-aware backpressure and semantic dedup** apply to memory writes/reads too.
- **Sovereign/on-prem friendly**: self-hosted bus + local models.

Risks: Confluent/Redpanda adding agent features; MCP/A2A commoditizing the protocol layer; hype cycle. Mitigation: be Kafka-compatible (a layer above), differentiate on agent-native memory + learning + governance, not raw streaming.

Phase tweak: Weeks 1-2 use NATS JetStream or Redis Streams as the log; store = derived view; all decisions/outcomes emitted as events from day one.

## Decisions needed
- Personal assistant vs team/product agent (changes privacy, multi-user needs).
- Target stack/agent framework and base models (local vs API).
- Build on Mem0/Graphiti vs from scratch.
- Success metric and baseline to beat.
