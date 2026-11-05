# ADR-0055: Task transitions through one transition function

- **Status:** Accepted (planned for iteration 3, `iter3-task-dependencies`)
- **Date:** 2026-11-05
- **Related:** [ADR-0016](0016-runs-and-triggers.md), [ADR-0043](0043-delegation-chain-chats-as-tasks.md), [ADR-0049](0049-lifecycle-events-outbox-and-ed25519-signatures.md), [ADR-0052](0052-dependencies-gate-the-first-start.md), [ADR-0053](0053-parallel-chats-topics-and-archiving.md)

## Context

A task behaves like a durable actor:

- its status is the actor's state;
- `task_messages` is its inbox;
- a run is an activation;
- a turn handles the messages waiting at its start;
- other tasks, humans and the system send it messages (`send_message`, `task.child_ended`, `question.asked`, `request.decided`).

The code doesn't treat it as one state machine. Eight statements in `df-db` (`create_task`, `queue_message_tx`, withdrawing a message, `mark_task_done`, `cancel_task`) and `df-agent-channel` (`start_queued_runs`, turn start and end, `end_run_tx`) each write `tasks.status` with their own guards. `end_run_tx` alone chains four overrides before `plan_resume`. Iteration 3 adds dependents released by a target, parents woken by children, subtree cancel, the completion gate, questions readdressed to humans and archiving. Spread across these call sites, the rules can only be tested through PostgreSQL.

## Decision

- **Every change of a task's status goes through one pure function** in `df-core`: `transition(&TaskSnapshot, TaskEvent) -> Result<Transition, Rejected>`.
  - `TaskSnapshot` holds what the decision reads about the task itself: status, active run, undelivered messages, whether it ever ran, unsatisfied dependencies, resume count.
  - `TaskEvent` names what happened: created, message queued or withdrawn, run started, turn started or ended, run ended, marked done, cancelled, dependency satisfied, child ended.
  - `Transition` is the new status and resume time, plus **effects**: queue a message on a task, emit an event to another task, close pending requests, finalize the idle run, enqueue an outbox job (forge PR actions), publish, audit.
  - `Rejected` is a command the status doesn't allow (marking a running task done). An event that changes nothing returns an empty transition, not an error.
- **One applier in `df-db`** (`apply_task_event`) locks the task (its run rows first, then the task row; for an event from another task only the task row), loads the snapshot, calls `transition`, writes the result and executes the effects **in the caller's transaction**. An effect on another task becomes an event for that task, applied in the same transaction from a work queue. Nothing is sent asynchronously.
- **Other tasks' state is read inside the transaction, not through messages.** A dependency is satisfied according to its target's row (ADR-0052), and scope checks such as "the caller is the parent" read rows directly.
- **Run state stays where it is.** `runs.state` and the scheduler's slot accounting are not part of the task state machine. They raise task events.

## Consequences

- All status rules are unit tests on a pure function. The PostgreSQL tests check that the applier writes and locks correctly, not the individual rules.
- Each new iteration-3 rule adds an event or effect and a match arm instead of another status update.
- Cross-task effects lock a second task while the first is still locked. They lock only that task's row, never its run rows. A transaction that locks run rows of several tasks (subtree cancel in `iter3-task-tree`) takes all of them before any task row. With these two rules a run end that wakes a parent and a top-down subtree cancel can't deadlock.
- An effect cascade stays bounded: a child end queues a message on its parent, and a satisfied dependency queues its dependent. Neither raises another cross-task event, so the depth stays at one until the subtree cancel of `iter3-task-tree`, whose depth is bounded by `tasks.maxDepth`.

## Alternatives considered

- **An in-memory actor runtime** (one actor per task with a mailbox, for example ractor or kameo). Rejected: tasks live for days, must survive restarts, run on several replicas and need exactly-once tool calls. PostgreSQL rows, row locks and `task_messages` already give a durable actor. An in-memory runtime would add ownership and rebalancing across replicas.
- **Effects as asynchronous messages (outbox) between tasks.** Rejected: dependency release would race with task creation again (ADR-0052 and the `FOR SHARE` design rely on one transaction), and the delay buys nothing because everything happens in one database.
- **Only an event hook after the existing transitions** (the first `iter3-task-dependencies` design). Rejected: it leaves the status rules spread across eight statements and adds a second place that interprets events.
