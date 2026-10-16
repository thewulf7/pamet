---
title: How human memory works
date: 2026-10-16
tags: [neuroscience, memory, cognition, ai-agents]
related: "[[transformer-style-agent-memory]]"
status: draft
---

# How human memory works

## 1. Direct answer
Human memory is not a recording. It is a set of cooperating systems that **encode** experience, **consolidate** it from fast/fragile to slow/stable form, and **reconstruct** it at recall time, guided by cues and context. Memories are distributed across networks of neurons (engrams), strengthened by synaptic plasticity, and rewritten every time they are retrieved.

## 2. Step-by-step

### Stage 1: Encoding
- Sensory input -> **sensory memory** (iconic ~0.5 s, echoic ~3-4 s).
- Attention selects what passes on. No attention, no memory.
- **Working memory** (prefrontal cortex; Baddeley model: phonological loop, visuospatial sketchpad, central executive, episodic buffer) holds roughly 4 chunks for seconds to a minute.
- Depth of processing matters: meaning, emotion, and links to existing knowledge encode far better than shallow repetition.

### Stage 2: Storage and consolidation
- **Hippocampus** binds the elements of an episode (what, where, when, who) into one index/trace. It encodes fast.
- **Synaptic consolidation** (minutes to hours): long-term potentiation, protein synthesis, structural changes at synapses.
- **Systems consolidation** (days to years): during sleep (slow-wave sleep and sharp-wave ripples, REM) the hippocampus "replays" traces to the neocortex, which gradually extracts regularities. Complementary Learning Systems theory: fast hippocampal learning + slow neocortical learning avoids catastrophic interference.
- **Amygdala** tags emotionally significant events, boosting consolidation. Dopamine/novelty signals also boost it.

### Stage 3: Types of long-term memory
- **Declarative (explicit)**: episodic (events: "my first day at work") and semantic (facts: "Prague is in Czechia"). Hippocampus + neocortex.
- **Non-declarative (implicit)**: procedural skills (basal ganglia, cerebellum), priming, conditioning (amygdala), habits.

### Stage 4: Retrieval
- Driven by **cues** (context, mood, smell, words). Encoding specificity: recall is best when retrieval cues match encoding context.
- Retrieval is **reconstruction**: pattern completion from partial cues in hippocampus, then assembling details in cortex, filling gaps with schemas and expectations.
- **Reconsolidation**: a retrieved memory becomes labile and is stored again, possibly modified. Every recall can change the memory.

### Stage 5: Forgetting
- Mostly **retrieval failure and interference**, not erasure. Decay (Ebbinghaus curve), interference (old vs new), and active forgetting (suppression, synaptic pruning) all occur.
- Forgetting is a feature: it improves generalization and reduces clutter.
- **Spacing and testing effects**: spaced retrieval practice strengthens memory far more than rereading.

## 3. Alternative perspectives
- **Engram / cellular view**: specific sparse ensembles of neurons hold a memory (Tonegawa, Josselyn); optogenetic reactivation can trigger recall.
- **Constructive memory**: Bartlett and Loftus; false memories are normal, and eyewitness confidence is a poor accuracy signal.
- **Predictive processing**: the brain stores a generative model; memory = what is needed to predict, so surprise drives storage.
- **Multiple trace theory / transformation**: details stay hippocampus-dependent forever, gist moves to cortex.
- **Embodied / distributed cognition**: we offload memory to notes, tools, other people ("transactive memory"). An Obsidian vault is literally an external memory.

## 4. Mapping to AI agent memory
| Brain | AI agent |
|---|---|
| Sensory + working memory | Context window |
| Hippocampus (fast episodic index) | Vector/graph store of episodes |
| Neocortex (slow semantic) | Consolidated summaries/schemas, LoRA |
| Sleep replay | Nightly consolidation job |
| Amygdala/dopamine tagging | Importance/surprise write gate |
| Cue-dependent retrieval | Query rewriting + multi-signal retrieval |
| Reconsolidation | Update-on-use, supersede/versioning |
| Forgetting/pruning | Decay + archive |

See [[transformer-style-agent-memory]].

## 5. Practical summary / action plan
- **For your own learning**: use spaced repetition, retrieval practice (self-testing), interleaving, sleep after study, and link new ideas to existing ones (elaboration).
- **For notes (Obsidian)**: write in your own words, link densely, review on a schedule. This builds retrieval cues.
- **For AI memory design**: copy the two-speed architecture (fast episodic + slow consolidated), a sleep/consolidation cycle, cue-based retrieval, and deliberate forgetting.
