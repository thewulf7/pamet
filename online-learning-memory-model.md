---
title: Small model that learns on the fly as a memory for a main model
date: 2026-10-16
tags: [ai-agents, memory, continual-learning, lora, architecture]
related: "[[transformer-style-agent-memory]], [[human-memory-how-it-works]]"
status: draft
---

# Small model that trains on the go next to a frozen main model

## 1. Direct answer
Yes, theoretically and increasingly practically. It is the machine version of Complementary Learning Systems: a **frozen large model = slow cortex**, a **small, fast-learning model = hippocampus**. The hard parts are not the plumbing but (a) catastrophic forgetting, (b) small models absorb new facts poorly from a few exposures, (c) parametric memory can't be edited, audited or deleted easily, (d) safety/poisoning.

## 2. Designs (from simplest to most ambitious)
1. **Sidecar controller (recommended first)**: the small model learns *what to remember and what to retrieve* (write gate, reranker, query rewriter) from usage logs. Supervised, cheap, safe, no fact-injection problem.
2. **Sidecar memory generator**: small model fine-tuned (LoRA) on the user's history; the main model calls it as a tool or it emits a short "memory brief" injected into the prompt. Main model never changes.
3. **Per-user adapters on the main model**: small LoRA updated periodically, hot-swapped at inference.
4. **Hypernetwork / amortized learning**: a network that maps an episode or document to adapter weights in one forward pass, with no gradient steps at use time.
5. **Test-time-learned memory inside the architecture** (Titans, TTT-style layers): needs a custom architecture and pretraining; research stage.
6. **Self-editing**: model writes its own training data from experience (paraphrases, Q&A, implications) and trains on it (SEAL-style idea).

## 3. Training loop (for designs 2-3)
1. Log interactions -> write gate selects candidates.
2. Convert each to many training forms (paraphrases, QA, implications). Raw text alone is absorbed badly.
3. Mix with **replay** of old and general data (rehearsal) to avoid forgetting.
4. Small LoRA step, low learning rate, during idle time ("sleep").
5. **Eval gate**: held-out probes (new facts, old facts, general ability). Promote if pass, else rollback.
6. Versioned checkpoints, provenance of every training example, ability to retrain without a given example.

## 4. Problems
- Catastrophic forgetting and interference.
- Few-shot fact absorption is weak; updates/contradictions are hard to apply parametrically.
- Reversal-curse-like gaps: learned "A is B" may not give "B is A".
- Deletion / privacy (GDPR): needs retrain or unlearning.
- No source citation; memory can confabulate.
- Poisoning: a malicious input can be baked into weights permanently.
- Compute and ops cost; evaluation is hard.

## 5. Alternative perspectives
- **Hybrid wins**: brain analogy again. Episodes/facts -> external store (editable, auditable). **Skills, style, preferences, habits (procedural memory) -> parametric**.
- **Retrieval first, weights later**: distill frequently retrieved stable knowledge into weights overnight.
- **Don't train at all**: long context + caching + good retrieval may be enough for years.
- **Different small model roles**: summarizer/consolidator, judge for write gate, router between memories.

## 6. Action plan
1. Log every retrieval and whether it was used in the final answer.
2. Train a 0.5-3B model as reranker / write gate on those logs (supervised).
3. Add a nightly LoRA for style/preferences with replay + eval gate + rollback.
4. Build the eval harness first (probes for new facts, updates, forgetting, abstention).
5. Only then experiment with parametric facts via synthetic QA augmentation.
6. Keep the external store as the source of truth; weights are a cache.
