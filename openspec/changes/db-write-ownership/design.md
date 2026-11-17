# Design

## Context

See proposal.md for motivation. Production writes outside `df-db`:

| Table | Writers in `df-agent-channel` | Writers in `df-db` |
|---|---|---|
| `runs` | `runs.rs`: `start_queued_runs` (INSERT), `set_run_state_tx`, `set_idle_tx`, `end_run_tx`, `mark_started`, `mark_connected`, `begin_provisioning`; `session.rs`: `start_finalizing` (2 UPDATEs), `last_persisted_seq`; `tools.rs`: `complete_task` (`completion_summary`) | `task_events.rs`, `tasks.rs` (`stop_run`) |
| `tasks` | `runs.rs` (`end_run_tx`: `harness_session_id`, `diverged`); `session.rs` (`apply_push`: `diverged`); `pr/sync.rs` (`record_pr`); `tools.rs` (`update_pull_request`: `pr_title`/`pr_body`, `report_progress`: `status_line`) | `tasks.rs`, `task_events.rs`, `push_checks.rs`, `volumes.rs`, `webhooks.rs` |
| `task_messages` | `transcript.rs` (`insert`: delivery confirmation, a copy of `df_db::mark_prompt_delivered` with a `task_id` scope) | `tasks.rs`, `task_events.rs` |
| `transcript_events` | `transcript.rs` (`insert`, keyed `(run, seq)`) | `transcript.rs` (`append_control_event`, keyed `(run, source_key)`) |
| `tool_calls` | `tools.rs` (claim, `store_result`) | none |

Production reads outside `df-db`: the files above plus `retention.rs`, `summary.rs`, `pr.rs`, `assign.rs`, `tools/delegation.rs`, `tools/feedback.rs` (about 54 query calls in 11 files). `dark-factory`, `df-gateway` and `df-web` (except `auth/store.rs`) already use `df-db` only.

#123 entries in this code:

| Entry | Where | Fix |
|---|---|---|
| TD-19 | channel SQL is runtime-checked; `transcript::insert` copies `mark_prompt_delivered` | decisions 2, 8 |
| TD-65 | state lists as literals: `terminal_states!` (`runs.rs`), `session.rs`, `df-db/src/tasks.rs`, `egress.rs` | decision 7 |
| TD-45 | `session.rs::turn_open` reads `transcript_events`; kind names as string literals | decision 8 |
| TD-14 | `Session::active` and `persist_batch` read the run several times per batch | decision 9 |
| TD-23 | `list_active_runs` loads every non-terminal run each scheduler tick | decision 9 |
| TD-42 | `df-db` `users`, `catalog` (`select_project!` string macro), `task_views` are runtime-checked | decision 2 |
| TD-33 | `.sqlx/` from mixed sqlx-cli versions | decision 10 |

`ChannelError` wraps `DbError` (`#[from]`). Constraints: `df-db/src/tasks.rs` has 1,231 lines (limit 1,500), `df-agent-channel/src/tools.rs` 1,476. The root `clippy.toml` is the only clippy configuration, for cargo and Bazel (`.bazelrc`, `--config=lint`).

## Goals / Non-Goals

**Goals:**
- Every production query of `df-agent-channel` lives in `df-db`; the semantics of each statement and the order of statements in each transaction stay as they are.
- Moved and new queries are compile-time checked.
- Clippy keeps it that way.

**Non-Goals:**
- Merging the run state machine into `df_core::task_transition` or changing allowed transitions. Rejected: this change moves code, it does not redesign it.
- Converting the other runtime-checked queries of `df-db` (proposal, Out of Scope).
- A trait for the storage layer. Rejected (decision 1).

## Decisions

### 1. Plain modules and functions in `df-db`, no traits

One implementation, and tests run against real PostgreSQL (`df_test_postgres`). A trait would be an interface with a single implementation.

Alternative: a `Repository` trait per table, faked in channel tests. Rejected: the channel tests already use a database, and fakes would drift from the SQL.

### 2. Compile-time checked queries, executor-generic where possible

Moved and converted queries use `query!`/`query_as!`/`query_scalar!`. A function with a single statement takes `impl PgExecutor<'_>` (callable with a pool or a transaction); a function with several statements takes `&mut Transaction<'_, Postgres>`. Row types with domain fields (`RunRow`) keep a private record type plus `TryFrom`, filled by `query_as!`.

