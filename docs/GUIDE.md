# Pamet: Launch & Usage Guide

This covers three things: running the CLI by hand, wiring the MCP server into a real agent
(Claude Code), and the gotchas discovered while doing exactly that. For the feature's own spec,
plan, and design rationale see `specs/001-memory-store-mvp/`.

## 1. Build

```sh
# CLI only (default)
cargo build --release

# CLI + the MCP server binary (pulls in rmcp/tokio/schemars/anyhow — gated behind a
# feature flag so the plain CLI never needs them)
cargo build --release --features mcp
```

Binaries land in `target/release/`:
- `pamet` — the CLI (`remember`, `recall`, `outcome`, `review`, `forget`, `export-log`).
- `pamet-mcp` — the MCP server (stdio transport), only with `--features mcp`.

## 2. Using the CLI directly

Every command takes `--db <path>` (defaults to `./pamet.db`, created on first write).

```sh
# Store something (rule-based write policy decides store/merge/supersede/skip/review)
pamet --db ./pamet.db remember --text "The project is called Pamet."

# Retrieve relevant memories (hybrid vector+keyword search, fused with RRF)
pamet --db ./pamet.db recall --query "what is the project called" --budget 5

# Report whether a returned memory was actually used (closes the training-data loop)
pamet --db ./pamet.db outcome --event-id <id> --item-id <uuid> --used true

# Inspect/resolve the review queue (low-confidence or contradictory candidates)
pamet --db ./pamet.db review list
pamet --db ./pamet.db review resolve --review-id <id> --decision store

# Remove an item from normal retrieval (reversible — tombstoned, not deleted)
pamet --db ./pamet.db forget --item-id <uuid>

# Dump the full append-only event log (newline-delimited JSON, replay-ordered)
pamet --db ./pamet.db export-log
```

Full runnable walkthrough: `specs/001-memory-store-mvp/quickstart.md`.

## 3. Connecting a real agent (Claude Code) via MCP

The MCP server exposes three tools: `remember`, `recall`, `outcome` — same semantics as the CLI
subcommands, described to the agent so it knows when to call each one.

### 3.1 Register it

```sh
cargo build --release --features mcp

# Project-scoped (only active when Claude Code is run from this directory):
claude mcp add pamet -s project -- \
  /absolute/path/to/pamet/target/release/pamet-mcp --db /absolute/path/to/your/memory.db

# Or user-scoped (active everywhere):
claude mcp add pamet -s user -- \
  /absolute/path/to/pamet/target/release/pamet-mcp --db ~/.pamet/agent-memory.db
```

This writes `.mcp.json` (project scope) or your global Claude config (user scope). First
interactive `claude` run will ask you to approve the new server once.

### 3.2 Verify it's wired up

```sh
claude mcp list
# pamet: /path/to/pamet-mcp --db ... - ✔ Connected  (after first approval)
```

Or drive it directly with raw JSON-RPC to sanity-check before trusting an agent to it (the
trailing `sleep` matters — the server exits on stdin EOF, so give it a moment to finish
responding before the pipe closes):

```sh
( printf '%s\n%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'; sleep 0.3 ) \
  | ./target/release/pamet-mcp --db /tmp/pamet-check.db
```
You should get back an `initialize` result and a `tools/list` result naming `remember`, `recall`,
`outcome` with their JSON schemas.

### 3.3 Use it from Claude Code

In an interactive session, just talk to it normally — it will decide to call `remember`/`recall`
on its own once the tools are available and approved. For scripted/headless runs (`claude -p`),
be explicit about which MCP config to load:

```sh
claude -p --strict-mcp-config --mcp-config=.mcp.json -- \
  "Remember that the deploy window is Tuesdays 2-4pm UTC."

# New, independent process — no conversation history, only the persistent pamet DB:
claude -p --strict-mcp-config --mcp-config=.mcp.json -- \
  "When is the deploy window?"
```

## 4. Gotchas found while actually doing this (read before you debug the wrong thing)

- **Claude Code has its own built-in cross-session memory** (writes `.md` files under
  `~/.claude/projects/<project>/memory/`). If you ask it to "remember" something and don't force
  the issue, it will often use *that* instead of your MCP tool — not because your tool is broken,
  but because its native memory is a closer/more habitual option. To get a clean test of pamet
  specifically, either be explicit ("use the mcp__pamet__remember tool") or start the process with
  `--bare` (also skips plugin sync, hooks, LSP — fine for a scripted probe, not for daily use).
- **`--disallowedTools "Write Edit"` does NOT block the native memory feature** — it's not
  implemented as the `Write`/`Edit` tool under the hood. `--bare` is the actual switch.
- **`--mcp-config '{"mcpServers":{}}'` alone is not a "no persistence" baseline** if the agent has
  `Bash` access and your memory CLI binary is sitting in the repo — it will happily shell out to
  `pamet remember ...` directly, bypassing MCP entirely and still persisting the fact. For a true
  memory-less baseline you need `--disallowedTools "Bash Edit Write"` too.
- **`--mcp-config` eats following positional args** if you don't separate the prompt with `--`
  (`claude -p --mcp-config=.mcp.json -- "your prompt"`, not `--mcp-config .mcp.json "your prompt"`)
  — otherwise your prompt text gets parsed as another config file path.
- **Tool calls fired without waiting for a response can race** — the pamet-mcp server serializes
  DB access with a `Mutex<Store>` but if a client pipes multiple `tools/call` requests back-to-back
  without awaiting each response, they can execute out of order. A real agent's tool-calling loop
  awaits each result before issuing the next, so this doesn't bite in normal use — only in
  hand-rolled stdio test harnesses that fire-and-forget multiple requests.

## 5. Minimal verification that it actually works end-to-end

This is the exact sequence that proved pamet-backed memory survives a process restart while a
properly-sandboxed baseline (no MCP, no Bash, no native memory) does not — run it yourself to
reproduce:

```sh
# clean slate
rm -f ~/.pamet-demo/agent-memory.db
echo "# Memory Index" > ~/.claude/projects/-path-to-this-repo-/memory/MEMORY.md  # if it exists

# PAMET condition
claude -p --bare --dangerously-skip-permissions --strict-mcp-config --mcp-config=.mcp.json -- \
  "Remember this fact for future sessions: the office vending machine code is 7734."
claude -p --bare --dangerously-skip-permissions --strict-mcp-config --mcp-config=.mcp.json -- \
  "What is the office vending machine code?"
# -> correctly answers 7734, and `pamet export-log` shows write_decision -> retrieval ->
#    outcome_update, proving it actually called the tools rather than guessing.

# TRUE BASELINE condition (separate fact, zero persistence mechanism available)
claude -p --bare --dangerously-skip-permissions --strict-mcp-config --mcp-config '{"mcpServers":{}}' \
  --disallowedTools "Bash Edit Write" -- \
  "Remember this fact for future sessions: the baseline secret number is 4417."
claude -p --bare --dangerously-skip-permissions --strict-mcp-config --mcp-config '{"mcpServers":{}}' \
  --disallowedTools "Bash Edit Write" -- \
  "What is the baseline secret number?"
# -> correctly says it has no memory and doesn't know the number.
```
