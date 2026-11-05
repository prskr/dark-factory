# Tasks

Prerequisites: `iter2-cancel` (cancel path) is merged. `iter3-webhooks` merges first. Its merged-PR path goes through `mark_task_done`, so task 4.2 covers it without extra code; if it isn't merged yet, task 4.2 tests mark-done by hand only.

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Task transitions, no behavior change (design D2, ADR-0055)

- [ ] 1.1 Write `df-core` unit tests for today's rules, then add `task_transition::{transition, TaskSnapshot, TaskEvent, Transition, Effect, Rejected}` covering only the existing events (`Created`, `MessageQueued`, `MessageWithdrawn`, `RunStarted`, `TurnStarted`, `TurnEnded`, `RunEnded`, `MarkedDone`, `Cancelled`). Verify with `cargo test -p df-core task_transition`. The tests cover:
  - the `queue_message_tx` `CASE`;
  - the `end_run_tx` overrides (cancelled, pending messages, marked done, reopened after a stop);
  - `plan_resume` (rate limit with and without retry time, capacity rejection, backoff up to `max_auto_resumes`);
  - the forge job rules of design D2: `EnqueuePrSync` on run end (pushed, completed, both, neither, cancelled task) and on mark done (with and without PR), `EnqueueMarkDraft` on a message (from `in_review`/`completed` with unmerged PR, merged PR, no PR, other statuses);
  - `can_mark_done` and `can_cancel` as `Rejected`.

  `plan_resume`, `task_status_for` and `keeps_status_on_pending_prompts` move from `df-agent-channel` to this module, together with their pure tests.
