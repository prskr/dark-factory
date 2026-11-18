# Proposal

## Why

The rules that ask "is this run over", "is this task done" or "is this message still waiting" are written out in many places: as SQL literal lists, in partial indexes and in Rust `matches!`. Each copy can drift from the others, and one already has: `undelivered_feedback` (`df-db/src/push_checks.rs:619`) checks `delivered_at IS NULL` but not `discarded_at IS NULL`. A `push.rejected` message that was discarded (cancel or done, `Effect::DiscardMessages`) therefore still counts as waiting. When the task is later resumed by a new message, every rejected push is refused without feedback, so the agent never learns why.

`db-write-ownership` (TD-65) covers the run-state lists in `tasks.rs` and `egress.rs`. This change covers the remaining run-state lists and the predicates of the other state columns.

## What Changes

- **Message delivery:** two stored generated columns on `task_messages` replace the hand-written predicates:
  - `undelivered` (`delivered_at IS NULL AND discarded_at IS NULL`) replaces 5 copies (`tasks.rs`, `task_events.rs`, `runs.rs`), plus the copy in `push_checks.rs`, which fixes the bug above;
  - `held_back` (`delivered_at IS NULL AND forwarded_at IS NULL AND discarded_at IS NULL`, the messages a human may still edit or withdraw) replaces 3 copies in `tasks.rs`.
- **Task status sets:** `TaskStatus` gets `DONE` (`completed`, `failed`, `cancelled`, the members of `StatusGroup::Done`), `WAITS_FOR_RESUME` (`queued`, `rate_limited`) and `CLOSED` (`completed`, `cancelled`; `failed` excluded on purpose: the retention of task volumes). The SQL lists in `tasks.rs` (`open_branch_holder`, the resume query of `start_queued_runs`), in `webhooks.rs` and in `volumes.rs` (`due_for_retention`) bind these constants with `= ANY($n)` / `<> ALL($n)`, like the run-state constants of `db-write-ownership`.
- **Run states:** the remaining literal lists in `df-db/src/task_events.rs` (the `ActiveRun` snapshot query) bind `RunState::TERMINAL` and `NOT_COMMANDABLE`.
- **Partial indexes:** a `df-db` test reads the predicates of `runs_one_active_per_task_idx`, `tasks_open_branch` and the `task_messages` indexes from `pg_indexes` and compares them with the constants. SQL index predicates can't bind parameters, so they stay literal; the test makes drift fail.
- **Requests:** `RequestState::is_pending`. `df-web/src/requests.rs:149` uses it.
- **Rule:** the `rust` skill's "Database access" section (added by `db-write-ownership`) says that every multi-value state list comes from an enum constant. A single value may stay a literal: the column's `CHECK` constraint lists the allowed values, and the query's test catches a typo.
- The only change in behavior is the `push.rejected` bug fix.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. The bug fix restores the behavior `push-checks` already specifies. `skip_specs: true` in `.openspec.yaml`.

## Out of Scope

- The run state machine itself (`set_run_state_tx`, the transitions): change `run-state-machine`.
- Single-value comparisons (`status == Completed`, `state = 'pending'`). Rejected: one value can't drift from a list.
- `complete_task` closing the pending requests outside `apply_task_event`. Rejected: `complete_task` doesn't change the task status (the run end does). Closing the requests is the agent's action and is audited as such.
- `mark_stale_forwarding` (`git_pushes.rs:245`) setting `forwarding → unknown` without `advances_to`. Rejected: `advances_to` governs the verdicts the gateway reports; going stale is a separate timeout transition, which no report may cause.
- `title_source`: `tasks.rs:1201` duplicates `TitleSource::may_replace` deliberately (comment at `tasks.rs:959`). Rejected.

## Impact

- Code: `df-core` (`domain.rs`), `df-db` (`tasks.rs`, `volumes.rs`, `task_events.rs`, `push_checks.rs`, `requests.rs`, the queries that read `task_messages`, a new migration), `df-agent-channel` (`webhooks.rs`; `runs.rs` and `transcript.rs` already sit in `df-db` after `db-write-ownership`), `df-web` (`requests.rs`).
- Schema: one migration adds a stored generated column and may switch existing partial indexes to it. No data migration.
- `.sqlx/` metadata regenerated for the changed queries.
- Docs: `rust` skill; `docs/content/security/git-proxy.md` gets a "Feedback messages" paragraph that states the rule the bug fix restores.
- Order: after `db-write-ownership` (it moves the queries this change edits and adds the `RunState` constants). Independent of `run-state-machine`.
