# Tasks: Memory Pipeline Quality

**Input**: Design documents from `specs/006-memory-pipeline-quality/`

**Prerequisites**: plan.md ✅, spec.md ✅, research.md ✅, data-model.md ✅, quickstart.md ✅

**Organization**: Tasks grouped by user story — each story is independently implementable
and testable. US1 and US5 are Python-only; US3 is Rust-only; US2 bridges both. US4 is
skill/prompt-only. No story has a hard dependency on another.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no incomplete-task deps)
- **[Story]**: User story label [US1]–[US5]

---

## Phase 1: Setup

**Purpose**: Verify baseline — spec 005 fixes must be green before any new work.

- [X] T001 Verify spec 005 regressions clean: run `cargo test --lib` from `~/Developer/pamet/` and confirm 17 tests pass

---

## Phase 2: Foundational

No foundational blocking prerequisites — US1–US5 changes are in independent files
(Python plugin, `src/services/review.rs`, `src/bin/pamet.rs`, skill). All user stories
can proceed once T001 is green.

**Checkpoint**: T001 green → all user story phases can begin.

---

## Phase 3: User Story 1 — Prefetch noise suppression (Priority: P1) 🎯 MVP

**Goal**: The agent never fires a Pamet recall on a query with fewer than 3 meaningful
tokens. Question-only messages produce zero candidates on write.

**Independent Test**: Run `pamet remember --text "who am i?" --source-id test` → zero
output lines. Run `pamet recall --query "who" --budget 5` from the plugin with the
stop-word filter active → recall is skipped, empty block returned.

### Implementation for User Story 1

- [X] T002 [US1] Add `_filter_prefetch_query(query: str) -> str` helper in `~/.hermes/plugins/pamet/__init__.py` — tokenizes on whitespace, strips tokens in STOP_WORDS set (FR-010: who, what, where, when, how, why, am, is, are, i, the, a, an, in, on, at, to, do, you), returns `""` if fewer than 3 tokens remain
- [X] T003 [US1] Define `STOP_WORDS: frozenset` module constant in `~/.hermes/plugins/pamet/__init__.py` containing the FR-010 word list
- [X] T004 [US1] Update `prefetch()` in `~/.hermes/plugins/pamet/__init__.py` to call `_filter_prefetch_query(query)` and return `""` early if the result is empty (FR-003)
- [X] T005 [US1] Manually verify: start Python REPL, import plugin, call `_filter_prefetch_query("who am i")` → `""`, `_filter_prefetch_query("Evgenii manager Prague")` → non-empty string

**Checkpoint**: US1 complete — prefetch skips noise queries. Identity queries now surface
facts, not fragments.

---

## Phase 4: User Story 2 — Assistant content retention (Priority: P2)

**Goal**: Facts stated in assistant replies are retained across sessions. The write path
handles deduplication — no new logic needed beyond calling `remember` with assistant text.

**Independent Test**: Call `sync_turn(user_content="tell me about myself", assistant_content="You are Evgenii Utkin, T4-2 EM on Travel Payments.")`, then `pamet recall --query "Evgenii T4 Engineering Manager" --budget 5` in a fresh session → returns the assistant-stated fact.

### Implementation for User Story 2

- [X] T006 [US2] Update `sync_turn()` in `~/.hermes/plugins/pamet/__init__.py` — after the existing `remember` call for `user_content`, add a second `_run(["remember", "--text", assistant_content[:2000], "--source-id", "chat"])` call, guarded by `assistant_content` being non-empty and not write-disabled
- [X] T007 [US2] Manually verify dedup: call `sync_turn` with identical text in both user and assistant fields against a test DB — confirm write policy emits `merge` or `review` for the second write (not `store` twice)

**Checkpoint**: US2 complete — assistant-stated facts are durable.

---

## Phase 5: User Story 3 — Review queue CLI (Priority: P2)

**Goal**: Operators can list pending review items with summary text + source count, and
approve or skip entire clusters in one command. The 1,396-item backlog from the Nov-25
consolidation becomes actionable.

**Independent Test**: Run `pamet review list --limit 10` → each row shows review_id,
reason, source count, and summary text. Run `pamet review approve-cluster <id>` → `ok`,
source items suppressed from recall, summary promoted.

### Implementation for User Story 3

