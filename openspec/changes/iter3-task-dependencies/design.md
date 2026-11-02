# Design

## Context

See proposal.md (Why). The current state:

- **Tasks** (`crates/df-db/src/tasks.rs`):
  - `create_task` allocates the key under a row lock on the project and queues the `task.assigned` prompt in one transaction.
  - `tasks.created_by` is a nullable reference to `users`.
  - The status is a `TEXT` column with a `CHECK` list.
- **Messages**:
  - `task_messages.author` is already a JSONB `Principal` (`Human`, `Agent { persona_id, display_name, run_id, task_id }`, `System { reason }`). An agent-authored message therefore needs no schema change.
  - `queue_message_tx` sets the status with one `CASE` (`running` with an active run, otherwise `queued`).
- **Run end**: `df_agent_channel::runs::end_run_tx` maps the outcome to the task status (`completed` → `in_review`, `failed` → `failed`, …) in one transaction.
- **Final transitions**:
  - `mark_task_done` sets `completed`; the merged-PR path of `iter3-webhooks` reuses it.
  - `iter2-cancel` sets `cancelled`.
- **Scheduler**: `start_queued_runs` only picks `queued` tasks, so a new status is skipped automatically.
- **Factory tools**: executed in `crates/df-agent-channel/src/tools.rs` (1,244 lines), declared in `contract/mcp/factory-tools.v2.json`. Every call is stored by `call_id` for exactly-once execution.
- **Fake agent**: `crates/fake-acp-agent` already scripts arbitrary tool calls (`tool_call` steps).

## Goals / Non-Goals

**Goals:**
- One place decides what a task event means for dependents and the parent. Run end, mark done and cancel only call it.
- Releasing a dependent can't be lost to a race between creating it and its target completing.
- The handover works without webhooks and without any forge call.

**Non-Goals:**
- Limits on delegation (depth, children, relayed messages): `iter3-task-tree`.
- `depends_on` in the composer: backlog ("Dependencies set by humans").
- A "start now" override: backlog ("Start now").
- Notifying dependents or their parents when a PR is closed without merge: rejected. The PR's own task is told (`iter3-webhooks`), and its parent wakes if that task then fails or is cancelled.
- Storing which agent read which child-ended message: rejected. Messages are delivered once, like any prompt.

## Decisions

### D1: Schema

One migration:

- **`tasks.parent_id`**: `UUID NULL REFERENCES tasks(id)`, indexed. A `CHECK` can't express "same project"; `create_task` sets it only from the calling run's task, which is always in the same project.
- **`tasks.created_by`**: becomes `JSONB NOT NULL` holding a `Principal`, the same shape as `task_messages.author`.
  - Migration: existing user ids become `{"kind":"human","user_id":…,"display_name":<users.display_name>}`, and `NULL` becomes `{"kind":"system","reason":"migration"}`.
  - Alternative: two nullable FK columns (`created_by_user_id`, `created_by_task_id`). That's a second principal representation next to messages and audit.
- **`tasks.first_completed_at`, `tasks.first_done_at`**: `TIMESTAMPTZ NULL`, each set once, the first time the event happens.
  - Alternative: `satisfied_at` per dependency row. That needs the target's history at creation time anyway, and with both columns satisfaction is a pure function of the target row.
- **`task_dependencies`**:
  - columns `task_id`, `depends_on_task_id`, `kind TEXT CHECK (kind IN ('completion','done'))`;
  - primary key `(task_id, depends_on_task_id)`;
  - an index on `depends_on_task_id`.
  - A dependency is satisfied when `target.first_done_at IS NOT NULL OR (kind = 'completion' AND target.first_completed_at IS NOT NULL)`.
- **`tasks.status`**: `CHECK` gains `awaiting_dependencies`.
- **`personas.description`**: `TEXT NOT NULL DEFAULT ''` with `CHECK (char_length(description) <= 500)`. 500 characters keep `list_personas` small for many personas.

### D2: One event hook

`df_db::tasks::record_task_event(tx, task_id, event)` with `TaskEvent::{Completed, Done, Failed { reason }, Cancelled}` runs inside the caller's transaction:

