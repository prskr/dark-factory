## ADDED Requirements

### Requirement: Mark done action
The details pane SHALL show the task status as a select above Project. Its menu SHALL offer "Mark done" for tasks that are not `queued`, `running` or `completed`; other tasks show the plain status name. Using it SHALL set the task to `completed` without a reload; the badge and the task list row update live. Tasks in `in_review` SHALL show an "In review" badge.

#### Scenario: Mark done from the status select
- **WHEN** a board member presses "Mark done" on a `waiting` task
- **THEN** the badge shows "Completed", the action disappears and the task moves to the "Done" filter

#### Scenario: Hidden while working
- **WHEN** the task is `running`
- **THEN** the details pane shows the status without a menu and no "Mark done" action
