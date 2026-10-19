# Phase 0 Research: Production-Grade Storage Backend

## Decision 1: Postgres client library

**Decision**: the `postgres` crate (sync, from the `rust-postgres`/`sfackler` project — the same
family as `tokio-postgres`, just the blocking wrapper) + `r2d2` + `r2d2_postgres` for pooling.

**Rationale**: Pamet's entire codebase today is synchronous — every service function takes
`&Store` and calls `rusqlite::Connection` methods directly, with no `async`/`.await` anywhere.
Introducing `tokio-postgres` or `sqlx` (both async-first) would force either (a) wrapping the
whole crate in an async runtime, a far larger and riskier change than this feature's actual
goal, or (b) a `block_on()` shim at every call site, which defeats the point of using an async
driver at all. The sync `postgres` crate gives a real, production-grade Postgres connection
with the exact same calling convention (`client.execute(...)`, `client.query(...)`) the codebase
already uses via `rusqlite`, with zero architectural ripple beyond the storage layer itself.
`r2d2`/`r2d2_postgres` is the standard, widely-used sync pooling layer for this exact driver —
adopting it (Constitution Principle VI) rather than hand-rolling a connection pool.

**Alternatives considered**:
- `tokio-postgres` directly / `sqlx`: rejected — async-first, would force an async rewrite of a
  currently fully-synchronous crate; far larger blast radius than this feature's scope.
- `diesel`: rejected — an ORM/query-builder layer adds real value for CRUD-heavy apps with many
  ad-hoc queries, but Pamet's queries are few, stable, and already hand-written per table; an
  ORM would add a dependency and a learning-curve/abstraction cost without a matching benefit,
  and its default Postgres backend still requires `libpq` (a C library) where the plain
  `postgres` crate can run fully pure-Rust (via `rustls`), which is a better fit for a
  self-hosted-friendly, dependency-light project.
- Adopt an existing memory-system framework (Mem0, Graphiti) wholesale instead of building this:
  rejected per the project's own prior decisions (`benchmark/` already treats Mem0/Graphiti as
  external comparison baselines, not as Pamet's implementation) — the differentiator is the
  rule-based, event-sourced, eval-gated pipeline itself (Principle III/V), not the storage
  driver; swapping storage backends underneath that pipeline is a much smaller, lower-risk move
  than replacing the pipeline with a third-party framework.

## Decision 2: Native vector index

**Decision**: the `pgvector` Postgres extension + the `pgvector` Rust crate's `postgres` feature
(gives a `pgvector::Vector` type that round-trips with the sync `postgres` crate directly).

**Rationale**: FR-005 requires replacing the embedded store's full-table brute-force cosine scan
with a real index. `pgvector` is the de facto standard, open-source, self-hostable Postgres
extension for this (not a cloud-only managed-vector-DB dependency, consistent with the
sovereignty constraint) and integrates with a single `CREATE EXTENSION` + a `vector` column
type — no new service, no new wire protocol (Principle VI). The existing hashed bag-of-words
embedder (Phase 1 decision, Principle III: frozen, rule-based) is unchanged — pgvector just
indexes whatever fixed-size float vector `embedding::embed()` already produces; the embedding
*model* is not part of this feature's scope (FR-010).

**Alternatives considered**:
- Keep brute-force scan on Postgres too (skip FR-005): rejected — defeats the stated purpose of
  moving to a "production-grade" backend; the brute-force scan is precisely the thing Phase 2's
  own tests had to route around (`cargo test --release` requirement for the 1,000-item scale
  test) and SC-003 explicitly requires sub-100ms search at that scale.
- A dedicated external vector database (e.g. Qdrant, Pinecone): rejected — adds a second
  service/dependency alongside Postgres for no proportionate benefit at Pamet's stated scale
  (single-logical-store, no multi-tenant/sharding per spec Assumptions), and several are
  cloud-only-first, in tension with the sovereignty constraint.

## Decision 3: Backend selection and `Store` internals