1. **Remember the first occurrence**: for `Completed`, sets `first_completed_at` if null; for `Done`, sets `first_done_at` if null. Both updates lock the target row.
2. **Release dependents**: sets every dependent that is in `awaiting_dependencies` and has no unsatisfied dependency left to `queued`. It publishes `TaskChanged` for every dependent of the target, released or not (the UI's dependency list changes either way), and `TaskMessageQueued` for released ones. Their assignment prompt is already queued, so the scheduler starts them.
3. **Wake the parent**: locks the parent row (`FOR UPDATE`). If the parent isn't `completed`, `cancelled` or `failed`, it queues the child-ended message with `queue_message_tx` (D4).

Callers:
- `end_run_tx`: outcome `completed` → `Completed`; outcome `failed` → `Failed`.
- `mark_task_done` (and through it the merged-PR path of `iter3-webhooks`) → `Done`.
- The cancel path → `Cancelled`.

Every event runs the parent wake-up; only `Completed` and `Done` can release dependents. The run-end mapping itself is unchanged.

Alternative: an outbox job per event. That's asynchronous for no gain: everything happens in the database, and the transactions are short.

### D3: Creation and the release race

`create_task` gains `created_by: Principal`, `parent_id: Option<TaskId>` and `depends_on: Vec<NewDependency { key, kind }>`. In the same transaction, after allocating the key:

1. It resolves the targets with `SELECT … FROM tasks WHERE project_id = $1 AND key = ANY($2) FOR SHARE`. A missing key, or a key given twice, returns `DbError::InvalidDependency(key)`.
2. It computes the status: `awaiting_dependencies` if any dependency is unsatisfied, otherwise `queued`.
3. It inserts the task and its dependency rows.

The race: a target's step 1 in D2 updates the target row and needs a row lock, which conflicts with `FOR SHARE`. Either the creation commits first, and the target's release query then sees the new dependent, or the target commits first, and the creation's lock read returns the new `first_completed_at`. No release is lost. A test drives both orders with two transactions.

### D4: Messages

- **`queue_message_tx`**: the `CASE` gains a first branch. The status stays or becomes `awaiting_dependencies` when the task has never had a run (`NOT EXISTS runs`) and has an unsatisfied dependency. This covers both a message on a waiting task and reopening a cancelled task that never started.
- **Child-ended message**: author `System { reason: "task.child_ended" }`, one visible text block:

  ```
  DF-21 "Review DF-20" completed.
  PR: https://forge.example/acme/app/pulls/12
  Still waiting on DF-21: DF-23

  Summary from the agent of DF-21:
  > Review done, two findings.
  ```

  - The "how it ended" wording is `completed`, `is done`, `failed` or `was cancelled`. The failure reason is the run's `detail`.
  - The child's summary is quoted under a line naming its source, so the parent's agent can tell it apart from Dark Factory's text. Like any agent output, it is untrusted.

### D5: Factory tools

- **Contract**: the three tools go into `contract/mcp/factory-tools.v2.json` (tool set v2 already allows additions). Arguments:
  - `create_task`: `persona`, `title`, `prompt`, optional `depends_on: [{task, kind}]`.
  - `list_personas`: none.
  - `send_message`: `task`, `text`.
- **Code**: a new module `crates/df-agent-channel/src/tools/delegation.rs` keeps `tools.rs` well under the 1,500-line limit.
- **`create_task`**:
  - Resolves the persona by its unique name and calls `df_db::tasks::create_task` with `created_by = Principal::agent(<run's persona>, …, run_id, task_id)` and `parent_id` = the run's task.
  - The title goes through `sanitize_title` and is stored with source `agent`. An empty result is an error, as for `set_task_title`.
  - `DbError::InvalidDependency` becomes an `is_error` result naming the key.
- **`send_message`**: loads the target by key in the run's project, checks `parent_id = run's task`, and calls `queue_message_tx` with the agent principal. It returns the prompt id and the resulting status.
- **Exactly-once**: comes from the existing `call_id` store. A retried `create_task` returns the stored key and creates nothing.

### D6: Web UI

- **Task details**:
  - creator: a human principal shows its display name; an agent principal shows `<KEY> · <persona>`, with the key linked;
  - the parent link;
  - while `awaiting_dependencies`, a "Waiting for" section listing each dependency with its satisfied state. It re-renders on `TaskChanged`, which D2 publishes for every dependent.
- **Status**: the status chip label is "awaiting dependencies".
- **Filters**: the "Waiting" filter group adds the status in `df-core`, where the group mapping lives.
- **Personas**: the persona form gets a description textarea (500 characters, counter). The card shows the description when it is set.

### D7: Demo and tests without webhooks

- **e2e**: one scenario creates a coordinator task whose fake script calls `create_task` twice (implementation, then review with `depends_on` completion). The implementation script calls `complete_task`; the review task starts; the coordinator receives the child-ended message and calls `send_message`. No webhook and no forge review are involved.
- **Database tests**: the D2 and D3 tests use `df_test_postgres` (container-backed).

## Risks / Trade-offs

- [Parent and child ping-pong without a limit until `iter3-task-tree`] → The board member sees it in the transcript and can cancel. A per-child message limit follows if it shows up.
- [Unbounded delegation by a prompt-injected agent] → No cap in this change (decided). `iter3-task-tree` brings depth and children limits right after, and the instance concurrency limit bounds parallel runs.
- [Child summaries are untrusted input to the parent's agent] → They are quoted and attributed in the message (D4). The parent can only act within its own task and children.
- [Parent woken twice for one merged child (Completion, then Done)] → Intended. The merge is news for a coordinator, and a wake-up with nothing to do costs a few tokens.
- [`FOR SHARE` on targets serializes creation against target events] → The locks are held only for single-row updates, so contention is negligible.
- [`created_by` as JSONB loses the foreign key to `users`] → Users are never deleted (audit references them the same way), and the display name is a snapshot, as for message authors.

## Migration Plan

- One forward migration (D1), run by `dark-factory migrate`. Existing tasks keep their status and get `created_by` converted. No task has dependencies or a parent.
- Regenerate `.sqlx/` for the changed queries.
- Rollback: pre-MVP, forward-only. No down migration.
