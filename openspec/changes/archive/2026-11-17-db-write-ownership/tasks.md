# Tasks

Every group moves code with its SQL first (same semantics, same statement order), runs the existing tests, then converts to `query!`/`query_as!`/`query_scalar!` and adjusts call sites. Tests covering moved code move with it. Database tests and `query!` compilation without `.sqlx/` entries need PostgreSQL (`DATABASE_URL` or a container socket) and `SQLX_OFFLINE=false`, which overrides the default `true` in `.cargo/config.toml`; report what could not run.

## 1. Run states and runs (`df_db::runs`)

- [x] 1.1 Write a failing `df-core` test pinning `RunState::is_terminal` against `RunState::TERMINAL` and `NOT_COMMANDABLE` = `TERMINAL` + `finalizing`; add the constants; verify `cargo nextest run -p df-core run_state`.
- [x] 1.2 Create `crates/df-db/src/runs.rs` with `RunRow`, its record type, `get_run`, `list_unprovisioned_runs`, `lock_task`, `has_undelivered_message`, moved from `df-agent-channel/src/runs.rs` with `DbError` and checked queries; state lists bound from `RunState` constants instead of `terminal_states!`/`run_columns!`; verify `cargo build --workspace`.
- [x] 1.3 Move the transitions `start_queued_runs`, `set_run_state`, `set_idle`, `mark_started`, `mark_connected`, `begin_provisioning`, `end_run` (with `RunEnd`, `EndedRun`; `ChannelConfig` replaced by `&EventSinks`, `max_auto_resumes`, `resume_backoff`) and their tests; verify `cargo nextest run -p df-db runs`.
- [x] 1.4 Write failing `df-db` tests for `start_finalizing` (idle-only move blocked by an undelivered message; non-idle move from any commandable state), `set_last_persisted_seq` and `set_completion_summary`, then implement them from `session.rs` and `tools.rs:complete_task`; verify `cargo nextest run -p df-db runs`.
- [x] 1.5 TD-23: write failing `df-db` tests for `list_overdue_runs` (design decision 9), one per branch and each below the other thresholds: a `connecting` run with a fresh stop request before its connect timeout is returned; a `running` run whose stop request passed `STOP_TIMEOUT_SECONDS` while its budget remains is returned; a run past its connect timeout and a run past its hard stop are returned; a run within all limits and without a stop request is not; with `u64::MAX` connect and wall-clock budgets the query does not fail and still returns a stop-requested unconnected run. Implement it and switch `scheduler::enforce_timeouts` to it, keeping the Rust match as the final decision; the existing `scheduler` timeout and stop tests must pass unchanged. Verify `cargo nextest run -p df-db runs -p df-agent-channel scheduler`.
- [x] 1.6 TD-14: add `runs::persist_state` (one read of `state`, `last_persisted_seq`) with a test and use it once per batch in `Session::active`/`persist_batch`, keeping the `FOR UPDATE` read in the transaction; verify `cargo nextest run -p df-agent-channel session`.
- [x] 1.7 TD-65 in `df-db`: replace the literal state lists in `tasks.rs` (`queue_message`, `request_stop_tx`, `has_active_run`) and `egress.rs` with the `RunState` constants bound as `= ANY`/`<> ALL`; verify `cargo nextest run -p df-db tasks egress`.
- [x] 1.8 Switch `df-agent-channel` (`runs.rs`, `session.rs`, `tools.rs`, `scheduler`, `assign.rs`) to `df_db::runs`; keep `finish_run`/`teardown` in the channel; verify `cargo nextest run -p df-agent-channel` and no `runs` SQL left in `df-agent-channel/src` outside test code.

## 2. Task field updates (`df_db::task_updates`)

- [x] 2.1 Write failing `df-db` tests for `set_pr_draft_fields` (returns `pr_number`), `set_status_line` and `mark_diverged` (each publishes `TaskChanged` where the original does), then implement them in `crates/df-db/src/task_updates.rs` from `tools.rs:update_pull_request`, `tools.rs:report_progress`, `session.rs:apply_push`; verify `cargo nextest run -p df-db task_updates`.
- [x] 2.2 Write a failing test "a second PR does not overwrite the first", then move the conditional update of `pr/sync.rs:record_pr` into `task_updates::record_pr`; verify `cargo nextest run -p df-db task_updates`.
- [x] 2.3 Switch `tools.rs`, `session.rs`, `pr/sync.rs` (including the `pr_attached` read); verify `cargo nextest run -p df-agent-channel` and no `tasks` SQL left in `df-agent-channel/src` outside test code.

## 3. Transcript (`TranscriptWriter`, TD-19, TD-45)

