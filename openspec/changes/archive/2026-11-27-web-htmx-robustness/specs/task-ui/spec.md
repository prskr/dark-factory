## ADDED Requirements

### Requirement: Transcript is announced to screen readers
The transcript of a task or chat SHALL be a log live region: screen readers SHALL announce items that the live update adds, without moving focus and without interrupting the user.

#### Scenario: New agent message
- **WHEN** a screen-reader user reads the details of DF-12 and the agent's new message arrives through the live update
- **THEN** the screen reader announces the message after it finishes its current output, and focus stays where it was

### Requirement: Composer remembers project and assignee
When a board member submits the new-task composer, the browser SHALL remember the chosen project and assignee. When the composer opens without a preselected project or assignee, it SHALL preselect the remembered ones if they are still offered. The composer SHALL work when browser storage is unavailable.

#### Scenario: Next task in the same project
- **WHEN** a board member created a task in project `DF` for Linus and then opens the composer again from the rail
- **THEN** the composer shows project `DF` and assignee Linus

#### Scenario: Explicit choice wins
- **WHEN** the composer opens while the task list is filtered to project `OPS`, which preselects `OPS`, and `DF` is remembered
- **THEN** the composer shows project `OPS`

#### Scenario: Remembered project removed
- **WHEN** the remembered project is no longer offered
- **THEN** the composer shows its default project

#### Scenario: Storage blocked
- **WHEN** the browser blocks storage
- **THEN** the composer opens with its defaults and submitting it creates the task