- [ ] 1.2 Add `df_db::apply_task_event` (lock runs then task, load the snapshot, write, execute effects through a work queue in the caller's transaction). Then route all eight status writes through it:
  - in `df-db`: `create_task`, `queue_message_tx`, withdrawing a message, `mark_task_done`, `cancel_task`;
  - in `df-agent-channel`: `start_queued_runs`, turn start and end, `end_run_tx`.

  Replace `pr::enqueue_run_end` and the inline `pr.sync` / `pr.mark_draft` jobs of `mark_task_done` and `queue_message_tx` with the effects; `enqueue_open_after_push` stays. Remove `update_task_status`; its tests set the status with SQL. Verify that `cargo test -p df-db` and `cargo test -p df-agent-channel` pass with no test changed except that one, and that `grep -rn "SET status" crates --include='*.rs'` finds `tasks.status` written only in `apply_task_event` and tests.

## 2. Schema and domain

- [ ] 2.1 Write failing `df-core` tests, then add the domain types. Verify with `cargo test -p df-core`. The tests and types cover:
  - `TaskStatus::AwaitingDependencies` with string `awaiting_dependencies`, in `StatusGroup::Waiting`;
  - `DependencyKind::{Completion, Done}` round-trips through its string;
  - a pure `is_satisfied(kind, first_completed_at, first_done_at)` per spec `task-dependencies` ("When a dependency is satisfied");
  - `TaskSnapshot` gains `ever_ran` and `unsatisfied_dependencies`.
- [ ] 2.2 Add the migration of design D1:
  - `tasks.parent_id`;
  - `tasks.created_by` converted to a JSONB principal;
  - `first_completed_at`, `first_done_at`;
  - `task_dependencies`;
  - the status `CHECK`;
  - `personas.description` with its length `CHECK`.

  Then adapt every query reading `created_by` and regenerate `.sqlx/` (`docs/development.md`, "Updating `sqlx` Offline Metadata"). Verify with a `df-db` migration test: a task with a user `created_by` and one with `NULL` both come out as the D1 principals, and `cargo build --workspace` passes offline.

## 3. Creating tasks with a parent and dependencies

- [ ] 3.1 Write failing tests, then extend `create_task` with `created_by: Principal`, `parent_id` and `depends_on` as in design D3, and the `Created` arm with `awaiting_dependencies`. Verify with `cargo test -p df-core task_transition` and `cargo test -p df-db create_task`. The tests cover the scenarios of `task-dependencies` ("Dependencies given at creation", "Waiting for dependencies") and `task-management` ("Task creation queues the assignment prompt", including "Task created by an agent"):
  - unknown, foreign-project and duplicate targets;
  - an already satisfied target gives `queued`;
  - an unsatisfied target gives `awaiting_dependencies`.
- [ ] 3.2 Update the web composer's create path to pass the board member as a human principal. Verify that the existing `df-web` composer tests pass and a created task shows the user as creator.
- [ ] 3.3 Write failing `transition` unit tests, then add the `awaiting_dependencies` rule to the `MessageQueued` arm (design D4). Verify with `cargo test -p df-core task_transition` and one `df-db` test through `queue_message_tx`. The tests cover the scenarios "Message on a task awaiting dependencies" and "Reopening a cancelled task that never started" (spec `task-management`), and that a task that has run before reopens as `queued`.

## 4. Task events: release and parent wake-up

- [ ] 4.1 Write failing `transition` unit tests, then add the arms and effects of design D2: `RecordFirst`, `NotifyDependents` and `NotifyParent` on `RunEnded` (Completion, resulting status `failed`), `MarkedDone` and `Cancelled`; the `DependencySatisfied` and `ChildEnded` events; the child-ended message of design D4. Verify with `cargo test -p df-core task_transition`. The tests cover:
  - every scenario of `task-dependencies` "Dependents start once";
  - every scenario of `task-delegation` "Parent wakes when a child ends", "Final parents are not woken" and "Content of the child-ended message";
  - `Publish(TaskChanged)` for a dependent that stays waiting.
- [ ] 4.2 Write failing `df-db` and `df-agent-channel` tests, then execute the new effects in `apply_task_event` (work queue: dependents, then parent; lock order of design D2). Verify with tests:
  - a `df-agent-channel` run-end test: Completion releases a dependent and wakes the parent in one transaction;
  - `df-db` tests for mark done ("Mark done while waiting", "Project without webhooks") and cancel ("Child cancelled", "Cancel while waiting");
  - a parent that is `completed` gets no message.
- [ ] 4.3 Write a test with two concurrent transactions for the release race of design D3 (creation commits first / target event commits first). Verify that in both orders the dependent ends `queued`.

## 5. Factory tools

- [ ] 5.1 Add `create_task`, `list_personas` and `send_message` to `contract/mcp/factory-tools.v2.json` with descriptions that tell the agent:
  - delegation ends its turn;
  - the parent is woken when a child ends;
  - later rounds are its job, through `send_message`.

  Verify with the contract schema test, and that `Assign.tools` lists the three tools.
- [ ] 5.2 Write failing `df-agent-channel` tests, then implement the tools in `src/tools/delegation.rs` (design D5) and widen the scope check (spec `factory-tools`, "Calls scoped to the run's task"). Verify with `cargo test -p df-agent-channel delegation`. The tests cover every scenario of `create_task`, `list_personas`, `send_message` and "Messages from a parent to its children", including the retried `call_id`.

## 6. Web UI

- [ ] 6.1 Write failing `df-web` tests, then add the persona description: form textarea with counter, validation, and the card showing the description (spec `configuration-management`, "Persona description"). Verify with `cargo test -p df-web persona`.
- [ ] 6.2 Write failing `df-web` tests, then render creator, parent and the "Waiting for" dependency list in the task details, and the status label "awaiting dependencies". Live updates come through `TaskChanged`. Verify with `cargo test -p df-web` covering the scenarios of `task-ui` "Creator and parent in the task details" and "Dependencies in the task details".
- [ ] 6.3 Add `awaiting_dependencies` to the Waiting filter chip. Verify with a `df-web` test for "Task released from its dependencies" (spec `task-ui`, "Task list filters").
- [ ] 6.4 Check the task page of an `awaiting_dependencies` task and the persona form at 1280 px and on a narrow viewport (`just serve-local`, if a container socket is available). Verify with screenshots attached to the review, or state that this check was skipped.

## 7. Documentation

- [ ] 7.1 Write the how-to "Delegate work between agents" in `docs/` with a Mermaid sequence diagram. It covers:
  - the CEO creating an implementation and a review task;
  - the release on Completion;
  - the parent relay for the next round;
  - `done` dependencies without webhooks.

  Link it from `product/agent-playbook.md` pattern 2.2. Verify that the links resolve and `typos` passes.
- [ ] 7.2 Add reference entries for the status `awaiting_dependencies`, the message reason `task.child_ended` and the persona `description` where statuses, reasons and persona fields are documented. Verify with `grep -rn "awaiting_dependencies\|task.child_ended" docs`.
- [ ] 7.3 Add a Mermaid state diagram of the task status machine (events as edges) to the contributor docs next to `df-core`'s `task_transition` module doc, generated by hand from the `transition` arms. Verify that every `TaskStatus` and every `TaskEvent` appears in it.

## 8. End to end

- [ ] 8.1 Add a `df-e2e` scenario following design D7: coordinator → implementation + review (`depends_on` completion) → Completion → review starts → child-ended message → `send_message` → second round, with no webhook configured. Verify with `cargo test -p df-e2e` (container-backed), or state that it couldn't run.
- [ ] 8.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length` and `cargo machete`. Verify that all three pass.

## Workflow follow-up

- Archive the change after review; update `iteration-3.md` if the demo wording changed during implementation.
