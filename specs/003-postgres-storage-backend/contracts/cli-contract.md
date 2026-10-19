# CLI Contract: Production-Grade Storage Backend

Extends `specs/001-memory-store-mvp`'s and `specs/002-consolidation-eval-harness`'s existing CLI
contract. No existing subcommand's observable behavior changes (FR-010) — this adds backend
selection to the existing `--db` argument and one new subcommand.

## `--db <target>` (existing argument, extended meaning)

- Unchanged: a file path (or `:memory:`) selects the embedded SQLite backend — exactly today's
  behavior, byte-for-byte (FR-006, SC-004). No existing script or workflow needs to change.
- New: a value starting with `postgres://` or `postgresql://` selects the new Postgres backend.
- If a `postgres://`/`postgresql://` URL is given and the server is unreachable, the process
  exits non-zero immediately with a clear error identifying the connection target — it MUST NOT
  silently fall back to an embedded store (FR-007, Edge Cases).

## `pamet migrate --from <source> --to <target>` (new subcommand)

- `--from`: a `--db`-style source (today, in practice, always an existing embedded SQLite file —
  migrating *from* Postgres is not a use case this feature needs, though the implementation
  does not need to special-case against it).
- `--to`: a `--db`-style target — in practice, always a `postgres://`/`postgresql://` URL for
  this feature's purpose.
- Refuses to run (non-zero exit, clear error) if `--to` already contains any data — migration
  always targets a freshly-initialized store (data-model.md "Migration Run" — re-run safety).
- On success, prints a one-line summary to stdout:
  `migration_complete items=<n> events=<n> reviews_pending=<n> reviews_resolved=<n>` — the same
  counts for both source and target are available via the existing `export-log`/`review list`
  commands against each, for the operator to independently diff (FR-008 verification path; this
  command does not invent a new verification report format).
- Exit code 0 on success; non-zero on any error (unreachable source/target, non-empty target,
  a replay error during the underlying `rebuild`-equivalent pass).

## Feature gating

- The Postgres backend and the `migrate` subcommand both require the crate to be built with
  `--features postgres-backend` (mirrors the existing `mcp` / `baseline-hindsight` pattern). A
  default build (no extra features) has no new runtime dependency and behaves exactly as it
  does today — passing a `postgres://` URL to a build without this feature fails with a clear
  "not compiled in" error, not a silent no-op.
