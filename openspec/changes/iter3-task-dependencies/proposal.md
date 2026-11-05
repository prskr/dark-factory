# Proposal

## Why

Agents can't hand work to each other: only a human creates tasks, a task can't wait for another one, and nobody learns when a task they started has ended. Iteration 3 makes delegation the core primitive ([ADR-0043](../../../docs/adr/0043-delegation-chain-chats-as-tasks.md)): an agent creates tasks for other personas, tasks start once their dependencies are satisfied, and the parent wakes whenever a child ends and relays further rounds ([ADR-0052](../../../docs/adr/0052-dependencies-gate-the-first-start.md)). Chats, the task tree and artifacts build on it.

## What Changes

- **Parent and creator**:
  - `tasks.parent_id`: the task a task reports to. An agent creating a task becomes its parent; a task created in the UI has none.
  - **BREAKING** (schema): `tasks.created_by` changes from a user reference to a principal (human or agent task). Existing rows are migrated to human principals.
- **Dependencies** (`depends_on`):
  - a list of `{task, kind}` given at creation, never changed afterwards;
  - targets are existing tasks of the same project;
  - all must be satisfied.
  - Kind `completion`: the target has completed at least once (any past Completion), or is Done.
  - Kind `done`: the target is Done (PR merged or marked done).
- **New task status `awaiting_dependencies`**:
  - A task with an unsatisfied dependency is created in this status. It has no run, branch, volume or sandbox.
  - It moves to `queued` once all its dependencies are satisfied, then starts like any queued task.
  - Messages queued on it wait for the first run.
  - Cancel and mark done work as for other statuses. Mark done makes it Done and so can release its own dependents.
  - It belongs to the "Waiting" filter group.
- **Dependencies gate only the first start.** A new Completion of a target never restarts a task that has already started; later rounds are relayed by the parent.
- **Parent wake-up**:
  - The parent wakes when a child completes or becomes Done, fails, or is cancelled. The wake-up is a queued message from the system principal (reason `task.child_ended`) listing:
    - the child's key and title;
    - how it ended;
    - the summary or failure reason;
    - the PR link;
    - the tasks still `awaiting_dependencies` on that child.
  - No message is queued when the parent is Done, cancelled or failed.
- **Factory tools** (added to tool set v2):
  - `create_task`: takes `persona`, `title`, `prompt` and `depends_on`. It creates a task in the caller's project with the caller as parent and creator, and returns its key.
  - `list_personas`: returns each persona's name and description.
  - `send_message`: queues a message on one of the caller's own children, with the same rules as a board member's message. The author is the calling agent.
- **Personas** get an optional `description` (role, strengths), editable in the persona form and shown on the card.
- **Task page**:
  - The details show the creator and the parent (link).
  - While the task is `awaiting_dependencies`, the details list each dependency with its kind and current state.
- **Task transitions** (internal, [ADR-0055](../../../docs/adr/0055-task-transitions-through-one-transition-function.md)):
  - Every task status change goes through one pure function of task snapshot and event.
  - The existing status writes (creation, messages, run start, turns, run end, mark done, cancel) move to it first, with no behavior change.
  - Releasing dependents and waking the parent are effects of that function.
- **Docs**:
  - ADR-0052, ADR-0055, ADR amendments, glossary terms, `iteration-3.md` and `roadmap.md` (written);
  - the agent playbook's review loop (written);
  - a how-to "Delegate work between agents".

Out of scope:
- Limits on tree depth, children per task and relayed messages: `iter3-task-tree`.
- `base_task`, subtask branches, the Subtasks tab and cancelling a subtree: `iter3-task-tree`.
- Questions addressed to the parent (`ask_question` addressee, `answer_question`): `iter3-parent-questions`.
- Chats as parents: `iter3-chats`.
- Reading other tasks' artifacts along dependencies: `iter3-artifacts`.
- `depends_on` in the new-task composer: backlog ("Dependencies set by humans").
- Overriding the dependencies of an `awaiting_dependencies` task: backlog ("Start now").
- Restarting a dependent on every new Completion of its target, or waking the reviewed task when its reviewer ends: rejected (ADR-0052).
- Polling the forge for merges and closes: rejected; forge facts come from webhooks (ADR-0052).
- Passing task-scoped egress allow rules on to created tasks: rejected. A child would get network access no human approved for it.
- A permission check on which personas an agent may delegate to: rejected for now. Personas differ in prompt, harness and model, not in permissions; the permission model is iteration 4 (ADR-0011).

## Capabilities

### New Capabilities
- `task-dependencies`: the `depends_on` relation, its kinds, when a dependency is satisfied, the `awaiting_dependencies` status and releasing dependents.
- `task-delegation`: parent and creator of a task, the parent wake-up when a child ends, and messages from a parent to its children.

### Modified Capabilities
- `task-management`: the status vocabulary and groups gain `awaiting_dependencies`. Task creation stores a principal as creator. Queued messages leave an `awaiting_dependencies` task in that status. Completion, mark done and cancel feed dependencies and the parent wake-up.
- `factory-tools`: `create_task`, `list_personas` and `send_message`. Calls may act on the caller's own children and create tasks in the caller's project.
- `configuration-management`: persona `description` (at most 500 characters) in the form and on the card.
- `task-ui`: creator, parent and dependencies in the task details. The Waiting filter includes `awaiting_dependencies`.

## Impact

- `crates/df-db`:
  - migration:
    - `tasks.parent_id`;
    - `tasks.created_by` as a principal;
    - `tasks.first_completed_at`, `tasks.first_done_at`;
    - a `task_dependencies` table;
    - status `awaiting_dependencies`;
    - `personas.description`;
  - `apply_task_event`. Creation, `queue_message_tx`, withdrawing a message, mark done and cancel go through it. `update_task_status` is removed;
  - `create_task` with dependencies;
  - releasing dependents and the child-ended message on Completion, Done, failure and cancel, as effects;
  - `.sqlx/`.
- `crates/df-core`: the task transition function (`task_transition`), dependency kinds, the status, and validation of `create_task` input.
- `crates/df-agent-channel`: run start, turn start and end, and run end raise task events (`plan_resume` moves to `df-core`); the three tools and scope checks for child tasks.
- `contract/mcp/factory-tools.v2.json`: three tools.
- `crates/df-web`: task details, the status chip and filters, and the persona form and card.
- `crates/df-e2e`: the demo flow implement → review → relay, without webhooks.
- Builds on `iter2-cancel` (cancel path).
- Merges after `iter3-webhooks`. Its merged-PR path runs through mark-done and so releases `done` dependencies here without extra code. With webhooks missing, `done` is released by marking the task done by hand.
