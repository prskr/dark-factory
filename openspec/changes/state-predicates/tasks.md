# Tasks

Requires `db-write-ownership`. Before starting, re-measure the table in design.md with `rg`; the line numbers change with `db-write-ownership`. Database tests need PostgreSQL; report the ones that could not run. No user- or operator-visible behavior changes apart from the bug fix, which restores specified behavior; `docs/content/security/git-proxy.md` states the feedback rule.

## 1. Message predicates

- [x] 1.1 Write a failing `df-db` test: a discarded `push.rejected` message does not suppress the feedback for the next rejected push. Verify that it fails with `cargo nextest run -p df-db push_checks`.
- [x] 1.2 Add the migration with `task_messages.undelivered` and `held_back` (design decision 1) and recreate `task_messages_task_undelivered_idx` with `WHERE undelivered`. Switch every copy in the design table to the columns. Regenerate `.sqlx/`. Verify 1.1 passes, `cargo nextest run -p df-db`, and that `rg "delivered_at IS NULL" crates/df-db/src` finds only the column definition's tests.

## 2. Enum constants

- [x] 2.1 Write failing `df-core` tests for `TaskStatus::DONE` (equals the `StatusGroup::Done` members) and `WAITS_FOR_RESUME` (equals `waits_for_resume`), then add the constants, derive `StatusSet::Done` and `waits_for_resume` from them, and keep `TaskStatus::group()` an exhaustive match that the `DONE` test pins. Verify `cargo nextest run -p df-core domain`.
- [x] 2.2 Bind the constants in `tasks.rs` (`open_branch_holder`), the resume query of `df_db::runs::start_queued_runs`, `webhooks.rs:target_tasks`, `volumes.rs:due_for_retention` (`TaskStatus::CLOSED`: `completed`, `cancelled`; `failed` excluded), and bind `RunState::TERMINAL`/`NOT_COMMANDABLE` in `task_events.rs`. Verify `cargo nextest run -p df-db -p df-agent-channel webhooks` and that `rg "IN \('" crates/*/src` finds no multi-value state list outside migrations and tests.
- [x] 2.3 Add `RequestState::is_pending` with a test and use it in `df-web/src/requests.rs`. Verify `cargo nextest run -p df-db requests -p df-web requests`.

## 3. Index pins and rule

- [x] 3.1 Write the `df-db` test of design decision 3 for `runs_one_active_per_task_idx`, `tasks_open_branch` and `agent_requests_pending_idx`. Verify `cargo nextest run -p df-db index_predicates`. Then check that it fails when a member is temporarily removed from one constant.
- [x] 3.2 Extend the "Database access" section of the `rust` skill: multi-value state lists come from enum constants bound with `= ANY`/`<> ALL`, multi-column predicates are generated columns, single values may stay literal (design decision 4), and partial-index predicates are pinned by the test of 3.1.

## 4. Integration

- [x] 4.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `SQLX_OFFLINE=true cargo build --workspace` and `bazel --bazelrc=.bazelrc.agents test //crates/df-core/... //crates/df-db/... //crates/df-agent-channel/... //crates/df-web/...`; all pass.