- [X] T008 [US3] Add `pub struct EnrichedReviewRow` to `src/services/review.rs` with fields: `id: i64`, `reason: String`, `status: String`, `created_at: String`, `summary_item_id: Option<String>`, `summary_text: Option<String>`, `source_count: i64`
- [X] T009 [P] [US3] Add `pub fn list_enriched(store: &Store, status_filter: Option<&str>, limit: usize) -> Result<Vec<EnrichedReviewRow>>` in `src/services/review.rs` — executes the enriched query from data-model.md (joins `review_queue → write_decisions → memory_items LEFT JOIN cluster_members subquery`) for both SQLite and Postgres backends
- [X] T010 [P] [US3] Add `pub fn approve_cluster(store: &Store, cluster_id: &str) -> Result<usize>` in `src/services/review.rs` — SELECTs pending `review_queue` rows whose `write_decision.resulting_item_id` is a `cluster_members.summary_item_id` for the given `cluster_id`, calls `resolve(store, review_id, Decision::Store)` for each, returns count approved
- [X] T011 [P] [US3] Add `pub fn skip_cluster(store: &Store, cluster_id: &str) -> Result<usize>` in `src/services/review.rs` — same as T010 but `Decision::Skip`
- [X] T012 [US3] Expose `approve_cluster` and `skip_cluster` as public API in `src/lib.rs` (re-export from `services::review`)
- [X] T013 [US3] Add `ApproveCluster { cluster_id: String }` and `SkipCluster { cluster_id: String }` variants to `ReviewAction` enum in `src/bin/pamet.rs`
- [X] T014 [US3] Wire `ReviewAction::ApproveCluster` and `ReviewAction::SkipCluster` dispatch in `src/bin/pamet.rs` Command::Review match arm — print `ok clusters_approved=N` or `ok clusters_skipped=N`
- [X] T015 [US3] Update `ReviewAction::List` in `src/bin/pamet.rs` to accept `--limit <N>` arg and call `list_enriched()` instead of `list()`, printing `review_id reason=<r> sources=<n> "<summary_text>"`
- [X] T016 [US3] Add `#[cfg(test)]` unit tests in `src/services/review.rs` for `approve_cluster` (pending → resolved_store) and `skip_cluster` (pending → resolved_skip) against an in-memory SQLite store
- [X] T017 [US3] Run `cargo build --release` from `~/Developer/pamet/` and validate quickstart.md scenario 3 manually against `/tmp/pamet_006_test.db`

**Checkpoint**: US3 complete — review queue is actionable. Run `pamet review approve-cluster` to clear the Nov-25 backlog.

---

## Phase 6: User Story 4 — pamet_outcome discipline (Priority: P3)

**Goal**: The agent reliably calls `pamet_outcome` after using recalled items, closing the
use_count / decay feedback loop.

**Independent Test**: After a session in which recalled items were used in a reply, verify
`use_count > 0` for those items in the DB.

### Implementation for User Story 4

- [X] T018 [P] [US4] Update `system_prompt_block()` in `~/.hermes/plugins/pamet/__init__.py` to append: "After using a recalled memory item in a response, call `pamet_outcome(event_id=..., item_id=..., used=true)`. After discarding a recalled item as irrelevant, call `pamet_outcome(used=false)`. This closes the retrieval feedback loop."
- [X] T019 [P] [US4] Add rule to `pamet-rust-development` skill via `skill_manage patch`: "Always call `pamet_outcome` for each recalled item before the session ends — `used=true` if the item appeared in a response, `used=false` if discarded. Missing outcomes leave `use_count` stale and break the decay/RRF feedback loop."

**Checkpoint**: US4 complete — outcome feedback loop documented and enforced at prompt level.

---

## Phase 7: User Story 5 — Prefetch budget configurability (Priority: P3)

**Goal**: The number of items prefetched per turn is set in `config.yaml`, not hardcoded.
Budget 0 disables prefetch entirely.

**Independent Test**: Set `plugins.pamet.prefetch_budget: 8` in config.yaml → prefetch
returns up to 8 items. Set `prefetch_budget: 0` → prefetch returns `""`.

### Implementation for User Story 5

- [X] T020 [US5] Add `self._prefetch_budget: int = _DEFAULT_BUDGET` field to `PametMemoryProvider.__init__()` in `~/.hermes/plugins/pamet/__init__.py`
- [X] T021 [US5] In `initialize()` in `~/.hermes/plugins/pamet/__init__.py`, read `self._config.get("prefetch_budget")`, validate it is an integer ≥ 0, and assign to `self._prefetch_budget` (keep `_DEFAULT_BUDGET = 5` as fallback for missing/invalid values)
- [X] T022 [US5] Update `prefetch()` in `~/.hermes/plugins/pamet/__init__.py` — replace the hardcoded `str(_DEFAULT_BUDGET)` with `str(self._prefetch_budget)`, and add early return `""` when `self._prefetch_budget == 0` (FR-009)
- [X] T023 [US5] Manually verify via quickstart.md scenario 5: set budget=8, confirm up to 8 items; set budget=0, confirm empty block

**Checkpoint**: US5 complete — prefetch budget is configurable without a code change.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [X] T024 `cargo test --lib` from `~/Developer/pamet/` — all tests green (must include new T016 review tests)
- [X] T025 [P] Run quickstart.md scenarios 1–5 end-to-end against a clean `/tmp/pamet_006_test.db` — document any deviations
- [X] T026 [P] Update `pamet-rust-development` skill with any new pitfalls discovered during this implementation (e.g. SQLite LEFT JOIN edge cases, cluster_id lookup pattern)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Empty — no blocking prerequisites beyond T001
- **US1–US5 (Phases 3–7)**: All depend only on T001 green; can proceed in parallel
- **Polish (Phase 8)**: Depends on all desired user stories complete

