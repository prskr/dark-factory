# Spec Delta

## MODIFIED Requirements

### Requirement: Task list
The UI SHALL list tasks in the chosen sort order (default: last update, newest first) with key, title, status, time of the last update and status line, and update statuses and order live without a reload. A task's last update SHALL be the latest of its creation, a status change and a rename. Agent output and queued messages that do not change the status SHALL NOT change it.

#### Scenario: Status changes while viewing
- **WHEN** a listed task changes from `queued` to `running`
- **THEN** the list shows `running` within 2 seconds without a page reload

#### Scenario: Status change moves a task up
- **WHEN** the list shows the default sort and the third task changes its status
- **THEN** within 2 seconds that task is listed first without a page reload, and its row shows the new update time

## ADDED Requirements

### Requirement: Task list sort
The task list's title row SHALL offer a sort menu at every width with "Recently updated" (default), "Least recently updated" and "Newest created". The menu button SHALL name the current sort as its accessible name and tooltip. Ties SHALL be broken consistently, so that rows with the same time do not swap between updates.

#### Scenario: Default sort
- **WHEN** `/tasks` is opened
- **THEN** the tasks are ordered by last update, newest first, and the menu button reads "Recently updated"

#### Scenario: Sort by creation
- **WHEN** a board member picks "Newest created" in the sort menu
- **THEN** the tasks are ordered by creation time, newest first, and the URL contains `sort=created`

#### Scenario: Sort on a phone
- **WHEN** the task list is opened at 390 px width
- **THEN** the sort menu is shown in the title row, and choosing an option sorts the list without horizontal page scrolling

### Requirement: Task list sort in the URL
The sort SHALL be part of the URL as `sort=updated-asc` or `sort=created`, and left out for the default (`sort=updated`); an unknown value SHALL fall back to the default. Status chips, the project filter, task links and live updates SHALL keep the chosen sort.

#### Scenario: Sort kept with filters
- **WHEN** the list shows `sort=updated-asc` and the board member picks the Done chip
- **THEN** the URL contains `status=done` and `sort=updated-asc`, and the Done tasks are ordered by last update, oldest first

#### Scenario: Live update keeps the sort
- **WHEN** the list shows `sort=created` and an older task changes its status
- **THEN** the task keeps its position

#### Scenario: Unknown sort value
- **WHEN** `/tasks?sort=bogus` is opened
- **THEN** the list uses the default sort
