# Spec Delta

## MODIFIED Requirements

### Requirement: Task status vocabulary
A task SHALL have exactly one status: `awaiting_dependencies` (created with unsatisfied dependencies, never started), `queued` (pending work, no active run), `running` (a turn is in progress or about to start), `waiting` (the last turn ended; waits for a human, the run may still be alive in its keep-alive window), `blocked` (the last run waited for an answer or approval not given within the idle deadline), `in_review` (the agent called `complete_task`; the PR waits for review), `completed` (a human marked the task done, or its PR was merged), `cancelled` (a human cancelled the task), `rate_limited`, `limit_reached` or `failed`.

#### Scenario: Status is always one of the defined values
- **WHEN** any component writes a task status outside this set
- **THEN** the database rejects the write

#### Scenario: Message on a blocked task
- **WHEN** a message is queued on a task in status `blocked`
- **THEN** the task status becomes `queued`

#### Scenario: New task with an unsatisfied dependency
- **WHEN** a task is created with a dependency that is not satisfied
- **THEN** its status is `awaiting_dependencies`

### Requirement: Task status groups
`awaiting_dependencies`, `waiting`, `blocked`, `in_review` and `limit_reached` SHALL form the "Waiting" group of the task list filters, `completed` and `failed` the "Done" group.

#### Scenario: Waiting group
- **WHEN** the task list is filtered by the "Waiting" group
- **THEN** it lists the tasks in status `awaiting_dependencies`, `waiting`, `blocked`, `in_review` and `limit_reached`

### Requirement: Task creation queues the assignment prompt
Creating a task SHALL store description, project, assigned persona, optional linked issue number, creator (a principal), optional parent, dependencies and an optional title, and SHALL queue one prompt authored by the system principal (reason `task.assigned`) containing the description, in the same transaction. When the task has no title from its creator, the prompt SHALL ask the persona to set one with `set_task_title`, in a separate content block marked `hidden`; the description SHALL be in visible blocks.

#### Scenario: Task created
- **WHEN** a board member creates a task with description "Fix login" assigned to persona "Linus"
- **THEN** the task exists with status `queued` and one undelivered prompt with author reason `task.assigned`

#### Scenario: Prompt asks for a title
- **WHEN** a task is created without a title
- **THEN** its assignment prompt contains the description in a visible block and, in a block with `hidden` set, the request to call `set_task_title`

#### Scenario: Title given by a person
- **WHEN** a task is created with a title
- **THEN** the assignment prompt contains no hidden block

#### Scenario: Task created by an agent
- **WHEN** the agent of `DF-20` creates a task with a title and a prompt
- **THEN** the task's creator is the agent of `DF-20`, its parent is `DF-20`, and its assignment prompt contains the prompt and no hidden block

### Requirement: Queued messages
A human SHALL be able to queue a message on any task at any time. Each message SHALL get a unique prompt id, its author and queue time, and stays undelivered until a transcript `UserMessage` event with that prompt id is persisted. If the task has an active run that is not `finalizing`, the status SHALL become `running`; if the task has never started a run and has an unsatisfied dependency, it SHALL become or stay `awaiting_dependencies`; otherwise it SHALL become `queued`.

#### Scenario: Message on a waiting task
- **WHEN** a board member queues "also add a test for X" on a task in status `waiting` without an active run
- **THEN** an undelivered prompt is stored and the task status becomes `queued`

#### Scenario: Message on a failed task
- **WHEN** a message is queued on a task in status `failed`
- **THEN** the task status becomes `queued`

#### Scenario: Message on a task with an idle run
- **WHEN** a message is queued on a `waiting` task whose run is `awaiting_input`
- **THEN** the task status becomes `running` and no new run is scheduled

#### Scenario: Message on a task whose run is finalizing
- **WHEN** a message is queued on a task whose run is `finalizing`
- **THEN** the task status becomes `queued` and the next run starts once that run has ended

#### Scenario: Message on a task in review
- **WHEN** a message is queued on a task in status `in_review`
- **THEN** the task status becomes `queued` and a new run starts

#### Scenario: Message on a task awaiting dependencies
- **WHEN** a message is queued on a task in status `awaiting_dependencies`
- **THEN** the task stays `awaiting_dependencies` and no run starts

#### Scenario: Reopening a cancelled task that never started
- **WHEN** a message is queued on a cancelled task that never ran and still has an unsatisfied dependency
- **THEN** the task becomes `awaiting_dependencies`
