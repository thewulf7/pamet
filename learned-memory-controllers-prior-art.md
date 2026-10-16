---
title: Prior art for learned memory controllers, embeddings and online adaptation
date: 2026-10-16
tags: [ai-agents, memory, prior-art, rl, hypernetwork, continual-learning]
related: "[[transformer-style-agent-memory]], [[memory-embeddings-and-decision-model]], [[online-learning-memory-model]]"
status: draft
---

# Prior art: does something like our design already exist?

## 1. Direct answer
Yes, most individual pieces exist, mainly as 2025-2026 research. What I did not find is one open, productized system combining all of them. (This corrects my earlier claim that no learned controller existed.)

## 2. Decision model / memory controller (our "decision model")
- **Memory-R1** (Yan et al., ACL 2026): RL (PPO/GRPO) trained *Memory Manager* choosing ADD/UPDATE/DELETE/NOOP + an *Answer Agent* that filters retrieved memories. Reward = downstream answer correctness; reported as trained from few QA pairs (~152 per one source). Learns to consolidate instead of fragmenting memory.
- **MemCon / "Memory as a Controlled Process"** (2026): memory ops as an MDP; lightweight tabular contextual bandit with UCB that decides when/what/how much to retrieve, when to inject a plan, when to consolidate/forget; no extra LLM calls; learns from binary task feedback; wraps any backend. Claims up to +15.2 task-success points and 5-20% fewer tokens.
- **AgeMem, MemAct, MEM1**: memory operations put into the agent's action space and trained with RL (store/delete/summarize/retrieve/prune; MEM1 keeps constant memory size).
- **Mem0**: LLM-chosen ADD/UPDATE/DELETE/NOOP (prompted, not trained).
- **AtomMem, MemAgent**: other RL-learned memory policies (per a governance survey).

## 3. Learned retrieval / reranker / embeddings
- **CoEvo-Mem** (2026): co-evolves retrieval policy and memory bank from interaction feedback; hybrid dense+sparse retriever with learned "memory utility" and routing weights adapted from task rewards.
- **UpliftMem**: learns set-level retrieval from execution feedback (does supplying this memory set improve the same agent's execution?).
- A 2026 paper on a continually adapting reranker with RL updates and an experience buffer (static rerankers don't adapt).
- Gap I still see: very little on training the *embedding space itself* (Wq/Wk projections) from agent usage logs in an open agent framework.

## 4. Memory inside weights / online adaptation (our "small model that trains on the go")
- **Doc-to-LoRA / Text-to-LoRA** (Sakana, ICML 2026): hypernetwork generates a LoRA adapter from a document in one forward pass (sub-second, no per-document gradient steps); near-perfect needle-in-haystack beyond 4x native context. Closest to "instant parametric memory".
- **LatentSkill**: hypernetwork turns textual skills into LoRA adapters for a frozen LLM (procedural memory in weights).
- **SEAL** (MIT, NeurIPS 2025): model writes its own training data (self-edits), SFT updates weights, RL trains the edit generator. 72.5% vs 20% (no RL) vs 0% (ICL) on a few-shot ARC subset with a 1B model; ~30-45 s per edit; catastrophic forgetting not solved; knowing *when* to self-edit is manual.
- **Titans, TTT-style layers, Memorizing Transformers, Infini-attention**: memory inside the architecture.

## 5. Governance / safety
- **SSGM** (Stability and Safety Governed Memory) framework: risks of evolving memory (content, structure, policy).
- **Aegis Memory**: ACLs, agent identity binding for multi-agent memory.

## 6. What this means for us
- Don't start from zero: reuse **Memory-R1** ideas (outcome-reward training of ADD/UPDATE/DELETE/NOOP) and **MemCon** (bandit, no extra LLM calls) for the decision model.
- Use **CoEvo-Mem/UpliftMem** ideas for retrieval learning; add the Wq/Wk embedding projection experiment as our own contribution.
- For the "small model on the go": hypernetwork route (Doc-to-LoRA) beats gradient fine-tuning for latency; SEAL-style synthetic self-edits help fact absorption but cost compute and forget.
- Keep external store as source of truth; use weights for skills/preferences.

## 7. Suggested next steps
1. Read Memory-R1 and MemCon first (decision model); CoEvo-Mem (retrieval); Doc-to-LoRA (parametric memory).
2. Reproduce MemCon-style bandit on top of Mem0/Graphiti as the cheapest win.
3. Add logging of propensities + outcomes so any of these can be trained later.
4. Evaluate on LoCoMo / LongMemEval / BEAM plus own probes (updates, abstention, forgetting).
5. Decide the novel contribution: unified system (learned embeddings + controller + consolidation + safe online adapter) with governance.
