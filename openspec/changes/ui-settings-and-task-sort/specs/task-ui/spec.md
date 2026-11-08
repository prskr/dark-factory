# Spec Delta

## MODIFIED Requirements

### Requirement: Task list
The UI SHALL list tasks in the chosen sort order (default: last activity, newest first) with key, title, status, time of the last activity and status line, and update statuses and order live without a reload. A task's last activity SHALL be the latest of its creation, a status change or rename, a queued message and a transcript event (agent output, forge activity). Transcript events SHALL be recorded at most once every 10 seconds per task, so the shown time can lag the latest output by up to 10 seconds.

#### Scenario: Status changes while viewing
- **WHEN** a listed task changes from `queued` to `running`
- **THEN** the list shows `running` within 2 seconds without a page reload

#### Scenario: Agent output moves a task up
- **WHEN** the list shows the default sort, the agent of the third task writes to its transcript, and no other listed task records activity until the list updates
- **THEN** within 15 seconds that task is listed first without a page reload, and its row shows the new activity time

#### Scenario: Message counts as activity
- **WHEN** the list shows the default sort and a board member queues a message for a task that is not first
- **THEN** that task is listed first after the next list update

## ADDED Requirements

### Requirement: Task list sort
The task list's title row SHALL offer a sort menu at every width with "Newest activity" (default), "Oldest activity" and "Newest created". The menu button SHALL name the current sort as its accessible name and tooltip. Ties SHALL be broken consistently, so that rows with the same time do not swap between updates.

#### Scenario: Default sort
- **WHEN** `/tasks` is opened
- **THEN** the tasks are ordered by last activity, newest first, and the menu button reads "Newest activity"

#### Scenario: Sort by creation
- **WHEN** a board member picks "Newest created" in the sort menu
- **THEN** the tasks are ordered by creation time, newest first, and the URL contains `sort=created`

#### Scenario: Sort on a phone
- **WHEN** the task list is opened at 390 px width
- **THEN** the sort menu is shown in the title row, and choosing an option sorts the list without horizontal page scrolling

### Requirement: Task list sort in the URL
The sort SHALL be part of the URL as `sort=oldest` or `sort=created`, and left out for the default; an unknown value SHALL fall back to the default. Status chips, the project filter, task links and live updates SHALL keep the chosen sort.

#### Scenario: Sort kept with filters
- **WHEN** the list shows `sort=oldest` and the board member picks the Done chip
- **THEN** the URL contains `status=done` and `sort=oldest`, and the Done tasks are ordered by last activity, oldest first

#### Scenario: Live update keeps the sort
- **WHEN** the list shows `sort=created` and the agent of an older task writes to its transcript
- **THEN** the task keeps its position

#### Scenario: Unknown sort value
- **WHEN** `/tasks?sort=bogus` is opened
- **THEN** the list uses the default sort
