# Contract: `pamet` CLI

Thin CLI over the `pamet-core` library API (contracts/library-api.md), for manual testing and for
use as a standalone tool without embedding the library. All commands operate on a single SQLite
file given by `--db <path>` (default: `./pamet.db`).

## `pamet remember --text "<text>" [--source-id <id>] [--explicit-remember]`

- Calls `remember()`. Prints one line per resulting `WriteDecision`:
  `<decision> rule=<rule_name> item=<id-or-'-'>`.
- Exit code 0 on success (including `skip`/`review` outcomes — those are not failures).

## `pamet recall --query "<text>" [--budget <n, default 5>]`

- Calls `recall()`. Prints the budget-bounded result list (id, type, content snippet, score) or
  the literal line `no relevant memory found` when the result is empty (FR-008).

## `pamet outcome --event-id <id> --item-id <uuid> --used <true|false>`

- Calls `report_outcome()`. Prints `ok` or the error (e.g., `unknown item for event`).

## `pamet review list [--status pending]`

- Lists `review_queue` entries matching the status filter (default `pending`).

## `pamet review resolve --review-id <id> --decision <store|merge|supersede|skip>`

- Calls `resolve_review()`.

## `pamet forget --item-id <uuid>`

- Calls `tombstone()`. Prints `ok`; item stops appearing in `pamet recall` but remains visible via
  `pamet history --item-id <uuid>` (not detailed further here — out of scope beyond "remains
  inspectable", per spec SC-005).

## `pamet export-log [--since <ISO-8601>]`

- Calls `export_log()`, prints newline-delimited JSON events to stdout — the hook point for future
  training/eval tooling (spec SC-006).

## Exit codes

- `0`: command completed (including expected "empty"/"skip"/"review" outcomes).
- `1`: storage/IO error.
- `2`: invalid arguments (e.g., malformed `--item-id`, unknown `--decision` value).
