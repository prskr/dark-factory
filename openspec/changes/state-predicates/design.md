# Design

## Context

See proposal.md. Copies of each predicate, measured on today's code (before `db-write-ownership`):

| Predicate | Copies |
|---|---|
| message undelivered (`delivered_at IS NULL AND discarded_at IS NULL`) | `tasks.rs:666`, `:806`; `task_events.rs:399`, `:478`; `df-agent-channel/src/runs.rs:393`; **`push_checks.rs:626` without `discarded_at`** |
| message held back (`delivered_at IS NULL AND forwarded_at IS NULL AND discarded_at IS NULL`) | `tasks.rs:827`, `:862`, `:916` |
| task done (`completed`, `failed`, `cancelled`) | `tasks.rs:488`, `webhooks.rs:54`, index `tasks_open_branch` (`20261112000001`), `StatusGroup::Done` |
| task waits for resume (`queued`, `rate_limited`) | `df-agent-channel/src/runs.rs:198` (SQL), `TaskStatus::waits_for_resume` |
| task closed (`completed`, `cancelled`; `failed` excluded on purpose) | `volumes.rs:due_for_retention` |
| run terminal / not commandable | `task_events.rs:364`, `:367` (rest after TD-65), index `runs_one_active_per_task_idx` |
| request pending | `df-web/src/requests.rs:149` (Rust), index `agent_requests_pending_idx` |

## Goals / Non-Goals

**Goals:**
- Each multi-value predicate is defined once: as an enum constant (bound in SQL) or as a generated column (multi-column predicates).
- The `push.rejected` feedback ignores discarded messages.

**Non-Goals:**
- See proposal, Out of Scope.

## Decisions

### 1. Generated columns for the message predicates

`task_messages` gets `undelivered boolean GENERATED ALWAYS AS (delivered_at IS NULL AND discarded_at IS NULL) STORED` and `held_back ... (delivered_at IS NULL AND forwarded_at IS NULL AND discarded_at IS NULL) STORED`. Queries write `WHERE undelivered` / `WHERE held_back`. `task_messages_task_undelivered_idx` is recreated with `WHERE undelivered`. Today it uses `WHERE delivered_at IS NULL` and still covers discarded rows.

Alternatives:
- A SQL constant in Rust concatenated into queries. Rejected: `query!` takes a string literal; `db-write-ownership` removes the `concat!` macros for that reason.
- A view. Rejected: `sqlx` infers nullability from views less well, and the queries also update the table.

### 2. Enum constants bound with `= ANY`

`TaskStatus::DONE` and `TaskStatus::WAITS_FOR_RESUME` follow the `RunState::TERMINAL` pattern of `db-write-ownership`: a `&'static [Self]` constant, a method that checks membership, and SQL that binds the `as_str` list as `text[]`. `TaskStatus::group()` names every variant (an exhaustive match, so a new status fails to compile until it is grouped) and a test pins `DONE` to its `Done` arm; `StatusSet::Done` and `waits_for_resume` derive from the constants. A unit test pins each constant against the method that it replaces or backs.

### 3. Partial indexes pinned by a test

An index predicate can't bind a parameter. The index keeps its literal list, and a `df-db` test reads `indexdef` from `pg_indexes` for `runs_one_active_per_task_idx`, `tasks_open_branch` and `agent_requests_pending_idx`. It checks that the listed values equal the constant (order ignored). A new constant member then fails the test until a migration updates the index.

### 4. Single values stay literal

`state = 'pending'` or `status = 'cancelled'` stays as it is. A list can lose or gain a member; a single value can't drift that way, and the `CHECK` constraint plus the query's test catch a typo. The rule goes into the `rust` skill.

## Risks / Trade-offs

- [The generated columns cost a few bytes per message row and a rewrite of `task_messages` during the migration.] → Acceptable before MVP. The table is small, and it saves a predicate in many queries.
- [Bug fix: the agent gets `push.rejected` feedback in a case where it got none before.] → That's the behavior `push-checks` specifies. A failing test comes first (task 1.1).