- [x] 3.1 Move `PreparedEvent`, `prepare`, `allowed_from_runtime` and the blob helpers to `df_db::transcript` (split into `transcript/runtime.rs` beyond about 500 lines) with their tests; add the event kind constants and use them in `kind_name`; verify `cargo nextest run -p df-db transcript`.
- [x] 3.2 Make `mark_prompt_delivered` executor-generic with a `task_id` scope; write a failing test that a prompt of another task is not marked; update its callers; verify `cargo nextest run -p df-db tasks`.
- [x] 3.3 Write failing `df-db` tests for `TranscriptWriter`: `append_runtime` stores once per `(run, seq)`, rejects forge/factory activity and marks the queued message delivered (via `mark_prompt_delivered`) only for a newly stored event, so a duplicate `(run, seq)` carrying another prompt id marks nothing; `append_control` stores once per `(run, source_key)` and rejects runtime kinds; `finish` publishes one `transcript_event` with the newest id and none when nothing was stored. Implement with checked queries; verify `cargo nextest run -p df-db transcript`.
- [x] 3.4 Replace `append_control_event` with `append_control` + `finish` in `pr/sync.rs` (2 sites), `git_pushes.rs`, `artifacts.rs` and `crates/df-e2e/src/seed/demo/write.rs`; verify `cargo nextest run -p df-db -p df-agent-channel` and `cargo build -p df-e2e`.
- [x] 3.5 Move `turn_open`/`last_turn_event` to `df_db::transcript` using the kind constants (TD-45); switch `session.rs` (`BatchApply`, batch notify, turn checks) to `TranscriptWriter` and the constants; delete `df-agent-channel/src/transcript.rs` or keep only non-storage code; verify `cargo nextest run -p df-agent-channel session` and no `transcript_events`/`task_messages` SQL left in `df-agent-channel/src` outside test code.

## 4. Tool calls (`df_db::tool_calls`)

- [x] 4.1 Write failing `df-db` tests for `claim` (first claim true, repeat false) and `store_result` (keeps the first result), implement `crates/df-db/src/tool_calls.rs`, move the repeated-call read too, switch `tools.rs`; verify `cargo nextest run -p df-db tool_calls -p df-agent-channel tools`.

## 5. Remaining channel reads

- [x] 5.1 Move the queries of `retention.rs`, `summary.rs`, `pr.rs`, `assign.rs`, `tools/delegation.rs`, `tools/feedback.rs` and any `SELECT` left in `session.rs`/`tools.rs`/`pr/sync.rs` into the `df-db` module of the table they read, as checked queries with tests where none cover them; verify `cargo nextest run -p df-db -p df-agent-channel` and that `df-agent-channel/src` has no `sqlx::query` outside test code.

## 6. Checked queries in `df-db` (TD-42)

- [x] 6.1 Convert `users.rs`, `catalog.rs` (inline the `select_project!` variants as literal queries, remove the macro) and `task_views.rs` to `query!`/`query_as!`; verify their existing tests pass with `cargo nextest run -p df-db users catalog task_views`.

## 7. Guard and rules

- [x] 7.1 Add the `sqlx` entries to `disallowed-methods`, `disallowed-macros` and `disallowed-types` in the root `clippy.toml` (design decision 6) and the exemptions: crate level in `df-db`, `df-e2e`, `df-test-postgres`; module level in `df-web/src/auth/store.rs`; `cfg_attr(test, expect(...))` at crate roots whose tests use `sqlx`; `#![expect]` in integration test files. Verify `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [x] 7.2 Confirm the guard catches violations: add a temporary `sqlx::query("SELECT 1")`, a `sqlx::query!("SELECT 1")` and a direct `pool.execute("SELECT 1")` (`sqlx::Executor`) call in `df-agent-channel` production code, check that clippy reports all three, then remove them. Report the result (in particular whether `disallowed-macros` catches `query!`); if it does not, add the gap to design.md and propose an alternative. Result: clippy reported all three probes (`sqlx::query(...)` as a disallowed method, `sqlx::query!` as a disallowed macro, `pool.execute("SELECT 1")` as `sqlx::Executor::execute`); no gap.
- [x] 7.3 Document the rule in `AGENTS.md`: database access only via `df-db`, enforced by clippy, `web_sessions` and test fixtures as exceptions. No published docs change: no user- or operator-visible behavior changes.
- [x] 7.4 Add a section "Database access" to the `rust` skill (`.agents/skills/rust/SKILL.md`) and extend its `description` with "when writing SQL or a `sqlx` query, or adding a table or a query to one". Content: all production queries live in `df-db`, in the module of the table they touch (`runs`, `task_updates`, `transcript`, `tool_calls`, …); other crates call those functions and never use `sqlx::query*`; a new query is a checked `query!`/`query_as!` in a `df-db` function (`impl PgExecutor` for one statement, `&mut Transaction` for several) with a `df-db` test written first; `.sqlx/` is regenerated after query changes; state lists come from `RunState` constants; transcript writes go through `TranscriptWriter`, and transactions that also lock a run call `lock_run_and_task` first; no storage traits; the clippy exemptions and how to add one for test code. Verify the section names the clippy entries as configured in 7.1.
- [x] 7.5 Delete TD-14, TD-19, TD-23, TD-33, TD-42, TD-45 and TD-65 from `docs/tech_debt.md` (headings, table rows, origin notes that only cover them); verify `grep -E "TD-(14|19|23|33|42|45|65)\b" docs/tech_debt.md` finds nothing.

## 8. Integration

- [x] 8.1 Regenerate the whole `.sqlx/` directory with the pinned sqlx-cli (`docs/development.md`, "Updating `sqlx` Offline Metadata") (TD-33); verify `SQLX_OFFLINE=true cargo build --workspace` passes and all metadata files share one format.
- [x] 8.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length`, `cargo xtask check-crates` and `bazel --bazelrc=.bazelrc.agents test //crates/df-core/... //crates/df-db/... //crates/df-agent-channel/... //crates/df-web/...`; all pass.
- [x] 8.3 Run `just e2e-local` (or report that it could not run) to confirm unchanged end-to-end behavior.

## Workflow follow-up

- At archive: propose the backlog row "Convert the remaining runtime-checked queries of `df-db` (`model_providers`, `task_events`, `requests`, `artifacts`)" for `product/iterations/roadmap.md`; the egress paging row stays as it is.
- Close #123 with the PR (`Closes #123`).
