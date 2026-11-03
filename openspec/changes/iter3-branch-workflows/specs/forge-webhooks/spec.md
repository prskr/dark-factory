# Spec Delta

## MODIFIED Requirements

### Requirement: Routing
Review, close and CI events SHALL route to the task of the project that holds the event's head branch (see `task-branches`, "One open task per branch"), or, if no open task holds it, to the most recently created task with that branch. A comment on a pull request SHALL route to the project's task with that PR number. A comment on an issue SHALL route to every non-final task of the project linked to that issue. An event matching no task SHALL be logged at debug level and dropped.

#### Scenario: Review on a task PR
- **WHEN** a member submits a review on the PR from `factory/task/DF-1`
- **THEN** the event is routed to `DF-1`

#### Scenario: Review on an attached PR
- **WHEN** a member submits a review on Renovate's PR #40 from `renovate/serde-1.x`, which `DF-13` holds
- **THEN** the event is routed to `DF-13`

#### Scenario: Mention on a linked issue
- **WHEN** a member mentions the bot on issue #7, and `DF-3` and `DF-5` are non-final tasks linked to #7
- **THEN** the event is routed to both

#### Scenario: Branch outside the factory
- **WHEN** a review is submitted on a PR from `feature/login`, which no task has
- **THEN** no task is woken and a debug event is logged
