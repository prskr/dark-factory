# Spec Delta

## ADDED Requirements

### Requirement: Details tabs
The task details column SHALL have two tabs: Overview, holding the details shown so far plus the base task as a link when set, and Subtasks, listing the task's direct subtasks with key, title, status and their own done/total chip, below a progress bar of done over total. The Subtasks tab SHALL be shown only for tasks with subtasks, and both tabs SHALL update live.

#### Scenario: Feature task
- **WHEN** a board member opens `DF-30` with subtasks `DF-31` (`completed`) and `DF-32` (`running`)
- **THEN** the Subtasks tab lists both with their status and shows 1 of 2 done

#### Scenario: Task without subtasks
- **WHEN** a board member opens a task without subtasks
- **THEN** no Subtasks tab is shown

#### Scenario: Base task link
- **WHEN** a board member opens `DF-21` with base task `DF-20`
- **THEN** the Overview shows `DF-20` with its title as a link

### Requirement: Subtask progress on task rows
A task row in the task list SHALL show a done/total chip when the task has subtasks. Done SHALL count direct subtasks that are `in_review` or `completed`; total SHALL count direct subtasks that are not `cancelled`. Subtasks SHALL appear as ordinary rows in the flat list, and the chip SHALL update live.

#### Scenario: Partial progress
- **WHEN** `DF-30` has subtasks `in_review`, `completed`, `failed` and `cancelled`
- **THEN** its row shows 2/3

#### Scenario: No subtasks
- **WHEN** a task has no subtasks
- **THEN** its row shows no chip
