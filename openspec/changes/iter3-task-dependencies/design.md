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
- **Status writes**: eight statements set `tasks.status`, each with its own guards:
  - in `df-db/src/tasks.rs`: the `create_task` insert, `queue_message_tx`, withdrawing a message (back to `waiting`), `mark_task_done` and `cancel_task`;
  - in `df-agent-channel/src/runs.rs`: `start_queued_runs` (`running`), turn start and end (`running` ⇄ `waiting`) and `end_run_tx` (four overrides, then `plan_resume`).

  `update_task_status` is only called by tests.
- **Forge outbox jobs** (`iter3-webhooks`, `pr-forge-outbox`): three of these writers queue a job in the same transaction, with rules that depend on the status:
  - `end_run_tx` queues `pr.sync { run_id, open, ready }` when the branch was pushed (`RunEnd.task_branch_pushed`) or the run completed, not for a `cancelled` task (`pr::enqueue_run_end`);
  - `mark_task_done` queues `pr.sync { ready }` when the task has a PR;
  - `queue_message_tx` queues `pr.mark_draft` when it moves a task out of `in_review` or `completed` that has an unmerged PR.

  `ready` is `clock_timestamp()` read under the task lock (`clock_under_task_lock`, `pr-forge-outbox` D3). The job on a push report (`pr::enqueue_open_after_push` in `session.rs`) changes no status.
- **Factory tools**: executed in `crates/df-agent-channel/src/tools.rs` (1,244 lines), declared in `contract/mcp/factory-tools.v2.json`. Every call is stored by `call_id` for exactly-once execution.
- **Fake agent**: `crates/fake-acp-agent` already scripts arbitrary tool calls (`tool_call` steps).

## Goals / Non-Goals

**Goals:**
- One pure function decides every task status change, including what an event means for dependents and the parent ([ADR-0055](../../../docs/adr/0055-task-transitions-through-one-transition-function.md)). The current call sites only raise events.
- The existing transitions move to it first, with no behavior change, before the new statuses and events are added.
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

### D2: Task transitions (ADR-0055)

**Pure function** in `df-core` (new module `task_transition`):

```rust
pub fn transition(task: &TaskSnapshot, event: TaskEvent) -> Result<Transition, Rejected>;
```

- **`TaskSnapshot`** holds only what the task's own rows show:
  - `status`;
  - `active_run` (none, connected, finalizing, stop requested);
  - `undelivered_messages` (for a stopped run: only those queued after the stop);
  - `ever_ran`;
  - `unsatisfied_dependencies`;
  - `auto_resumes`;
  - `pr` (none, unmerged, merged).
- **`TaskEvent`**, one variant per thing that happens:

  | Event | Raised by | Today |
  |---|---|---|
  | `Created` | `create_task` | insert with `queued` |
  | `MessageQueued` | `queue_message_tx` | `CASE` in SQL |
  | `MessageWithdrawn` | withdrawing a message | back to `waiting` |
  | `RunStarted` | `start_queued_runs` | `running` |
  | `TurnStarted`, `TurnEnded` | run state `running` ⇄ `awaiting_input` | `running` ⇄ `waiting` |
  | `RunEnded { run_id, outcome, detail, retry_after, task_branch_pushed, max_auto_resumes, now }` | `end_run_tx` | overrides + `plan_resume` + `pr.sync` |
  | `MarkedDone` | `mark_task_done`, merged PR (`iter3-webhooks`) | `completed` |
  | `Cancelled` | cancel path | `cancelled` |
  | `DependencySatisfied` | effect of a target's Completion or Done | new |
  | `ChildEnded { child, how }` | effect of a child's end | new |

  `now` and the limits are event fields, so the function stays pure.
- **`Transition`**: the new status, `resume_at`, `auto_resumes`, and a list of **effects**:
  - `RecordFirst(Completion | Done)`: sets `first_completed_at` or `first_done_at` if null;
  - `NotifyDependents`: raises `DependencySatisfied` on every dependent;
  - `NotifyParent(how)`: raises `ChildEnded` on the parent;
  - `QueueMessage(author, content)`, for example the child-ended message (D4);
  - `CloseRequests(reason)`, `FinalizeIdleRun`, `StopRun`;
  - `EnqueuePrSync { run_id, open, ready }` with `ready: bool`, and `EnqueueMarkDraft`;
  - `Audit(action, details)`, `Publish(notification)`.
- **`Rejected`**: a command the status doesn't allow (`can_mark_done`, `can_cancel` move here), mapped to `DbError::Conflict`. An event that changes nothing returns an empty transition.

**Existing forge job rules as match arms** (no behavior change):
- `RunEnded`: unless the status before the event is `cancelled`, `EnqueuePrSync { run_id, open: task_branch_pushed, ready: outcome == completed }` when either is true.
- `MarkedDone`: `EnqueuePrSync { run_id: None, open: false, ready: true }` when `pr` isn't none.
- `MessageQueued`: `EnqueueMarkDraft` when the status before the event is `in_review` or `completed` and `pr` is unmerged.

