---
title: Embeddings and decision model for the agent memory system
date: 2026-10-16
tags: [ai-agents, memory, embeddings, decision-model, rl, architecture]
related: "[[transformer-style-agent-memory]], [[human-memory-how-it-works]], [[online-learning-memory-model]]"
status: draft
---

# Embeddings + decision model

## 1. Direct answer
Add two learned components on top of the store:
- an **embedding layer** (several spaces, one trained retrieval projection) = the "keys/queries" of the memory attention;
- a **decision model (memory controller)** that chooses actions: write / merge / supersede / skip, retrieve or not (and how), consolidate, forget, ask, abstain.
Start with rules and simple supervised models; move to bandit/RL only when you have logged outcomes.

## 2. Pipeline
```
event/message
  -> embed (content, entities, time, type)
  -> candidate memories (hybrid retrieval: dense + BM25 + entity/graph)
  -> DECISION MODEL (write policy): store | merge | supersede | skip
  -> store (items + embeddings + metadata)

user query / task
  -> DECISION MODEL (read policy): retrieve? which heads? k? budget?
  -> multi-head retrieval -> fusion (RRF)
  -> learned reranker (attention-style Wq/Wk over embeddings)
  -> context assembly within token budget -> main LLM
  -> outcome logging (used? corrected? thumbs? task success? cost)
  -> training data for embedder, reranker, decision model
```

## 3. Embeddings
- **Multiple spaces / roles**
  - semantic content (sentence embedding),
  - asymmetric query vs document embeddings (queries rewritten to look like memories),
  - entity embeddings (people, projects, tools) tied to a graph,
  - time encoding (sin/cos or bucketed recency features, kept as features, not only in vector),
  - type/scope tags (episodic, semantic, procedural, user, agent).
- **Efficiency**: Matryoshka-style truncatable vectors for coarse-to-fine search; quantization (int8/binary) for scale; optional late interaction (ColBERT-style) for the rerank stage; always hybrid with BM25.
- **Learned "memory attention"**: freeze a good base embedder, train small projections `Wq`, `Wk` (and maybe a value gate) on your logs so that `score = (Wq q) . (Wk k)` matches what was actually useful. This is the closest literal analogue of transformer attention over an external memory.
- **Training signal**: contrastive loss; positives = memories the answer used or the user confirmed; hard negatives = retrieved but unused or contradicted.
- **Operations**: version embeddings; re-embed lazily on model change; keep model id in every row; evaluate recall@k per version.

## 4. Decision model
**Actions**
| Decision point | Actions |
|---|---|
| Write | store new, merge into existing, supersede old (keep history), skip, queue for human review |
| Read | retrieve or not, which heads (dense/BM25/graph/temporal), k, scope, depth (summary vs raw) |
| Maintenance | consolidate cluster, promote to core, decay, archive, delete on request |
| Dialogue | ask clarifying question, abstain ("I don't know") |

**Features**: novelty (max similarity to existing), contradiction score (NLI/LLM judge), importance, user emotion/explicit "remember this", source trust, recency, past usage count, query type/complexity, token budget, latency/cost.

**Model ladder (climb only when the previous rung plateaus)**
1. Hand-written rules + thresholds.
2. Logistic regression / gradient-boosted trees on the features (fast, debuggable).
3. Small LM (0.5-3B) classifier/judge producing structured decisions with reasons.
4. Contextual bandit: reward-driven choice of retrieval depth/heads (log propensities for off-policy evaluation).
5. Full RL policy (reward = answer quality - cost - user corrections) only with a solid simulator/eval set.

**Reward signals**: memory cited/used in answer, user correction or thumbs, task success, abstention correctness, token and latency cost penalty, long-term: was the memory later contradicted?

**Guardrails**
- Hard rules outside the learned model for sensitive categories and deletion requests.
- Low confidence -> human review queue or ask the user.
- Explore only on low-risk decisions (e.g., retrieval depth), never on writes of sensitive data.
- Full audit log: input, decision, score, reason, outcome. Reversible writes (supersede, not overwrite).

## 5. Alternatives
- LLM-as-controller (prompted) instead of trained model: fastest start, costly and noisy.
- Letta-style: let the main agent decide memory edits via tools (simple, but couples memory quality to the main model).
- Single end-to-end model (Titans-like) that internalizes the write decision via surprise; elegant but research-stage.
- Learned-to-rank only (skip write decisions): store everything cheaply, invest in the reranker and consolidation.

## 6. Action plan
1. Define the item schema + action space + logging (include propensities and model versions).
2. Baseline: hybrid retrieval (dense + BM25) + RRF + rules for write/read.
3. Collect 2-4 weeks of logs; build probe set (new fact, updated fact, unknown, multi-hop, preference).
4. Train `Wq/Wk` reranker projections on frozen embeddings; compare recall@k and answer accuracy.
5. Replace rules with GBT write/read policy; check against probes and cost.
6. Add small-LM judge for contradiction/supersede decisions.
7. Try a contextual bandit for retrieval depth/heads.
8. Nightly consolidation job + eval gate + rollback ([[online-learning-memory-model]]).