`catalog.rs`'s `select_project!` string macro cannot feed `query_as!`; its variants are inlined as literal queries (TD-42).

### 3. `df_db::runs`: one function per transition

Moves `RunRow`, its reads (`get_run`, `list_active_runs`, `list_unprovisioned_runs`), `lock_task`, `has_undelivered_message` and every run write. `session.rs` statements become `runs::start_finalizing(tx, &RunTaskLock, only_idle)`, `runs::set_last_persisted_seq`; `tools.rs` uses `runs::set_completion_summary`.

`end_run_tx` depends on `ChannelConfig`; the `df-db` version takes `&EventSinks`, `max_auto_resumes`, `resume_backoff`. `RunEnd`/`EndedRun` move with it. `finish_run` and `teardown` stay in the channel: they call Kubernetes.

### 4. `TranscriptWriter` in `df_db::transcript`

```rust
pub struct TranscriptWriter<'l> { lock: &'l RunTaskLock, newest: Option<i64> }

impl TranscriptWriter<'_> {
    pub fn new(lock: &RunTaskLock) -> TranscriptWriter<'_>;
    /// Keyed (run, seq); rejects forge/factory activity; confirms prompt delivery.
    pub async fn append_runtime(&mut self, tx, event: &PreparedEvent) -> Result<Option<i64>, DbError>;
    /// Keyed (run, source_key); only forge/factory activity.
    pub async fn append_control(&mut self, tx, source_key: &str, event: &Event) -> Result<Option<i64>, DbError>;
    /// Publishes `transcript_event` with the newest id, if any was stored.
    pub async fn finish(self, tx) -> Result<(), DbError>;
}
pub async fn prepare(blobs: &PgBlobStore, seq: u64, event: Event) -> Result<PreparedEvent, DbError>;
```

- `prepare` stays a separate step before the transaction; only its location moves. Alternative: inside `append_runtime`. Rejected: blob writes would move into the event transaction.
- `allowed_from_runtime` moves along and is checked in `append_runtime`; the session keeps its early check by calling the `df-db` function.
- `append_control_event` is replaced; its callers (`pr/sync.rs` ×2, `git_pushes.rs`, `artifacts.rs`, the e2e seed) create a writer and call `finish`. Control-plane callers write one event per transaction, so one notify per transaction equals today's behavior.
- `TranscriptWriter` is `#[must_use]`. A forgotten `finish` is not a compile error; each caller's notify test covers it.

### 5. Task field updates in `df_db::task_updates`, tool calls in `df_db::tool_calls`

`tasks.rs` is close to the file limit, so channel task writes go to a new module: `set_pr_draft_fields`, `set_status_line`, `mark_diverged`, `record_pr` (the conditional `pr_number IS NULL` update). Each publishes `TaskChanged` where the original does.

`tool_calls::claim(executor, call_id, run_id, tool_name, arguments) -> bool` and `store_result(executor, call_id, result: Value)`; `df-db` does not depend on the channel's `ToolCallResult`.

The remaining channel reads go to the `df-db` module of the table they read (new functions in existing modules, or a new module when the file limit requires it).

### 6. Guard: `clippy.toml`

```toml
disallowed-methods = [
    { path = "std::env::temp_dir", reason = "use `tempfile`; never write to fixed paths" },
    { path = "sqlx::query", reason = "database access only via `df-db`" },
    # query_as, query_scalar, query_with, query_as_with, query_scalar_with, raw_sql: same reason
    { path = "sqlx::Executor::execute", reason = "database access only via `df-db`" },
    # Executor::{execute_many, fetch, fetch_many, fetch_all, fetch_one, fetch_optional, prepare,
    # prepare_with, describe}: same reason; they run SQL given as a string (`pool.execute("…")`)
]
disallowed-macros = [
    { path = "sqlx::query", reason = "database access only via `df-db`" },
    # query_as, query_scalar: same reason
]
disallowed-types = [
    { path = "sqlx::QueryBuilder", reason = "database access only via `df-db`" },
]
```

Exemptions, each `#[expect(..., reason = "...")]`:
- `df-db`, `df-e2e`, `df-test-postgres`: crate level.
- `df-web/src/auth/store.rs`: module level, not the whole crate, so a new query elsewhere in `df-web` is caught.
- Test code in other crates: `#![cfg_attr(test, expect(clippy::disallowed_methods, reason = "test fixtures"))]` at the crate root where tests use `sqlx`; each integration test file (`tests/*.rs`) carries its own `#![expect]`.

