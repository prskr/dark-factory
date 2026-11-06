# Spec Delta

## MODIFIED Requirements

### Requirement: Task list filters
The task list SHALL filter by status group (Active = queued, running, rate limited; Waiting = awaiting dependencies, waiting, blocked, in review, limit reached; Done = completed, failed; All) and by project, with Active and all projects as default. The URL SHALL also accept the narrower filters `queued` (queued) and `running` (running, rate limited), which have no chip and leave all chips unselected. Filters SHALL be part of the URL and SHALL also apply to live updates.

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
The task details SHALL show the task's creator (the board member's name, or the creating task's key with its persona) and, if the task has a parent, the parent's key and title as a link to the parent task. The creator and parent SHALL follow the branch in the properties.

#### Scenario: Created by an agent
- **WHEN** a board member opens `DF-21`, created by the agent of `DF-19` (persona "CEO")
- **THEN** the details show "DF-19 · CEO" as creator and `DF-19` with its title as parent link

#### Scenario: Created by a board member
- **WHEN** a board member opens a task they created
- **THEN** the details show their name as creator and no parent

### Requirement: Dependencies in the task details
A task with dependencies or dependants SHALL show a collapsible "Dependencies" section with its entry count between the properties (and description) and the runs; other tasks SHALL NOT. Group "Waits for" SHALL list each dependency: key (linked), title, kind ("completion" or "done") and whether it is satisfied. Group "Needed by" SHALL list each dependant: key (linked), title, kind and status.

#### Scenario: Dependants
- **WHEN** `DF-24` depends on `DF-23` (done) and a board member opens `DF-23`
- **THEN** "Needed by" lists `DF-24` with kind "done" and its status

#### Scenario: No dependencies
- **WHEN** a task has neither dependencies nor dependants
- **THEN** its details show no Dependencies section

### Requirement: Dependencies section while waiting
The Dependencies section SHALL be open when the page loads while the task is `awaiting_dependencies` and closed otherwise; live updates SHALL keep the reader's choice. A satisfied dependency SHALL be marked without a reload. The status SHALL be shown as "awaiting dependencies".

#### Scenario: One of two satisfied
- **WHEN** `DF-23` waits for `DF-20` (completion, satisfied) and `DF-22` (done, not satisfied)
- **THEN** the section is open and "Waits for" lists both, `DF-20` marked satisfied and `DF-22` not

#### Scenario: Released while viewing
- **WHEN** the last dependency of the open task is satisfied
- **THEN** the status changes to `queued` and the dependency is marked satisfied without a reload; the section stays

#### Scenario: After release
- **WHEN** a board member opens `DF-23` after it has started
- **THEN** the Dependencies section is collapsed and, expanded, lists `DF-20` and `DF-22` as satisfied
