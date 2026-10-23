# Spec Delta

## ADDED Requirements

### Requirement: set_task_title
`set_task_title` SHALL set the task's title with source `agent` after sanitizing it, and publish a `task_changed` notification. If the task already has a title from the user, a generator or an earlier `set_task_title` call, the call SHALL succeed without changing anything and say so in its result.

#### Scenario: First title
- **WHEN** the persona calls `set_task_title` with "Stop duplicate nightly backups" on a task with a placeholder title
- **THEN** the task's title is "Stop duplicate nightly backups" with source `agent`

#### Scenario: Title already set
- **WHEN** the persona calls `set_task_title` on a task the board member renamed
- **THEN** the result says the title is kept and the title is unchanged

#### Scenario: Empty title
- **WHEN** the persona calls `set_task_title` with only whitespace
- **THEN** the result has `is_error: true` and the title is unchanged
