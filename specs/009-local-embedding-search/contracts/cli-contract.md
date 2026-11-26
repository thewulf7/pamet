# Contract: `pamet embedding` CLI subcommands

Thin CLI surface over the new embedding/vector-search services, following this
project's existing `contracts/cli-contract.md` style.

## `pamet embedding fetch-model`

- One-time setup command (Decision 3, research.md). Downloads the frozen ONNX
  model + tokenizer config into the local model cache directory, verifies a
  checksum, and reports success/failure clearly.
- **Already present and checksum-valid** → no-op, prints a confirmation, exits 0
  (idempotent — safe to run again).
- **Checksum mismatch or download failure** → exits non-zero with a specific
  stderr message; never silently proceeds with a corrupt/partial model file.
- This is the only Pamet CLI command that requires network access, and it never
  runs implicitly as a side effect of any other command (FR-002/FR-008) — a
  missing model surfaces as a clear startup error (see `fetch-model` guidance
  below), not an automatic silent download.

## `pamet embedding backfill [--batch-size <n>]`

- Processes every `memory_items` row where `embedding_model_id IS NULL` (or
  does not match the currently configured model id), in batches, generating a
  real embedding via the loaded ONNX model and writing the corresponding
  `memory_items_vec` row + `embedding_models`/`memory_items.embedding_model_id`
  update, each wrapped with an `embedding_backfilled` event (data-model.md).
- Prints a real-time progress line and a final summary (`backfilled=<n>
  already_current=<n> errors=<n> elapsed=<duration>`) — matching the existing
  `pamet consolidate`/`pamet import-hindsight` summary-line convention.
- **Model not fetched yet** → exits non-zero with a message pointing at
  `pamet embedding fetch-model`, never a silent partial run.
- **Interrupted mid-run** → safe to re-run; already-backfilled items are
  skipped (idempotent, per data-model.md's state-transition notes).
- Exit codes: `0` success (including "nothing to do"); `1` storage/IO/model
  error; `2` invalid arguments.

## Side effects of `remember()` (no new flags — behavior change only)

- The live write path now generates the item's embedding via the loaded ONNX
  model instead of the hashed-BoW function, and writes both
  `memory_items.embedding_model_id` and the corresponding `memory_items_vec`
  row as part of the same write (no separate backfill step needed for newly
  written items).
- **Model not fetched/loaded** → `remember()` itself fails clearly (FR-008) —
  the live write path must not silently fall back to the old hashed-BoW vector
  or to an empty/degenerate vector.

## Side effects of `recall()` (no new flags — behavior change only)

- The vector half of hybrid retrieval now queries `memory_items_vec` via
  `sqlite-vec`'s native KNN instead of scanning every `memory_items` row and
  scoring in Rust. The keyword half (`retrieval_keyword.rs`, FTS5/BM25) and the
  fusion logic (`retrieval_fusion.rs`) are unchanged by this feature.
- An item with `embedding_model_id IS NULL` (not yet backfilled) is excluded
  from the vector half of results (it has no comparable vector), but remains
  fully findable via the keyword half — never a hard failure, never a false
  "no vector = zero relevance" exclusion from the keyword path too.
