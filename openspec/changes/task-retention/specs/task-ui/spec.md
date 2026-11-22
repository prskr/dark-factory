# Spec Delta

## ADDED Requirements

### Requirement: Deleted tasks in the task list
The task list SHALL hide deleted tasks by default. A "Deleted" filter SHALL list only deleted tasks with key, title, final status and deletion date. The filter SHALL be part of the URL.

#### Scenario: Default list
- **WHEN** `DF-1` is deleted and a board member opens the task list with the All filter
- **THEN** `DF-1` is not listed

#### Scenario: Deleted filter
- **WHEN** a board member opens `/tasks?status=deleted`
- **THEN** only deleted tasks are listed, each with its deletion date

### Requirement: Delete action
The task page SHALL offer Delete to board members when the task is deletable (`task-retention`). The action SHALL open a confirmation that names what is removed (transcript, messages, artifacts, workspace) and what stays (title, status, PR link, runs, usage) and SHALL say that it cannot be undone.

#### Scenario: Confirm delete
- **WHEN** a board member clicks Delete on completed `DF-1` and confirms
- **THEN** the page reloads as the deleted task page

#### Scenario: Not deletable
- **WHEN** a board member opens `DF-19` whose subtask `DF-21` is running
- **THEN** the Delete action is disabled with a tooltip naming `DF-21`

### Requirement: Deleted task page
A deleted task's page SHALL show its key, title, final status, PR link, parent, creator, runs (number, outcome, times) and usage, and a banner "Deleted on <date> by <deleter>; transcript, messages and artifacts were removed". It SHALL show no transcript, no message input, no artifacts and no actions.

#### Scenario: Open a deleted task
- **WHEN** a board member opens deleted `DF-1`
- **THEN** the page shows the banner, the title, the PR link and the runs, and no transcript or message input

### Requirement: Links to deleted tasks
Every link to a deleted task (parent, subtasks, dependencies, memory provenance, related tasks) SHALL stay and SHALL show the key and title with a "deleted" marker.

#### Scenario: Parent deleted
- **WHEN** a board member opens `DF-21` whose parent `DF-19` was deleted
- **THEN** the details show `DF-19` with its title and a "deleted" marker, linked to the deleted task page

### Requirement: Deletion date on the task page
When automatic retention applies to a task and its due date is at most 7 days away, the task page SHALL show "Will be deleted on <date>" with the action Keep. A kept task SHALL show "Kept; not deleted automatically" with the action to remove the exemption.

#### Scenario: Keep a task
- **WHEN** `DF-1` shows "Will be deleted on 2026-12-01" and a board member clicks Keep
- **THEN** the page shows "Kept; not deleted automatically" and `DF-1` leaves the project's retention list

### Requirement: Retention settings of a project
The project settings SHALL show the effective retention period (instance default, own period or never), let the project change it, and list the tasks due in the next 7 days with their due dates and the action Keep.

#### Scenario: Project sets its own period
- **WHEN** a board member sets 30 days for project `DF` while the instance default is 180 days
- **THEN** the settings show 30 days as the project's period and list the tasks of `DF` due in the next 7 days
