# task-management Specification

## Purpose

Defines tasks as the unit of delegated work: how they are identified, how creating a task and sending a message turn into pending work for an agent, and the status vocabulary every component shares.

## Requirements

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
Creating a task SHALL store description, project, assigned persona, optional linked issue number, creator and an optional title, and SHALL queue one prompt authored by the system principal (reason `task.assigned`) containing the description, in the same transaction. When the task has no title from the user, the prompt SHALL ask the persona to set one with `set_task_title`.

#### Scenario: Task created
- **WHEN** a board member creates a task with description "Fix login" assigned to persona "Linus"
- **THEN** the task exists with status `queued` and one undelivered prompt with author reason `task.assigned`

#### Scenario: Prompt asks for a title
- **WHEN** a task is created without a title
- **THEN** its assignment prompt contains the description and asks the persona to call `set_task_title`

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

### Requirement: Task title sources
A task's title SHALL come from one of four sources, ranked user > generated > agent > placeholder. A task created without a title SHALL get the first sentence of its description, cut to 80 characters, as placeholder. A source SHALL only replace a title from the same or a lower-ranked source, except that agent and generated titles are set at most once.

#### Scenario: Placeholder at creation
- **WHEN** a task is created with description "The nightly backup job sometimes runs twice. It started last week."
- **THEN** its title is "The nightly backup job sometimes runs twice." with source `placeholder`

#### Scenario: Agent title after a rename
- **WHEN** a board member renamed the task and the persona then sets a title
- **THEN** the title stays the board member's

### Requirement: Titles are sanitized
Every title from an agent or a generator SHALL be stored as plain text: control characters and line breaks replaced by spaces, whitespace collapsed, trimmed and cut to 80 characters. An empty result SHALL be rejected and leave the title unchanged.

#### Scenario: Title with a line break
- **WHEN** the persona sets the title "Fix login\n\nand logout"
- **THEN** the stored title is "Fix login and logout"

### Requirement: Rename
A board member SHALL be able to rename a task to a non-empty title of at most 80 characters. A rename SHALL set the source to `user`, write an audit entry `task.rename` with the old and new title, and publish a `task_changed` notification.

#### Scenario: Rename DF-15
- **WHEN** a board member renames DF-15 to "Stop duplicate nightly backups"
- **THEN** the title is stored with source `user` and an audit entry `task.rename` exists
