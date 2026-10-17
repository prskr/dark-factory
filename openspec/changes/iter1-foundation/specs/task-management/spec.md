# Spec Delta

## Purpose

Defines tasks as the unit of delegated work: how they are identified, how creating a task and sending a message turn into pending work for an agent, and the status vocabulary every component shares.

## ADDED Requirements

### Requirement: Per-project task keys
Every task SHALL get a key `<PROJECT_KEY>-<n>` where `n` starts at 1 and increases by one per project without gaps or reuse, also under concurrent creation. Project keys SHALL match `^[A-Z][A-Z0-9]{1,9}$`.

#### Scenario: First task of a project
- **WHEN** the first task is created in project `DF`
- **THEN** its key is `DF-1`

#### Scenario: Concurrent creation
- **WHEN** two tasks are created in project `DF` at the same time
- **THEN** they receive two distinct consecutive keys

#### Scenario: Invalid project key
- **WHEN** a project is saved with key `df` or `D`
- **THEN** it is rejected with a validation error

### Requirement: Derived task names
The task branch SHALL be `factory/task/<KEY>`, the WIP snapshot branch `factory/wip/<KEY>` and the task volume name `task-<key in lower case>`.

#### Scenario: Names for DF-1
- **WHEN** the names for task `DF-1` are derived
- **THEN** they are `factory/task/DF-1`, `factory/wip/DF-1` and `task-df-1`

### Requirement: Task creation queues the assignment prompt
Creating a task SHALL store title, description, project, assigned persona, optional linked issue number and creator, and SHALL queue one prompt authored by the system principal (reason `task.assigned`) containing title and description, in the same transaction.

#### Scenario: Task created
- **WHEN** a board member creates task "Fix login" assigned to persona "Linus"
- **THEN** the task exists with status `queued` and one undelivered prompt with author reason `task.assigned`

### Requirement: Queued messages
A human SHALL be able to queue a message on any task at any time. Each message SHALL get a unique prompt id, its author and queue time, and stays undelivered until a transcript `UserMessage` event with that prompt id is persisted.

#### Scenario: Message on a waiting task
- **WHEN** a board member queues "also add a test for X" on a task in status `waiting`
- **THEN** an undelivered prompt is stored and the task status becomes `queued`

#### Scenario: Message on a failed task
- **WHEN** a message is queued on a task in status `failed`
- **THEN** the task status becomes `queued`

### Requirement: Task status vocabulary
A task SHALL have exactly one status: `queued` (pending work, no active run), `running` (a run is active), `waiting` (last run ended its turn; waits for a human), `completed` (agent called `complete_task`), `rate_limited`, `limit_reached` or `failed`.

#### Scenario: Status is always one of the defined values
- **WHEN** any component writes a task status outside this set
- **THEN** the database rejects the write

### Requirement: Change notifications
Writes that change a task, run, transcript or queued message SHALL publish a notification after commit, so any control plane replica can react without polling.

#### Scenario: Message queued on another replica
- **WHEN** replica A queues a message for task `DF-1`
- **THEN** replica B receives a `task_message_queued` notification naming the task
