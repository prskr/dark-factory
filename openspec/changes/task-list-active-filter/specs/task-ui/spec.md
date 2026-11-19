## MODIFIED Requirements

### Requirement: Task list filters
The task list SHALL filter by status group (Active = queued, running, rate limited, awaiting dependencies, waiting, blocked, in review, limit reached; Done = completed, failed; All) and by project, with Active and all projects as default. The URL SHALL also accept the narrower filters `queued` (queued), `running` (running, rate limited) and `waiting` (awaiting dependencies, waiting, blocked, in review, limit reached), which have no chip and leave all chips unselected. Filters SHALL be part of the URL and SHALL also apply to live updates.

#### Scenario: Live update respects filters
- **WHEN** the list shows Active tasks and a listed task becomes `completed`
- **THEN** the task leaves the list without a reload

#### Scenario: Narrower filter from a link
- **WHEN** `/tasks?status=queued` is opened
- **THEN** only queued tasks are listed, live updates keep that filter, and no status chip is marked current

#### Scenario: Task released from its dependencies
- **WHEN** the list shows Waiting tasks and a listed task leaves `awaiting_dependencies` for `queued`
- **THEN** the task leaves the list without a reload

#### Scenario: Active includes tasks that wait
- **WHEN** the task list opens with the default filter and there are tasks in `running`, `waiting`, `blocked` and `in_review`
- **THEN** all of them are listed, and the chips are Active, Done and All

#### Scenario: Waiting task stays in Active
- **WHEN** the list shows Active tasks and a listed task goes from `running` to `waiting`
- **THEN** the task stays in the list

#### Scenario: Waiting from a project counter
- **WHEN** `/tasks?status=waiting` is opened from a project's Waiting counter
- **THEN** only tasks in awaiting dependencies, waiting, blocked, in review and limit reached are listed, and no status chip is marked current
