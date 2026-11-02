# Tasks

Prerequisites: `iter2-cancel` (cancel path) is merged. `iter3-webhooks` merges first. Its merged-PR path goes through `mark_task_done`, so task 3.2 covers it without extra code; if it isn't merged yet, task 3.2 tests mark-done by hand only.

Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Schema and domain

- [ ] 1.1 Write failing `df-core` tests, then add the domain types. Verify with `cargo test -p df-core`. The tests and types cover:
  - `TaskStatus::AwaitingDependencies` with string `awaiting_dependencies`, in `StatusGroup::Waiting`;
  - `DependencyKind::{Completion, Done}` round-trips through its string;
  - a pure `is_satisfied(kind, first_completed_at, first_done_at)` per spec `task-dependencies` ("When a dependency is satisfied").
- [ ] 1.2 Add the migration of design D1:
  - `tasks.parent_id`;
  - `tasks.created_by` converted to a JSONB principal;
  - `first_completed_at`, `first_done_at`;
  - `task_dependencies`;
  - the status `CHECK`;
  - `personas.description` with its length `CHECK`.

  Then adapt every query reading `created_by` and regenerate `.sqlx/` (`docs/development.md`, "Updating `sqlx` Offline Metadata"). Verify with a `df-db` migration test: a task with a user `created_by` and one with `NULL` both come out as the D1 principals, and `cargo build --workspace` passes offline.

## 2. Creating tasks with a parent and dependencies

- [ ] 2.1 Write failing `df-db` tests, then extend `create_task` with `created_by: Principal`, `parent_id` and `depends_on` as in design D3. Verify with `cargo test -p df-db create_task`. The tests cover the scenarios of `task-dependencies` ("Dependencies given at creation", "Waiting for dependencies") and `task-management` ("Task creation queues the assignment prompt", including "Task created by an agent"):
  - unknown, foreign-project and duplicate targets;
  - an already satisfied target gives `queued`;
  - an unsatisfied target gives `awaiting_dependencies`.
- [ ] 2.2 Update the web composer's create path to pass the board member as a human principal. Verify that the existing `df-web` composer tests pass and a created task shows the user as creator.
- [ ] 2.3 Write a failing test, then change `queue_message_tx` as in design D4. Verify with `cargo test -p df-db queue_message`. The test covers the scenarios "Message on a task awaiting dependencies" and "Reopening a cancelled task that never started" (spec `task-management`), and that a task that has run before reopens as `queued`.

## 3. Task events: release and parent wake-up

- [ ] 3.1 Write failing `df-db` tests, then implement `record_task_event` (design D2) with the child-ended message of design D4. Verify with `cargo test -p df-db task_event`. The tests cover:
  - every scenario of `task-dependencies` "Dependents start once";
  - every scenario of `task-delegation` "Parent wakes when a child ends", "Final parents are not woken" and "Content of the child-ended message";
  - `TaskChanged` published for a dependent that stays waiting.
- [ ] 3.2 Call `record_task_event` from `end_run_tx` (outcomes `completed` and `failed`), `mark_task_done` and the cancel path. Verify with tests:
  - a `df-agent-channel` run-end test: Completion releases a dependent and wakes the parent;
  - `df-db` tests for mark done ("Mark done while waiting", "Project without webhooks") and cancel ("Child cancelled", "Cancel while waiting").
- [ ] 3.3 Write a test with two concurrent transactions for the release race of design D3 (creation commits first / target event commits first). Verify that in both orders the dependent ends `queued`.

## 4. Factory tools

- [ ] 4.1 Add `create_task`, `list_personas` and `send_message` to `contract/mcp/factory-tools.v2.json` with descriptions that tell the agent:
  - delegation ends its turn;
  - the parent is woken when a child ends;
  - later rounds are its job, through `send_message`.

  Verify with the contract schema test, and that `Assign.tools` lists the three tools.
- [ ] 4.2 Write failing `df-agent-channel` tests, then implement the tools in `src/tools/delegation.rs` (design D5) and widen the scope check (spec `factory-tools`, "Calls scoped to the run's task"). Verify with `cargo test -p df-agent-channel delegation`. The tests cover every scenario of `create_task`, `list_personas`, `send_message` and "Messages from a parent to its children", including the retried `call_id`.

## 5. Web UI

- [ ] 5.1 Write failing `df-web` tests, then add the persona description: form textarea with counter, validation, and the card showing the description (spec `configuration-management`, "Persona description"). Verify with `cargo test -p df-web persona`.
- [ ] 5.2 Write failing `df-web` tests, then render creator, parent and the "Waiting for" dependency list in the task details, and the status label "awaiting dependencies". Live updates come through `TaskChanged`. Verify with `cargo test -p df-web` covering the scenarios of `task-ui` "Creator and parent in the task details" and "Dependencies in the task details".
- [ ] 5.3 Add `awaiting_dependencies` to the Waiting filter chip. Verify with a `df-web` test for "Task released from its dependencies" (spec `task-ui`, "Task list filters").
- [ ] 5.4 Check the task page of an `awaiting_dependencies` task and the persona form at 1280 px and on a narrow viewport (`just serve-local`, if a container socket is available). Verify with screenshots attached to the review, or state that this check was skipped.

## 6. Documentation

- [ ] 6.1 Write the how-to "Delegate work between agents" in `docs/` with a Mermaid sequence diagram. It covers:
  - the CEO creating an implementation and a review task;
  - the release on Completion;
  - the parent relay for the next round;
  - `done` dependencies without webhooks.

  Link it from `product/agent-playbook.md` pattern 2.2. Verify that the links resolve and `typos` passes.
- [ ] 6.2 Add reference entries for the status `awaiting_dependencies`, the message reason `task.child_ended` and the persona `description` where statuses, reasons and persona fields are documented. Verify with `grep -rn "awaiting_dependencies\|task.child_ended" docs`.

## 7. End to end

- [ ] 7.1 Add a `df-e2e` scenario following design D7: coordinator → implementation + review (`depends_on` completion) → Completion → review starts → child-ended message → `send_message` → second round, with no webhook configured. Verify with `cargo test -p df-e2e` (container-backed), or state that it couldn't run.
- [ ] 7.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length` and `cargo machete`. Verify that all three pass.

## Workflow follow-up

- Archive the change after review; update `iteration-3.md` if the demo wording changed during implementation.
