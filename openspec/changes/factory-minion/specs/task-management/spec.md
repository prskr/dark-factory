## ADDED Requirements

### Requirement: Generated titles
When the minion is on, creating a task with title source `placeholder` and a non-empty description SHALL enqueue a `task-title` minion job in the same transaction. The job SHALL read the task description when it runs and SHALL set the title with source `generated` through the title precedence, also over an agent title. A failed or given-up job SHALL leave the title unchanged.

#### Scenario: Task without a title
- **WHEN** the minion is on and a board member creates a task with only the description "The nightly backup job sometimes runs twice. It started last week."
- **THEN** the task is created with the placeholder title, and after the job returns "Stop duplicate nightly backups" the title is "Stop duplicate nightly backups" with source `generated`

#### Scenario: Board member gave a title
- **WHEN** a board member creates a task with the title "Backup fix"
- **THEN** no `task-title` job runs

#### Scenario: Renamed while the job runs
- **WHEN** a board member renames the task before its `task-title` job returns
- **THEN** the title stays the board member's

#### Scenario: Agent title before the job
- **WHEN** the agent calls `set_task_title` before the task's `task-title` job runs, and the job then returns "Stop duplicate nightly backups"
- **THEN** the title is "Stop duplicate nightly backups" with source `generated`

#### Scenario: Empty description
- **WHEN** the minion is on and a task is created with an empty description and no title
- **THEN** no `task-title` job is enqueued, and the assignment prompt asks the persona to call `set_task_title`

#### Scenario: Minion off
- **WHEN** the minion is off and a task is created without a title
- **THEN** no job runs and the task keeps its placeholder title

### Requirement: No title request when the minion makes the title
When creating a task enqueues a `task-title` job, its assignment prompt SHALL NOT ask the persona to call `set_task_title`. The `set_task_title` tool SHALL stay available.

#### Scenario: Minion makes the title
- **WHEN** a task with a description is created without a title while the minion is on
- **THEN** its assignment prompt contains no hidden block and a `task-title` job is enqueued

## MODIFIED Requirements

### Requirement: Task creation queues the assignment prompt
Creating a task SHALL store description, project, assigned persona, optional linked issue number, creator (a principal), optional parent, dependencies and an optional title, and SHALL queue, in the same transaction, one prompt from the system principal (reason `task.assigned`) with the description. Without a title from its creator and without a `task-title` job, the prompt SHALL ask the persona to call `set_task_title` in a separate `hidden` content block; the description SHALL be visible.

#### Scenario: Task created
- **WHEN** a board member creates a task with description "Fix login" assigned to persona "Linus"
- **THEN** the task exists with status `queued` and one undelivered prompt with author reason `task.assigned`

#### Scenario: Prompt asks for a title
- **WHEN** a task is created without a title and no `task-title` job is enqueued (minion off, or an empty description)
- **THEN** its assignment prompt contains the description in a visible block and, in a block with `hidden` set, the request to call `set_task_title`

#### Scenario: Title given by a person
- **WHEN** a task is created with a title
- **THEN** the assignment prompt contains no hidden block

#### Scenario: Task created by an agent
- **WHEN** the agent of `DF-20` creates a task with a title and a prompt
- **THEN** the task's creator is the agent of `DF-20`, its parent is `DF-20`, and its assignment prompt contains the prompt and no hidden block
