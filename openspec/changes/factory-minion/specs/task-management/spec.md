## ADDED Requirements

### Requirement: Generated titles
When the minion is on, the control plane SHALL start a `task-title` minion job after a task is created with title source `placeholder`, with the task description as input. A successful job SHALL set the title with source `generated` through the title precedence. A failed job SHALL leave the placeholder.

#### Scenario: Task without a title
- **WHEN** the minion is on and a board member creates a task with only the description "The nightly backup job sometimes runs twice. It started last week."
- **THEN** the task is created with the placeholder title, and after the job returns "Stop duplicate nightly backups" the title is "Stop duplicate nightly backups" with source `generated`

#### Scenario: Board member gave a title
- **WHEN** a board member creates a task with the title "Backup fix"
- **THEN** no `task-title` job runs

#### Scenario: Renamed while the job runs
- **WHEN** a board member renames the task before its `task-title` job returns
- **THEN** the title stays the board member's

#### Scenario: Minion off
- **WHEN** the minion is off and a task is created without a title
- **THEN** no job runs and the task keeps its placeholder title
