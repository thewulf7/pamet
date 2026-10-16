# Research: Memory Store MVP

All Technical Context items were resolved with reasonable defaults from the project's design docs
and constitution rather than left as open unknowns — no `NEEDS CLARIFICATION` markers remain.

> **Post-implementation note (2026-10-16, T045)**: the "Embedding model" and "Storage engine"
> decisions below describe the *originally planned* `ort`/ONNX + `sqlite-vec` stack. The shipped
> MVP uses a deterministic hashed bag-of-words embedding and Rust-computed cosine similarity
> instead — see plan.md's Technical Context deviation note and `tasks.md`'s Implementation Notes
> for the reasoning (buildable without a downloaded model file or loadable C extension). The
> rationale below is kept as the record of the original decision and the bar a future swap to a
> real sentence-transformer should clear.

## Decision: Implementation language — Rust

- **Decision**: Rust (stable, edition 2021).
- **Rationale**: Native, no interpreted-runtime dependency, compiles to a single self-hosted
  binary/library (Constitution's sovereign/on-prem constraint), matches the user's established
  preference for native implementations over subprocess/interpreted wrappers, and matches the
  stack already used in this user's other systems (wz-teams: Rust/Axum).
- **Alternatives considered**:
  - Python — rejected: user has twice pushed back on Python for this kind of system; also adds a
    runtime dependency that complicates "self-hosted, sovereign-friendly" deployment.
  - TypeScript/Node — rejected: weaker native vector/embedding ecosystem; adds a JS runtime
    dependency for no benefit over Rust here.

## Decision: Storage engine — SQLite (rusqlite) + FTS5 + sqlite-vec

- **Decision**: Single SQLite file via `rusqlite`, FTS5 virtual table for keyword search, and the
  `sqlite-vec` loadable extension for vector similarity search.
- **Rationale**: Matches the constitution's explicit "SQLite or Postgres" store constraint;
  SQLite needs no server process, stays portable, and both FTS5 and sqlite-vec are mature, open,
  embeddable components — satisfying Principle VI ("build-don't-reinvent": don't hand-roll a
  tokenizer or ANN index).
- **Alternatives considered**:
  - Postgres + pgvector — rejected for the MVP: requires a running server process, which works
    against "no cloud-only dependency" / easy local self-hosting for a first feature; revisit
    when a multi-process or multi-tenant need appears.
  - A dedicated vector DB (Qdrant/Weaviate) — rejected: another service to run and operate for an
    MVP whose scale target is 50k items on one machine; adds ops overhead with no MVP-stage payoff.

## Decision: Embedding model — local ONNX model via `ort`, no network call

- **Decision**: Run a small, open sentence-embedding model (e.g., a MiniLM/BGE-small class model)
  locally through the `ort` (ONNX Runtime) Rust bindings; the model file ships/downloads once and
  all embedding calls are in-process.
- **Rationale**: Constitution Architecture Constraints: "start from a frozen base embedder" and
  "MUST support a self-hosted / on-prem mode; no feature may require a cloud-only dependency as
  its sole implementation." A local ONNX model satisfies both with no learned fine-tuning (frozen,
  per Principle III) and no external API dependency.
- **Alternatives considered**:
  - Hosted embedding API (OpenAI/Cohere/etc.) — rejected as the *sole* path: would violate the
    no-cloud-only-dependency constraint; could be added later as an optional pluggable backend,
    not required for this MVP.
  - Train/fine-tune an embedder now — rejected: Principle III (rules before learning) — no
    learned component belongs in this MVP at all, trained or not.

## Decision: Keyword matching — SQLite FTS5 (BM25 ranking built in)

- **Decision**: Use SQLite's FTS5 virtual table with its built-in BM25 ranking function as the
  keyword/lexical half of hybrid retrieval.
- **Rationale**: Ships inside SQLite already in use for storage — zero extra dependency, proven,
  and exactly the "BM25" method named in the design docs' hybrid-retrieval pipeline.
- **Alternatives considered**:
  - Tantivy (standalone Rust full-text engine) — rejected for MVP: a second index/storage engine
    to keep in sync with SQLite, for a capability FTS5 already provides at this scale.

## Decision: Result fusion — Reciprocal Rank Fusion (RRF), hand-written rule

- **Decision**: Fuse the vector-similarity ranked list and the FTS5/BM25 ranked list with
  Reciprocal Rank Fusion (`score = Σ 1 / (k + rank_i)`), a fixed `k` constant, no learned weights.
- **Rationale**: RRF is the exact fusion method named in the design docs' retrieval pipeline, is
  deterministic/inspectable (Principle III: rules before learning), and needs no training data to
  start producing reasonable rankings on day one.
- **Alternatives considered**:
  - A learned reranker (cross-encoder or Wq/Wk projection) — explicitly deferred: Principle III
    and the spec's own Assumptions rule out any learned component in this MVP; RRF output is also
    exactly what future reranker training will be evaluated against as the baseline (spec SC-002,
    SC-003 and Constitution Principle V).

## Decision: Write policy — hand-written rule thresholds, not a model

- **Decision**: Deterministic rules operating on: max similarity to existing items (novelty),
  an NLI/keyword-based contradiction check, and an explicit "remember this" signal, each compared
  against fixed, documented thresholds, to choose store / merge / supersede / skip / review-queue.
- **Rationale**: Directly mandated by spec FR-002/FR-003 and Constitution Principle III; thresholds
  are logged alongside every decision so a future GBT/LM policy (per the design docs' model ladder)
  has rule-decision parity data to beat.
- **Alternatives considered**:
  - LLM-as-controller (prompted) — rejected for MVP: non-deterministic, costly per decision, and
    the design docs themselves flag it as "fastest start, costly and noisy" — not worth it when
    rules are cheap and inspectable and satisfy every FR for this feature.

## Decision: Event log representation — append-only SQLite table, not a separate bus

- **Decision**: An `events` table (append-only, SQLite) is the source of truth; `memory_items`,
  FTS5 index rows, and vector index rows are derived/rebuilt from it.
- **Rationale**: Satisfies Principle I (event-sourced spine, replayable) without introducing a
  distributed message bus for a single-process MVP; Principle VI only requires the *bus* to stay
  Kafka-compatible once one is introduced — a single-process MVP legitimately has no bus yet.
- **Alternatives considered**:
  - Stand up NATS JetStream/Redpanda now — rejected as premature for a single-process, single-user
    MVP; revisit when a second process/service needs to consume the event stream (e.g., a
    consolidation job in a later feature, per Constitution Principle IV).
