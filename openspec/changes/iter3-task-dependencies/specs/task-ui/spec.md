# Spec Delta

## MODIFIED Requirements

### Requirement: Task list filters
The task list SHALL filter by status group (Active = queued, running, rate limited; Waiting = awaiting dependencies, waiting, limit reached; Done = completed, failed; All) and by project, with Active and all projects as default. The URL SHALL also accept the narrower filters `queued` (queued) and `running` (running, rate limited), which have no chip and leave all chips unselected. Filters SHALL be part of the URL and SHALL also apply to live updates.

#### Scenario: Live update respects filters
- **WHEN** the list shows Active tasks and a listed task becomes `completed`
- **THEN** the task leaves the list without a reload

#### Scenario: Narrower filter from a link
- **WHEN** `/tasks?status=queued` is opened
- **THEN** only queued tasks are listed, live updates keep that filter, and no status chip is marked current

#### Scenario: Task released from its dependencies
- **WHEN** the list shows Waiting tasks and a listed task leaves `awaiting_dependencies` for `queued`
- **THEN** the task leaves the list without a reload

## ADDED Requirements

### Requirement: Creator and parent in the task details
The task details SHALL show the task's creator (the board member's name, or the creating task's key with its persona) and, if the task has a parent, the parent's key and title as a link to the parent task.

#### Scenario: Created by an agent
- **WHEN** a board member opens `DF-21`, created by the agent of `DF-19` (persona "CEO")
- **THEN** the details show "DF-19 · CEO" as creator and `DF-19` with its title as parent link

#### Scenario: Created by a board member
- **WHEN** a board member opens a task they created
- **THEN** the details show their name as creator and no parent

### Requirement: Dependencies in the task details
While a task is `awaiting_dependencies`, its details SHALL list each dependency with the target's key (linked), title, kind ("completion" or "done") and whether it is satisfied, and SHALL update when a dependency is satisfied without a reload. The status SHALL be shown as "awaiting dependencies".

#### Scenario: One of two satisfied
- **WHEN** `DF-23` waits for `DF-20` (completion, satisfied) and `DF-22` (done, not satisfied)
- **THEN** the details list both, `DF-20` marked satisfied and `DF-22` not

#### Scenario: Released while viewing
- **WHEN** the last dependency of the open task is satisfied
- **THEN** the status changes to `queued` and the dependency list disappears without a reload