### User Story Dependencies

- **US1 (P1)**: Independent — Python plugin only, no Rust changes
- **US2 (P2)**: Independent — Python plugin only; write policy dedup is already in place
- **US3 (P2)**: Independent — Rust only (`src/services/review.rs`, `src/bin/pamet.rs`)
- **US4 (P3)**: Independent — skill update + system_prompt_block only; no DB changes
- **US5 (P3)**: Independent — Python plugin `initialize()` + `prefetch()` only

### Within Each User Story

- US3: T008 (struct) → T009/T010/T011 [P] (service fns) → T012 (lib.rs re-export) → T013/T014/T015 (CLI) → T016 (tests) → T017 (build+validate)
- US5: T020 (field) → T021 (init) → T022 (prefetch) → T023 (validate)
- All others are single-pass in task order

### Parallel Opportunities

- US1, US2, US4, US5 are all Python plugin changes but in different methods — parallelizable by function scope
- US3 T009, T010, T011 are independent service functions — parallelizable
- T018 (system_prompt_block) and T019 (skill update) have no deps on each other

---

## Parallel Example: US3

```text
# After T008 (struct defined):
T009: list_enriched() — join query
T010: approve_cluster() — resolve path
T011: skip_cluster() — resolve path

# All three can be written simultaneously (different functions, same file, non-conflicting)
```

---

## Implementation Strategy

### MVP First (US1 — 4 tasks)

1. T001: Verify baseline
2. T002–T005: US1 prefetch filter (Python only, no build needed)
3. **STOP and VALIDATE**: confirm `_filter_prefetch_query("who am i")` → `""` and prefetch skips it
4. This directly fixes the session bug that triggered this work

### Incremental Delivery

1. T001 → T002–T005 (US1, ~30 min) → test → ✅ fixes the original bug
2. T006–T007 (US2, ~20 min) → assistant facts retained
3. T008–T017 (US3, ~90 min, includes Rust build) → review queue actionable
4. T018–T019 (US4, ~15 min) → outcome loop documented
5. T020–T023 (US5, ~20 min) → budget configurable
6. T024–T026 (Polish, ~30 min)

### Single Developer Order

US1 → US2 → US5 (all Python, no rebuild) → US4 (skill only) → US3 (Rust, single build at end)

---

## Notes

- [P] = different functions/files, no incomplete-task deps
- Story labels map to spec.md user stories US1–US5
- No schema migrations — all data already exists
- Rust binary rebuild only needed for US3 (T017 build step)
- Python plugin changes take effect immediately on next Hermes session start
- FR-001/FR-002 from spec 005 are already in `target/release/pamet` — T001 verifies no regression, no reimplementation needed

---

## Phase 9: Convergence

| Finding | Severity | Source |
|---------|----------|--------|
| F1 — `cluster_id` missing from `review list` output | HIGH | FR-005 |
| F2 — assistant content low-value gate missing in plugin | MEDIUM | FR-004 / US2/AC2 |
| F3 — `cluster_members(summary_item_id)` index unverified | LOW | SC-003 |

- [X] T027 Add `cluster_id: Option<String>` field to `EnrichedReviewRow` struct in `src/services/review.rs`; update `list_enriched()` SQL to SELECT `cm2.cluster_id` via a second LEFT JOIN on `cluster_members`; include `cluster_id=<val>` in the `review list` println format in `src/bin/pamet.rs`; rebuild and confirm output includes cluster_id column per FR-005 (partial)
- [X] T028 Add a local low-value token gate in `sync_turn()` in `~/.hermes/plugins/pamet/__init__.py` before calling `_run(["remember", ...])` for `assistant_content`: skip the subprocess call if `len([t for t in assistant_content.split() if t]) < 3`, matching US2/AC2 ("same low-value gate applies") per FR-004 (partial)
- [X] T029 Verify or add a SQLite index on `cluster_members(summary_item_id)` in `src/storage/schema.rs` to ensure `list_enriched` GROUP BY subquery is fast on stores with 1,000+ cluster members; confirm with `EXPLAIN QUERY PLAN` on the enriched list query per SC-003 (partial)

---

## Phase 10: Convergence

| Finding | Severity | Source |
|---------|----------|--------|
| F1 — `resolve_cluster_items` collects duplicate review_ids via N:1 JOIN | HIGH | US3/AC2, SC-004 |

- [X] T030 Fix duplicate review_id collection in `resolve_cluster_items()` in `src/services/review.rs`: change the SELECT to use `SELECT DISTINCT rq.id` (or deduplicate the collected Vec<i64> in Rust with `.sort(); .dedup()` before the resolve loop) so that a cluster with N source items produces exactly 1 resolve call per unique review_queue row, not N; rebuild, re-run `cargo test --lib` (all 19 tests must pass), and manually verify `pamet review approve-cluster <id>` succeeds on the real DB cluster with 18K+ members per US3/AC2 and SC-004 (partial)
