# Contract: `pamet gram` CLI subcommands

Thin CLI surface over the gram-model service (`src/services/gram.rs`), following
this project's existing `contracts/cli-contract.md` style (feature 001) — a thin
layer over a library function, documented by behavior, not implementation.

## `pamet gram generate --cluster-id <id> [--rng-seed <u64>] [--max-tokens <n>]`

- Calls `gram::generate(store, cluster_id, rng_seed, max_tokens)`.
- Prints the generated text to stdout, nothing else, on success.
- **No persisted model for `cluster-id`** → exits non-zero with a specific stderr
  message (e.g. `no gram model for cluster <id> — run 'pamet consolidate' first`),
  never silent empty stdout (FR-009).
- **`--rng-seed` omitted** → a seed is derived from current time; output is valid
  but not reproducible run-to-run (this is intentional — determinism is an explicit
  opt-in via `--rng-seed`, matching the spec's framing that reproducibility is
  conditioned on supplying a seed).
- **`--rng-seed` supplied** → byte-identical output across repeated invocations
  with the same cluster/model/seed (FR-006, SC-002).
- Exit codes: `0` success; `1` storage/IO error; `2` invalid arguments or unknown
  `--cluster-id` (no such cluster at all, distinct from "cluster exists but has no
  model yet" — both are user-facing errors but the message differs).

## `pamet gram show --cluster-id <id>`

- Calls `gram::describe(store, cluster_id)`.
- Prints `gram_order=<n> vocab_size=<n> training_tokens=<n> consolidation_run_id=<id>
  created_at=<iso8601>` on one line (FR-007) — no generation performed, no sampling,
  no PRNG involved.
- **No persisted model for `cluster-id`** → same non-zero/specific-error contract as
  `gram generate` (FR-009).
- Exit codes: same scheme as `gram generate`.

## Side effects of `pamet consolidate` (no new flags — behavior change only)

- After writing a cluster's summary (existing behavior, unchanged), `pamet
  consolidate` now also calls `gram::build_for_cluster()` for that cluster (US1).
- Clusters below the minimum size (`g.len() >= 2`, existing consolidation filter)
  never reach gram-model construction — consistent with them never reaching
  summary construction either.
- Clusters whose combined training-token count is below `GRAM_ORDER` (3) are
  skipped for gram-model construction specifically (they may still get a summary);
  this is reported in the existing `ConsolidationSummary` output as a new
  `grams_skipped_too_small` count, alongside whatever fields that struct already
  reports.
- Clusters whose membership is unchanged since their last gram model was built are
  left untouched — no rebuild, no duplicate row (FR-004).
- `pamet consolidate`'s own exit code/overall contract is unchanged; this is
  additive instrumentation, not a new failure mode for the command.