The applier fills `ready` with `clock_timestamp()` when it executes `EnqueuePrSync`, still under the task lock. So `transition` stays pure and a reopening message is still ordered against the ready time (`pr-forge-outbox` D3). The push-report job changes no status and stays in `session.rs`.

**New rules as match arms:**
- `RunEnded` with outcome `completed`: `RecordFirst(Completion)`, `NotifyDependents`, `NotifyParent(completed)`. When the resulting status is `failed` (outcome `failed`, or infrastructure failures with no resume left): `NotifyParent(failed)`. The run-end mapping and its overrides are unchanged.
- `MarkedDone`: `RecordFirst(Done)`, `NotifyDependents`, `NotifyParent(done)`.
- `Cancelled`: `NotifyParent(cancelled)`.
- `DependencySatisfied`, applied to a dependent: `awaiting_dependencies` with no unsatisfied dependency left → `queued` and `Publish(TaskMessageQueued)` (its assignment prompt is already queued, so the scheduler starts it). Always `Publish(TaskChanged)`, because the UI's dependency list changes either way.
- `ChildEnded`, applied to a parent: `completed`, `cancelled` or `failed` → empty transition. Otherwise `QueueMessage` with the child-ended message, which then goes through the `MessageQueued` arm.

**Applier** in `df-db`: `apply_task_event(tx, task_id, event) -> Result<Transition, DbError>`.
- For the event a caller raises, it locks the task's run rows, then the task row, as all status writers do today. Loads the snapshot, calls `transition`, writes status, `resume_at` and `auto_resumes`, and executes the effects.
- Effects that raise events on other tasks go into a work queue in the same transaction. They are applied after the current task's writes, in order: dependents, then the parent.
- A cross-task event (`DependencySatisfied`, `ChildEnded`) locks only the target's task row, as `queue_message_tx` does today. It never locks run rows of another task. So a transaction holds at most its own task's run rows, and any task row lock it takes after them can't close a cycle with a transaction that locks run rows first (the subtree cancel of `iter3-task-tree`).
- The eight status writes in Context become calls to `apply_task_event`. `update_task_status` is removed and its tests set the status with SQL.
- `pr::enqueue_run_end` and the inline jobs of `mark_task_done` and `queue_message_tx` become the effects above.
- `end_run_tx` keeps writing the run data it carries (`harness_session_id`, `diverged`) itself. Those are no status rules.

Alternatives:
- An event hook after the existing transitions (`record_task_event`, the previous version of this decision). It leaves the rules spread across eight statements (ADR-0055).
- An outbox job per event. That's asynchronous for no gain: everything happens in the database, and the transactions are short.

### D3: Creation and the release race

`create_task` gains `created_by: Principal`, `parent_id: Option<TaskId>` and `depends_on: Vec<NewDependency { key, kind }>`. In the same transaction, after allocating the key:

1. It resolves the targets with `SELECT … FROM tasks WHERE project_id = $1 AND key = ANY($2) FOR SHARE`. A missing key, or a key given twice, returns `DbError::InvalidDependency(key)`.
2. It counts the unsatisfied dependencies and calls `transition` with `Created` on a snapshot built from that input, since there is no row to load yet. The `Created` arm gives `awaiting_dependencies` if any dependency is unsatisfied, otherwise `queued`.
3. It inserts the task with that status and its dependency rows.

The race: the applier locks a target's row (`FOR UPDATE`) before it handles the target's event, and that lock conflicts with `FOR SHARE`. Either the creation commits first, and the target's release query then sees the new dependent, or the target commits first, and the creation's lock read returns the new `first_completed_at`. No release is lost. A test drives both orders with two transactions.

### D4: Messages

- **`queue_message_tx`**: inserts the message and applies `MessageQueued`. The SQL `CASE` moves into that arm (`running` with a connected run that has no stop request, otherwise `queued`). The arm gains a first rule: the status stays or becomes `awaiting_dependencies` when the task has never run (`ever_ran` is false) and has an unsatisfied dependency. This covers both a message on a waiting task and reopening a cancelled task that never started.
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
- [Moving the eight existing status writes is a refactor of the run lifecycle] → It is a separate task group (tasks 1.x) that changes no behavior. The existing `df-db` and `df-agent-channel` tests must stay green unchanged before any new event lands. The `end_run_tx` override and `plan_resume` tests become unit tests of `transition`.
- [Cross-task effects lock a second task while the first is locked] → Cross-task events lock only task rows, never another task's run rows (D2). A transaction that locks several tasks' run rows takes all of them before any task row (`iter3-task-tree` subtree cancel).
- [`created_by` as JSONB loses the foreign key to `users`] → Users are never deleted (audit references them the same way), and the display name is a snapshot, as for message authors.

## Migration Plan

- One forward migration (D1), run by `dark-factory migrate`. Existing tasks keep their status and get `created_by` converted. No task has dependencies or a parent.
- Regenerate `.sqlx/` for the changed queries.
- Rollback: pre-MVP, forward-only. No down migration.
