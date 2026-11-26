# Research: Real Semantic Memory Search

All decisions below are grounded in real documentation/source checks performed
during this planning session (not guessed), per `verify-before-coding` discipline.

## Decision 1: Vector storage/search — `sqlite-vec`, statically linked

**Decision**: Use the official `sqlite-vec` Rust crate (crates.io: `sqlite-vec`,
current stable `0.1.9`). It embeds the `sqlite-vec` C source and compiles/statically
links it via the `cc` crate at build time — **no loadable extension, no runtime
network dependency**. Registration is one-time per process via
`rusqlite::auto_extension::register_auto_extension`, transmuting
`sqlite_vec::sqlite3_vec_init` into rusqlite's `RawAutoExtension` type.

**Rationale**: Native, no interpreted runtime (matches the user's standing
preference, recorded repeatedly for this project). Statically linked means the
Docker "offline default build" claim stays true for the vector feature itself
(unlike the embedding model, which is a real data asset, not code — see
Decision 3).

**Alternatives considered**: Keep the current brute-force Rust scan but add an
in-memory index (e.g. a simple IVF built at startup) — rejected: reinvents what
`sqlite-vec` already does correctly and tested, violating Constitution Principle
VI (build-don't-reinvent). `pgvector` — rejected outright, the project is
SQLite-only (constitution v1.1.0); no Postgres path exists to put it in.

**Real compatibility finding**: the documented example at
alexgarcia.xyz/sqlite-vec/rust.html uses an older rusqlite `sqlite3_auto_extension`
call pattern that a real, resolved GitHub issue (asg017/sqlite-vec#206, closed
2025-03-08) found does not compile against current rusqlite — the correct current
pattern is:
```rust
use rusqlite::auto_extension::{RawAutoExtension, register_auto_extension};
use sqlite_vec::sqlite3_vec_init;
unsafe {
    let raw_ext: RawAutoExtension = std::mem::transmute(sqlite3_vec_init as usize);
    register_auto_extension(raw_ext)?;
}
```
`rusqlite::auto_extension` is gated behind rusqlite's `loadable_extension`
Cargo feature (confirmed via docs.rs) — the project's current pinned rusqlite
version (`0.31`, from `Cargo.toml`) predates this module's stabilization; a
version bump is required (exact target version to be pinned during
implementation, verified by running the full existing regression suite against
it before this feature's own code is added — a dependency bump is a
whole-crate risk, not scoped to this feature alone, and must be verified as such).

## Decision 2: Embedding model runtime — `ort` (ONNX Runtime)

**Decision**: Use the `ort` crate (currently `2.0.0-rc.15`, wraps ONNX Runtime
1.31, per pykeio/ort's own version-mapping docs) to run a frozen sentence-embedding
model exported to ONNX format.

**Rationale**: Safe Rust wrapper around a mature, widely-used inference runtime;
no Python, no interpreted runtime at any point. `ort` is the de facto standard
choice for ONNX inference from Rust (confirmed via its crates.io listing and
GitHub release cadence — actively maintained, not an abandoned wrapper).

**Alternatives considered**: `candle` (Hugging Face's pure-Rust ML framework) —
a real alternative worth a second look if `ort`'s native-library linking proves
troublesome on a target platform, but `ort` was chosen first since it's the more
mature/battle-tested path for *running* a pre-exported model (candle shines more
for training/custom architectures, not relevant here — Principle III forbids
training anyway).

## Decision 3: Embedding model choice and packaging

**Decision**: `sentence-transformers/all-MiniLM-L6-v2`, ONNX-exported (verified
real exports exist on HuggingFace, Apache-2.0 licensed, 384-dimensional output,
`model.onnx` real file size ~90.4MB). Tokenization via the `tokenizers` crate
(HuggingFace's official Rust tokenizer library) loading that model's
`tokenizer.json` directly — not a hand-rolled WordPiece implementation, which
would be a real correctness risk for a component whose whole point is being a
frozen, trustworthy feature extractor.

**Packaging decision**: The model is **not** vendored into the git repository
(a ~90MB binary asset in version control grows the repo forever and has no
diffability) and **not** baked into the default Docker image (keeps the existing
"default build stays offline/dependency-free" claim about the *build*, which this
decision does not want to break). Instead: a new one-time setup command
(`pamet embedding fetch-model`) downloads the model + tokenizer config into a
local cache directory (alongside the existing `~/.hermes/pamet/`-style data
directory convention) once, with a checksum check. This is explicitly a
*setup-time* action, not part of "normal operation" (FR-002/FR-008's offline
requirement is about write/read/query time, not initial setup) — the same
distinction tools like `ollama`/`whisper.cpp` make for their own model files.
For genuinely air-gapped deployments, the Dockerfile gets an additional optional
build arg (matching the existing `FEATURES=...` pattern) to bake the model in at
image-build time instead.

**Rationale**: Matches the project's existing "optional, explicit, never silent"
pattern for anything that adds a real dependency (same spirit as the
`postgres-backend`/`baseline-hindsight` opt-in Cargo features).

**Alternatives considered**: A smaller/quantized model (int8) — real quantized
ONNX exports of the same model exist (confirmed in the HuggingFace listing) and
are a reasonable follow-up optimization once real backfill/query timing (SC-002,
SC-004) is measured against the fp32 version first; not chosen as the initial
default since a known-good fp32 baseline should be established before trading
accuracy for size.

## Decision 4: Re-embedding existing data (FR-006/FR-010/SC-003/SC-004)

**Decision**: A dedicated, explicit CLI subcommand (`pamet embedding backfill`),
following feature 008's `pamet gram`/consolidation precedent — never an implicit
side effect of opening a store. It must print real elapsed time and item counts on
completion (same pattern as `pamet consolidate`'s existing summary line), and the
implementation task list must include running it for real against a *copy* of the
project's own live ~25,000-item store before it is considered safe to run against
the live file — exactly the discipline feature 008's schema migration used after
a real corruption scare during that feature's own implementation.

**Rationale**: FR-010/SC-004 are explicit, non-negotiable requirements that this
be measured, not estimated — this project has a direct, recent, real precedent
(feature 008) for why that discipline matters: an earlier, logically-correct
migration approach there was *measured* to take minutes against real data due to
a library-version-specific slowdown that no amount of code review would have
caught without actually running it.

## Decision 5: `vec0` schema shape

**Decision**: A new virtual table `memory_items_vec` (following the existing
`memory_items_fts` naming convention), storing the 384-dim MiniLM vector keyed by
`memory_items.id`, plus a plain column recording which embedding-model
identifier/version produced each row (FR-009) — stored in a small ordinary table
(`embedding_models` or similar), not inside the vec0 virtual table itself (vec0
tables are specialized for vector columns; metadata belongs alongside it, same
pattern `cluster_gram_models` uses a plain table next to specialized storage).

**Rationale**: Keeps the "which model produced this" tracking queryable with
ordinary SQL, and keeps the vec0 table itself minimal (vec0's own documentation
recommends this separation for anything beyond the vector + primary key).

**Alternatives considered**: Storing model version as a vec0 metadata/auxiliary
column (sqlite-vec supports this in newer versions) — worth revisiting in
`data-model.md` once the exact pinned sqlite-vec version's auxiliary-column
support is confirmed during implementation; defaulting to the simpler
separate-table approach for the initial design since it has no dependency on a
specific sqlite-vec version's feature set.
