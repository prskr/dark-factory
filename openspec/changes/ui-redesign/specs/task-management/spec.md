# Spec Delta

## ADDED Requirements

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

## MODIFIED Requirements

### Requirement: Task creation queues the assignment prompt
Creating a task SHALL store description, project, assigned persona, optional linked issue number, creator and an optional title, and SHALL queue one prompt authored by the system principal (reason `task.assigned`) containing the description, in the same transaction. When the task has no title from the user, the prompt SHALL ask the persona to set one with `set_task_title`.

#### Scenario: Task created
- **WHEN** a board member creates a task with description "Fix login" assigned to persona "Linus"
- **THEN** the task exists with status `queued` and one undelivered prompt with author reason `task.assigned`

#### Scenario: Prompt asks for a title
- **WHEN** a task is created without a title
- **THEN** its assignment prompt contains the description and asks the persona to call `set_task_title`