Passing `PgPool` or `Transaction` through other crates stays allowed. Clippy runs in pre-push, CI and Bazel's lint config, so no new tool is needed. A fixture test confirms that `disallowed-macros` catches `query!` (a `macro_rules!` wrapper around a proc macro) and that the `Executor` entries catch a direct `pool.execute("…")`, the path `df-db/src/pool.rs` uses for role setup.

Alternative: `cargo xtask check-db-writes`, a text match for `INSERT`/`UPDATE`/`DELETE`. Rejected: covers only writes, matches text instead of resolved paths, and is one more tool to maintain.

### 7. Run-state lists as `RunState` constants (TD-65)

`df_core::RunState` gets `TERMINAL` (`succeeded`, `failed`, `cancelled`) and `NOT_COMMANDABLE` (`TERMINAL` plus `finalizing`), next to `is_terminal`. Queries bind them as `state <> ALL($n)` / `state = ANY($n)` with a `&[&str]`. This replaces `terminal_states!`, `run_columns!` (which `query!` cannot take anyway, being `concat!`) and the literal lists in `df-db/src/tasks.rs` (`queue_message`, `request_stop_tx`, `has_active_run`) and `egress.rs`. A unit test pins `is_terminal` against `TERMINAL`.

### 8. Prompt delivery, turn state and kind names (TD-19, TD-45)

- `mark_prompt_delivered` becomes executor-generic and gets the `task_id` scope; `append_runtime` calls it instead of its own `UPDATE`.
- `turn_open` / `last_turn_event` move to `df_db::transcript`.
- Event kind names (`turn_started`, `turn_ended`, …) become constants in `df_db::transcript`, used by `kind_name`, the session's `match` and the SQL bind.

### 9. Query shapes (TD-14, TD-23)

- TD-14: one `SELECT state, last_persisted_seq FROM runs` before preparing blobs serves both checks of `Session::active` and `persist_batch`; the `FOR UPDATE` read inside the transaction stays.
- TD-23: `runs::list_overdue_runs(now, &RunsConfig)` returns the non-terminal runs that one of the four branches of `enforce_timeouts` can end, so the SQL predicate is an OR of all four:
  1. a stop request on a run that is not connected yet (`pending`, `provisioning`, `connecting` with `stop_requested_at` set), at any age;
  2. a stop request older than `STOP_TIMEOUT_SECONDS`, in any state;
  3. the connect timeout (not connected yet, `started_at` + `connect_timeout_seconds`);
  4. the hard stop (connected states, `COALESCE(connected_at, started_at)` + `max_wall_clock_seconds` + 2 × `cancel_grace_seconds`).

  The filter may return more runs than get ended, never fewer: `enforce_timeouts` keeps its Rust match as the final decision. The intervals come from the same constants and `RunsConfig` fields the Rust check uses, bound as parameters.

### 10. `.sqlx/` metadata (TD-33)

Regenerate the whole `.sqlx/` directory with the sqlx-cli version pinned in `docs/development.md` ("Updating `sqlx` Offline Metadata"), once at the end, and commit it. Intermediate steps run against a live `DATABASE_URL`.

## Risks / Trade-offs

- [Statement order changes during the move and breaks the lock order] → Move each function with its SQL first, run the existing channel tests (`session/tests.rs`, `scheduler/tests.rs`, `pr/sync/tests.rs`, tools tests), then convert to `query!`.
- [`query!` infers nullability differently than the hand-written record types] → Use `column AS "column!"` / `"column?"` overrides where inference is wrong; the compile fails, it does not drift silently.
- [`.sqlx/` generation and database tests need PostgreSQL; a sandbox without a container socket can do neither] → CI runs the tests; the implementer reports what was not run and generates metadata where a database is available.
- [A crate-level `#![expect(clippy::disallowed_methods)]` in `df-db` also lifts the `temp_dir` ban there] → Accepted: `df-db` tests use `tempfile` by convention; review catches the rest.
- [`df-db` gains channel concepts (`RunEnd`, `PreparedEvent`)] → They describe stored data, not the protocol; `df-db` already depends on `df-proto` and `df-core`.

## Migration Plan

Code-only refactor, no schema or configuration change. Deploy as usual; rollback is a revert.
