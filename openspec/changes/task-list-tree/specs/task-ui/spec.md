## MODIFIED Requirements

### Requirement: Task list
The UI SHALL list tasks in the chosen sort order (default: last update, newest first) with key, title, status cue, time of the last update and status line, and update statuses and order live without a reload. A task's last update SHALL be the latest of its creation, a status change and a rename. Agent output and queued messages that do not change the status SHALL NOT change it.

#### Scenario: Status changes while viewing
- **WHEN** a listed task changes from `queued` to `running`
- **THEN** the list shows the `running` cue (no badge) within 2 seconds without a page reload

#### Scenario: Status change moves a task up
- **WHEN** the list shows the default sort and the third task changes its status
- **THEN** within 2 seconds that task is listed first without a page reload, and its row shows the new update time

### Requirement: Subtask progress on task rows
A task row in the task list SHALL show a done/total chip when the task has subtasks that are not `cancelled`. Done SHALL count direct subtasks that are `in_review` or `completed`; total SHALL count direct subtasks that are not `cancelled`. The chip SHALL update live.

#### Scenario: Partial progress
- **WHEN** `DF-30` has subtasks `in_review`, `completed`, `failed` and `cancelled`
- **THEN** its row shows 2/3

#### Scenario: No subtasks
- **WHEN** a task has no subtasks
- **THEN** its row shows no chip

#### Scenario: Only cancelled subtasks
- **WHEN** every subtask of a task is `cancelled`
- **THEN** its row shows no chip

## ADDED Requirements

### Requirement: Top-level tasks in the task list
With a status chip selected (Active, Done or All), the task list SHALL list only top-level tasks, and the status and project filters SHALL select among them. The link-only status filters (`queued`, `running`, `waiting`) SHALL list every matching task, subtasks included, as rows without disclosure control.

#### Scenario: Default list
- **WHEN** `DF-30` (top-level) has subtask `DF-31`, and `DF-40` was created by chat `DF-12-CHAT`
- **THEN** the default list shows `DF-30` and `DF-40`, and not `DF-31`

#### Scenario: Project counter link
- **WHEN** `/tasks?status=waiting` is opened and subtask `DF-31` is `waiting`
- **THEN** `DF-31` is listed as its own row

### Requirement: Opening a task's subtasks
A top-level task row with subtasks SHALL have a disclosure control that opens or closes it. An open row SHALL list its direct subtasks right below it, indented one level, in the list's sort order and regardless of the status filter. The open rows SHALL be part of the URL (`open`, comma-separated task keys) and SHALL stay open across live updates and reloads.

#### Scenario: Open a task
- **WHEN** a board member activates the disclosure control of `DF-12`, whose subtasks are `DF-13` (`completed`) and `DF-16` (`waiting`)
- **THEN** `DF-13` and `DF-16` are listed indented below `DF-12`, and the URL holds `open=DF-12`

#### Scenario: Live update keeps it open
- **WHEN** `DF-12` is open and `DF-16` changes to `running`
- **THEN** within 2 seconds `DF-16` shows the new cue and is still listed below `DF-12`

#### Scenario: Task without subtasks
- **WHEN** a top-level task has no subtasks
- **THEN** its row has no disclosure control

### Requirement: List scoped to a subtask
A subtask row with subtasks of its own SHALL have a control that opens the task list scoped to that subtask (`parent` in the URL): the subtask first, then its direct subtasks indented one level, and a breadcrumb of "Tasks" and every ancestor up to the top-level task, each a link. The scoped list SHALL hide the status chips and the project filter and SHALL update live.

#### Scenario: Second level
- **WHEN** `DF-12` is open and a board member activates the control of its subtask `DF-17`, whose subtasks are `DF-21` and `DF-22`
- **THEN** the list shows `DF-17` with `DF-21` and `DF-22` below it and the breadcrumb "Tasks › DF-12 › DF-17"

#### Scenario: Back up
- **WHEN** the list is scoped to `DF-17` and a board member activates "DF-12" in the breadcrumb
- **THEN** the list is scoped to `DF-12`

#### Scenario: Back to the full list
- **WHEN** the list is scoped to `DF-17` and a board member activates "Tasks"
- **THEN** the list shows the top-level tasks with the previous status and project filters

### Requirement: Status cues on task rows
A task row SHALL show no status badge for `running` and for `completed`; a `completed` task's title SHALL be struck through. A task that needs you SHALL show the needs-you dot in place of its badge. Every other status SHALL keep its badge.

#### Scenario: Running task
- **WHEN** `DF-14` is `running`
- **THEN** its row shows no badge

#### Scenario: Done task
- **WHEN** `DF-10` is `completed`
- **THEN** its row shows no badge and its title is struck through

#### Scenario: Failed task
- **WHEN** `DF-8` is `failed`
- **THEN** its row shows the failed badge

### Requirement: Needs-you dot
A task SHALL need you when it is `waiting` or `blocked`, or `in_review` unless its project has `subtask_merge: parent-agent` and its base task is its parent. A row SHALL show the needs-you dot when its task or any task below it needs you; when only tasks below it do, its status line SHALL read "N subtask(s) need(s) you". The dot SHALL update live.

#### Scenario: Subtask in review, humans merge
- **WHEN** project `DF` has `subtask_merge: human` and subtask `DF-16` of `DF-12` is `in_review`
- **THEN** `DF-16` and `DF-12` show the dot, and the status line of `DF-12` reads "1 subtask needs you"

#### Scenario: Subtask in review, parent agent merges
- **WHEN** project `DF` has `subtask_merge: parent-agent` and subtask `DF-31` of `DF-30`, with base task `DF-30`, is `in_review`
- **THEN** `DF-31` shows the in review badge and `DF-30` shows no dot for it

#### Scenario: Two levels down
- **WHEN** `DF-22`, a subtask of `DF-17`, which is a subtask of `DF-12`, becomes `blocked`
- **THEN** within 2 seconds the rows of `DF-17` and `DF-12` show the dot

#### Scenario: Running parent without waiting subtasks
- **WHEN** `DF-14` is `running` and none of the tasks below it needs you
- **THEN** its row shows no dot and its own status line