**Decision**: `Store::open(db_url: &str)` inspects the URL scheme. A `postgres://` or
`postgresql://` prefix selects the new backend; anything else (a file path, or `:memory:`) keeps
today's embedded-SQLite behavior completely unchanged. Internally, `Store` gains a `backend:
Backend` field where `Backend` is an enum of `Sqlite(rusqlite::Connection)` or
`Postgres(r2d2::Pool<PostgresConnectionManager<...>>)`. Existing SQL-touching functions are
updated in place to branch on `store.backend` and call a small backend-specific private helper
co-located in the same file, rather than introducing a new parallel module tree or a `dyn Trait`
abstraction layer.

**Rationale**: Pamet's rule-based logic (`write_policy::evaluate`, `models::decay::multiplier`,
`consolidation`'s clustering/union-find/summary-selection) already operates purely on in-memory
`MemoryItem` structs — it has zero SQL in it today and needs none added; only the persistence
layer (~15 files: `schema.rs`, `events.rs`, `review.rs`, `outcome.rs`, `retrieval_vector.rs`,
`retrieval_keyword.rs`, `retrieval_fusion.rs`, `write_policy.rs`'s `active_items` query,
`consolidation.rs`'s queries, `rebuild.rs`, `eval_harness.rs`'s persistence functions, `lib.rs`)
needs a second, Postgres-dialect implementation of the same operations. Keeping the two
implementations co-located per file (rather than splitting into `sqlite/` and `postgres/`
module trees) keeps each operation's two dialects next to each other for easier review and
keeps this a bounded, mechanical port rather than a speculative new abstraction layer
(Principle VI — build the minimum needed, not a framework).

**Alternatives considered**:
- A `dyn StorageBackend` trait object with one method per table operation: considered, more
  conventional "repository pattern" — rejected as the primary design because Rust trait objects
  here would need either boxed/dynamic return types for every query result (friction against
  `rusqlite`'s/`postgres`'s native row-mapping ergonomics) or a generic-over-backend design that
  monomorphizes every call site anyway, for no clear benefit over an enum given there are
  exactly two backends, not an open-ended set. Documented here, not rejected outright — if a
  third backend is ever needed, revisit this.
- Rewrite the whole crate against an async runtime so a single async driver serves both a
  future web-service deployment and this feature: rejected as out of scope — nothing in spec.md
  (this feature or prior phases) asks for an async web service; this would be solving a problem
  Pamet does not yet have, at the cost of the architectural stability of 53 passing tests.

## Decision 4: Migration mechanism

**Decision**: migration is implemented as `pamet migrate --from <sqlite-path> --to <postgres-url>`,
built almost entirely out of machinery Phase 1/2 already shipped: open the source store, call
the existing `export_log()` to get its full event history, then replay that log — via the
existing `services::rebuild::apply_event` logic, generalized to target either backend instead of
always creating a fresh in-memory SQLite store — into a freshly-initialized store on the target
backend.

**Rationale**: this is a direct payoff of Constitution Principle I. "Migrate to a new backend"
and "rebuild derived state from the event log" are, structurally, the same operation Pamet
already had to build and test for a completely different reason (crash recovery / replay
verification). Reusing it rather than writing a second, bespoke copy-every-table migration path
means the migration path is exercised by the same replay logic every `/speckit-converge` pass
has already stress-tested, and automatically satisfies FR-008 (migration verifiable by replay)
because migration *is* a replay.

**Alternatives considered**:
- A bespoke `INSERT ... SELECT`-per-table copy tool: rejected — would duplicate logic that
  already exists in `rebuild.rs`, and would need its own separate correctness argument instead
  of inheriting the one `rebuild_from_events` already has from three convergence passes.

## Decision 5: Testing strategy for the new backend

**Decision**: a new optional Cargo feature, `postgres-backend` (mirrors the existing `mcp` and
`baseline-hindsight` feature pattern), gates the `postgres`/`r2d2`/`r2d2_postgres`/`pgvector`
dependencies. Postgres-backend-specific tests read a `PAMET_TEST_POSTGRES_URL` env var and skip
cleanly (not fail) if it is unset or unreachable — the exact pattern `test_eval_harness.rs`
already uses for the real local Hindsight container.

**Rationale**: keeps the default `cargo build`/`cargo test` fully offline and dependency-free
(FR-006, SC-004) — nobody who only wants the embedded mode pays for a Postgres driver
dependency they never use. Reusing the project's own established "skip cleanly if the external
dependency isn't reachable" pattern rather than inventing a second convention for the same kind
of problem.

## Decision 6: Verifying true concurrency (SC-001)

**Decision**: a new e2e test spawns two genuine OS threads — one looping live
`remember`/`recall` calls against one pooled Postgres connection, the other running
`consolidate()` against a second pooled connection — and measures p95 latency on the live thread
with and without the concurrent consolidation thread running.

**Rationale**: Phase 2's existing concurrency test (`test_consolidation_e2e.rs`) is explicit in
its own doc comments that it could only test fine-grained *interleaving* on one shared SQLite
connection, because `rusqlite::Connection` isn't `Sync`. A connection pool against a real
multi-connection-capable database removes that limitation entirely — this is the first time
SC-001-equivalent language can be tested with actual thread-level concurrency instead of an
approximation.
