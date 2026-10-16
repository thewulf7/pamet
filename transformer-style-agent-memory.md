---
title: Transformer-style memory for AI agents
date: 2026-10-16
tags: [ai-agents, memory, architecture, transformers, idea]
status: draft
---

# Transformer-style memory for AI agents

## 1. Direct answer
Yes, it's a good idea, and a partly validated one. Attention *is* a memory-retrieval mechanism: a query is matched against keys and the matching values are read out. An agent memory built on the same primitives can be designed as a **differentiable-in-spirit, hierarchical store with query/key/value retrieval, multiple "heads", a time signal, and a write/consolidate/forget loop**.
Caveat: attention works because it is trained end-to-end. An external memory around a frozen LLM can't backprop through retrieval, so the "learning" part must be replaced by heuristics, a small trained retriever/reranker, or an outcome-based feedback signal.

## 2. Mapping: transformer concept to memory component
| Transformer | Memory system |
|---|---|
| Token | Memory item (episode, fact, preference, procedure) |
| Key | Index embedding(s) + tags + entities of the item |
| Value | Full content (raw text, summary, links) |
| Query | Embedding built from current task + recent context (+ a rewritten query) |
| Softmax attention | Scored retrieval: weights over memories |
| Top-k / sparse attention | kNN / ANN retrieval, only k items enter the prompt |
| Multi-head attention | Parallel retrievers: semantic, lexical (BM25), entity/graph, temporal, procedural |
| Positional encoding | Timestamps, session ids, recency decay, causal links (X caused Y) |
| Layers (low to high abstraction) | Raw episodes -> summaries -> schemas/beliefs -> user/world model |
| KV cache / context window | Working memory (L1) |
| Weights | Slow memory: LoRA / fine-tune, or distilled "core" prompt (L3) |
| Residual stream | Write-back: results of reasoning are re-stored |

## 3. Step-by-step design
1. **Tiers**: L1 working memory (context), L2 episodic+semantic store (vector + keyword + graph), L3 core/slow memory (small always-loaded profile and rules; optionally LoRA).
2. **Write path (gate)**: after each interaction, extract candidate memories. Score by *surprise* (does it contradict or extend what I know?), *importance*, *reuse likelihood*. Write only above threshold. Deduplicate/merge with near neighbours instead of appending.
3. **Item schema**: `id, type, content, summary, embedding, entities, created_at, last_used, use_count, importance, confidence, source, links[], supersedes`.
4. **Read path (multi-head)**: rewrite the query, run heads in parallel, fuse (reciprocal rank fusion), rerank with a cross-encoder or small LLM, then fit to a token budget. Score = `a*similarity + b*recency_decay + c*importance + d*graph_proximity`.
5. **Consolidation ("sleep")**: periodic job clusters episodes, produces summaries and schemas (layer up), resolves contradictions with `supersedes`, promotes frequently used facts to L3.
6. **Forgetting**: decay unused low-importance items, archive instead of delete, keep provenance so any belief can be traced to its source episodes.
7. **Feedback loop**: log which retrieved items were actually used or cited in the answer. Use that as the training signal for the reranker (this is your substitute for backprop).
8. **Evaluate**: LongMemEval, LoCoMo-style tests plus your own: recall of old facts, updated-fact correctness, abstention when unknown, latency, token cost.

## 4. Prior art to study
- Generative Agents (Park et al. 2023): recency + importance + relevance scoring, reflection.
- MemGPT / Letta: OS-style paging between context and external store.
- Memorizing Transformers, kNN-LM, RETRO: kNN attention over external memory inside the model.
- Infini-attention, Titans (Google): compressive / test-time learned memory inside the architecture.
- Graph-based memory (GraphRAG, Zep/Graphiti style temporal knowledge graphs).

## 5. Alternative perspectives
- **Plain files beat fancy systems more often than expected**: markdown notes + good links + grep/BM25 (an Obsidian vault works as a human-auditable memory layer).
- **Knowledge graph first**: if the domain is entity-heavy (people, projects, tickets), a temporal graph beats pure embeddings on multi-hop and "what changed" questions.
- **Parametric memory**: periodic LoRA on distilled facts; cheap at inference, but hard to edit or delete, so use only for stable knowledge.
- **Just use long context + prompt caching**: for small memories (<200k tokens) it can beat any retrieval system. Build the system only when you outgrow it.
- **Learned memory controller**: train a small model (RL on task success) to decide write/read/forget, instead of hand-tuned heuristics.
- **Biological view**: complementary learning systems (hippocampus = fast episodic, neocortex = slow semantic); consolidation is the key piece most implementations skip.
- **Risks**: memory poisoning/prompt injection via stored content, privacy and deletion rights, stale beliefs, self-reinforcing errors (the agent retrieves its own hallucinations).

## 6. Action plan
1. Day 1: SQLite (or Postgres+pgvector) with the schema above; embeddings + FTS5 keyword index.
2. Day 2: Write gate (LLM extractor with a JSON schema) and read path (hybrid search + RRF + token budget).
3. Day 3: Scoring function with recency/importance; log retrieval -> used/not-used.
4. Week 2: Nightly consolidation job (cluster, summarize, supersede) and a markdown export to an Obsidian vault for inspection.
5. Week 3: Eval set of ~100 questions (fresh facts, updated facts, unknown facts, multi-hop); compare against the baseline of "stuff everything into long context".
6. Later: train a reranker from usage logs; try graph head; consider LoRA for L3.
7. Decide upfront: what is your success metric (accuracy, cost, latency) and what's the baseline to beat.

## 7. What already exists (checked 2026-10-16)
**Application-layer memory (most mature, open source)**
- **Mem0**: drop-in memory layer; hybrid retrieval (semantic + keyword + entity), extraction, decay. Self-reports ~92-94 on LoCoMo/LongMemEval.
- **Zep / Graphiti**: temporal knowledge graph, facts have validity windows; vector + full-text + graph retrieval. Graphiti is open source (Apache 2.0).
- **Letta (ex-MemGPT)**: agent manages its own memory blocks (OS-style tiers); current versions use a Git-backed file system for long-term memory.
- **Cognee** (KG + vector), **LangMem** (LangGraph), **Memori**, **ReMe** (file-based), **claude-mem**, **memorywire** (vendor-neutral wire format + router), **Aegis Memory** (security/ACLs for multi-agent).
- Consensus: hybrid retrieval, contradiction handling and consolidation are now table stakes.

**Architecture-layer memory (inside the model; closest to the "transformer-like" idea)**
- **Titans** (Google, NeurIPS 2025): neural long-term memory module that learns to memorize at test time via gradient updates; attention = short-term memory, neural memory = long-term; scales past 2M context in needle-in-haystack tests. Memorizing Transformers, Infini-attention, RETRO, kNN-LM are earlier relatives.

**Benchmarks**: LoCoMo, LongMemEval, BEAM. Vendor numbers are self-reported with different judges/budgets, so treat as non-comparable.

## 8. Where the gap probably is
- CORRECTION (2026-10-16, after a deeper search): learned memory controllers DO exist as research (Memory-R1, MemCon, CoEvo-Mem, UpliftMem, AgeMem, MemAct, MEM1). See [[learned-memory-controllers-prior-art]]. What is missing is a *productized, open, combined* system (learned embeddings + controller + consolidation + safe online adapter).
- Multi-head retrieval + abstraction layers exist piecemeal (Zep, Mem0) but not as one principled attention-like design.
- Security/poisoning and provenance are under-served outside Aegis-like projects.
- Test-time-learned memory (Titans-style) exists in papers, not in agent frameworks.
