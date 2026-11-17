# Proposal

## Why

`df-agent-channel` reads and writes `runs`, `tasks`, `task_messages`, `transcript_events` and `tool_calls` with its own SQL, next to `df-db`, which writes the same tables. A table's invariants are therefore split across two crates:

- the run state machine: allowed transitions are spread over `df-agent-channel/src/runs.rs`, `session.rs` and `df-db`;
- the lock order (run row, then task row): enforced by type only for control-plane transcript writes;
- the notify contract: one `pg_notify` per event on one transcript path, one per batch on the other.

Most of that SQL, and part of `df-db`'s own, is runtime-checked `sqlx::query(...)`: a column rename compiles and fails only in container-backed tests or at runtime ([#123](https://github.com/prskr/dark-factory/issues/123)). A new writer has to find every place and copy the right one. Pre-MVP is the cheapest time to give the database one owner.

## What Changes

- `df-db` becomes the only crate with database access in production code: every query (reads and writes) of `df-agent-channel` moves into `df-db`. Exception: `df-web/src/auth/store.rs` (`web_sessions`).
- New `df_db::runs`: the `RunRow` type, its reads and one function per run state transition (start queued runs, begin provisioning, mark started, mark connected, set state, set idle, start finalizing, end run, persisted seq, completion summary).
- New `TranscriptWriter` in `df_db::transcript`: one entry point for runtime events (`(run, seq)`) and control-plane events (`(run, source_key)`). Both require `RunTaskLock`; both publish one notify per transaction with the newest id. Blob extraction (`prepare`), the runtime kind filter, `turn_open` and the event kind names move with it; the delivery confirmation reuses `df_db::mark_prompt_delivered`.
- Task field updates of the channel (PR fields, `diverged`, `status_line`, `pr_title`/`pr_body`) and `tool_calls` claim/result become `df-db` functions; the remaining channel reads (`retention`, `summary`, `pr`, `assign`, `tools/delegation`, `tools/feedback`) too.
- Moved and new queries are compile-time checked (`query!`/`query_as!`, `.sqlx/` metadata). `df-db`'s runtime-checked queries in `users`, `catalog` and `task_views` are converted as well.
- Fixes from #123 in the moved code: one `RunState` constant per state list bound with `= ANY($n)` (TD-65), one run read per batch instead of several (TD-14), overdue runs filtered in SQL (TD-23), `.sqlx/` regenerated with one sqlx-cli version (TD-33). Covers TD-14, TD-19, TD-23, TD-33, TD-42, TD-45, TD-65; their entries are removed from `docs/tech_debt.md` and the change closes #123 (TD-81 was fixed in `e68942d`).
- Guard: `clippy.toml` bans the `sqlx` query functions, macros and `QueryBuilder` outside `df-db` (`disallowed-methods`, `disallowed-macros`, `disallowed-types`). Exemptions by `#[expect]`: `df-db`, `df-web/src/auth/store.rs`, `df-e2e`, `df-test-postgres` and test code.
- `AGENTS.md` and the `rust` skill state the rule: database access only via `df-db`.
- No schema change and no change in observable behavior. TD-14 and TD-23 change query shapes, not results.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. Pure refactor: `skip_specs: true` in `.openspec.yaml`.

## Out of Scope

- SQL paging of the egress rules: stays in the backlog of `product/iterations/roadmap.md` as a separate change.
- The other runtime-checked queries in `df-db` (`model_providers`, `task_events`, `requests`, `artifacts`, about 80): new backlog row in `product/iterations/roadmap.md`, proposed at archive.
- `migrate.rs` stays runtime-checked: it builds statements from role names at runtime. Rejected.
- `web_sessions` stays in `df-web`: a session-store adapter without business invariants that picks its SQL at runtime. Rejected.
- Test fixtures (`testing.rs`, `testkit.rs`, `df-e2e` seed, test modules) keep their direct SQL. Rejected.

## Impact

- Code: `crates/df-agent-channel` (`runs.rs`, `session.rs`, `tools.rs`, `tools/delegation.rs`, `tools/feedback.rs`, `transcript.rs`, `pr.rs`, `pr/sync.rs`, `assign.rs`, `retention.rs`, `summary.rs`, `scheduler`) loses all SQL and calls `df-db`; `crates/df-db` gains `runs.rs`, `tool_calls.rs`, `task_updates.rs`, `TranscriptWriter` and checked queries in `users.rs`, `catalog.rs`, `task_views.rs`, `tasks.rs`, `egress.rs`; `df-core` gains the `RunState` lists.
- Tooling: root `clippy.toml`; `.sqlx/` regenerated.
- APIs: internal Rust only. `df_db` functions take plain values (`max_auto_resumes`, `resume_backoff`, `EventSinks`) instead of `ChannelConfig`.
- Contributor docs: `AGENTS.md`, the `rust` skill, `docs/tech_debt.md`. No published docs change.
- Dependencies: none added.
