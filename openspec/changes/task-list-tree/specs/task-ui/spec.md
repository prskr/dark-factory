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
With a status chip selected (Active, Done or All), the task list SHALL list only top-level tasks, and the status and project filters SHALL select among them. Active SHALL also list a top-level task in a final status while a task below it needs you. The link-only status filters (`queued`, `running`, `waiting`) SHALL list every matching task, subtasks included, as rows without disclosure control.

#### Scenario: Default list
- **WHEN** `DF-30` (top-level) has subtask `DF-31`, and `DF-40` was created by chat `DF-12-CHAT`
- **THEN** the default list shows `DF-30` and `DF-40`, and not `DF-31`

#### Scenario: Done parent with a waiting subtask
- **WHEN** a board member marked `DF-30` done while its subtask `DF-31` is still open, and `DF-31` becomes `waiting`
- **THEN** Active lists `DF-30` with the needs-you dot until `DF-31` no longer needs you

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
A row listed below an open task or in a scoped list, whose task has subtasks, SHALL have a control that scopes the task list to that task (`parent` in the URL): the task first, then its direct subtasks indented one level, and a breadcrumb of "Tasks", every ancestor and the task's key. The scoped list SHALL hide the status chips and the project filter and SHALL update live.

#### Scenario: Second level
- **WHEN** `DF-12` is open and a board member activates the control of its subtask `DF-17`, whose subtasks are `DF-21` and `DF-22`
- **THEN** the list shows `DF-17` with `DF-21` and `DF-22` below it and the breadcrumb "Tasks › DF-12 › DF-17"

#### Scenario: Back up one level
- **WHEN** the list is scoped to `DF-22`, a subtask of `DF-17`, and a board member activates "DF-17" in the breadcrumb
- **THEN** the list is scoped to `DF-17`

#### Scenario: Back to the top-level task
- **WHEN** the list is scoped to `DF-17` and a board member activates "DF-12" in the breadcrumb
- **THEN** the list shows the top-level tasks with the previous status and project filters and `DF-12` open

### Requirement: Breadcrumb links
In the breadcrumb of a scoped list, "Tasks" SHALL link to the top-level list with the previous status, project and sort, the top-level task SHALL link to that list with the task open, every other ancestor SHALL link to the list scoped to it, and the scoped task's own key SHALL NOT be a link.

#### Scenario: Breadcrumb of a third-level task
- **WHEN** the list is scoped to `DF-22` under `DF-17` under `DF-12`, with status filter Done
- **THEN** "Tasks" leads to `/tasks?status=done`, "DF-12" to `/tasks?status=done&open=DF-12`, "DF-17" to the list scoped to `DF-17`, and "DF-22" is plain text

### Requirement: Needs you
A task SHALL need you when it is `waiting`, `blocked` or `limit_reached`, or when it is `in_review`, unless it has a parent task and no pull request or pending pull-request job, or its project has `subtask_merge: parent-agent`, its base task is its parent and the parent is not final.

#### Scenario: Subtask in review, humans merge
- **WHEN** project `DF` has `subtask_merge: human` and subtask `DF-16` of `DF-12` is `in_review` with a PR
- **THEN** `DF-16` needs you

#### Scenario: Subtask in review, parent agent merges
- **WHEN** project `DF` has `subtask_merge: parent-agent` and subtask `DF-31` of the running `DF-30`, with base task `DF-30`, is `in_review` with a PR
- **THEN** `DF-31` does not need you

#### Scenario: Parent agent gone
- **WHEN** the same `DF-31` is `in_review` with a PR and `DF-30` was cancelled
- **THEN** `DF-31` needs you

#### Scenario: Research subtask without PR
- **WHEN** subtask `DF-22` of `DF-20` is `in_review` without PR and without pending PR job
- **THEN** `DF-22` does not need you

#### Scenario: Limit reached
- **WHEN** `DF-14` is `limit_reached`
- **THEN** `DF-14` needs you

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
A row SHALL show the needs-you dot when its task or any task below it needs you, next to the task's own cue (badge or struck-through title). When only tasks below it need you, its status line SHALL read "1 subtask needs you" or "N subtasks need you", unless the row shows a resume time. The dot SHALL update live.

#### Scenario: Running parent
- **WHEN** `DF-12` is `running` and one task below it needs you
- **THEN** `DF-12` shows the dot and the status line "1 subtask needs you"

#### Scenario: Failed parent
- **WHEN** `DF-8` is `failed` and two tasks below it need you
- **THEN** `DF-8` shows the failed badge, the dot and "2 subtasks need you"

#### Scenario: Two levels down
- **WHEN** `DF-22`, a subtask of `DF-17`, which is a subtask of `DF-12`, becomes `blocked`
- **THEN** within 2 seconds the rows of `DF-17` and `DF-12` show the dot

#### Scenario: Running parent without waiting subtasks
- **WHEN** `DF-14` is `running` and none of the tasks below it needs you
- **THEN** its row shows no dot and its own status line
