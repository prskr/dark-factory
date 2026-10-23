# task-ui Specification

## Purpose
The board member's view of delegated work: listing and creating tasks, following an agent's transcript live, sending follow-up messages and seeing runs, pull request and issue — usable on a phone (ADR-0035).

## Requirements

### Requirement: Task list
The UI SHALL list tasks newest first with key, title, status, time of the last change and status line, and update statuses live without a reload.

#### Scenario: Status changes while viewing
- **WHEN** a listed task changes from `queued` to `running`
- **THEN** the list shows `running` within 2 seconds without a page reload

### Requirement: Live transcript
The task detail page SHALL show the task's transcript across all runs in order and append new events live via SSE. Agent text SHALL update in place while chunks stream and be rendered as Markdown when the message is complete. Tool calls SHALL be collapsed to title, kind and status, expandable on tap.

#### Scenario: Streaming answer
- **WHEN** the agent streams a message in many chunks
- **THEN** one message element grows in place, updated at most every ~200 ms, and is replaced by rendered Markdown when the turn ends

#### Scenario: Tool call details
- **WHEN** the user taps a collapsed tool call
- **THEN** its input, output and diffs are shown as escaped text

### Requirement: Transcript content is untrusted
All transcript content SHALL be rendered escaped; Markdown rendering SHALL NOT pass through raw HTML.

#### Scenario: HTML in agent output
- **WHEN** an agent message contains `<script>alert(1)</script>`
- **THEN** the page shows the text literally and no script runs

### Requirement: Reconnect without gaps
SSE events SHALL carry the transcript event's global id; on reconnect the server SHALL replay every event after the last received id.

#### Scenario: Phone wakes up
- **WHEN** a phone loses the connection for a minute and reconnects
- **THEN** the events of that minute appear once, in order, and none twice

### Requirement: Message input
The task detail page SHALL have a message input that queues a message on the task. The draft SHALL survive SSE reconnects. Queued, undelivered messages SHALL be shown as pending until the transcript shows them delivered.

#### Scenario: Follow-up message
- **WHEN** the board member sends "also add a test for X" on a `waiting` task
- **THEN** the message appears as pending and the task status becomes `queued`

### Requirement: Runs, PR and issue
The task details SHALL show the task's status line, the pull request and linked issue as links when they exist, project, persona and task branch, and the task's runs with number, state, outcome, detail, start/end time and raw token counts (input, output, cache read, cache write, summed per model).

#### Scenario: Rate-limited run
- **WHEN** a run ended with outcome `rate_limited` and a retry time
- **THEN** the run list shows "rate limited" and the retry time

#### Scenario: PR opened
- **WHEN** the draft PR is opened while the page is open
- **THEN** the PR link appears without a reload

### Requirement: Phone-ready task pages
Task list, new-task composer and task detail SHALL be usable at 390 px width: no horizontal page scrolling (code blocks scroll inside their block), touch-sized targets, no hover-only interactions. On phones the composer SHALL open as a bottom sheet.

#### Scenario: Long code line on a phone
- **WHEN** the transcript contains a 300-character code line
- **THEN** only that code block scrolls horizontally

#### Scenario: New task on a phone
- **WHEN** New task is tapped in the bottom bar at 390 px width
- **THEN** the composer opens as a bottom sheet over the task list

### Requirement: Task workspace layout
At 1280 px and more, a task's page SHALL show the task list, the transcript with the message input and the task details side by side. From 1024 to 1279 px the details SHALL open as a panel over the transcript. Below 1024 px the task list and the transcript SHALL be separate screens, with the details in a bottom sheet. Selecting a task SHALL NOT reload the task list.

#### Scenario: Selecting another task on desktop
- **WHEN** a board member clicks DF-11 in the list while DF-12 is open at 1440 px width
- **THEN** the transcript and details of DF-11 replace those of DF-12, the list keeps its scroll position and the URL becomes `/tasks/DF-11`

#### Scenario: Details on a phone
- **WHEN** the details button of DF-12 is tapped at 390 px width
- **THEN** a bottom sheet with status line, links, project, persona, branch and runs opens over the transcript

### Requirement: Task list filters
The task list SHALL filter by status group (Active = queued, running, rate limited; Waiting = waiting, limit reached; Done = completed, failed; All) and by project, with Active and all projects as default. The project menu SHALL offer a search over project names and keys. Filters SHALL be part of the URL and SHALL also apply to live updates.

#### Scenario: Filter by project
- **WHEN** the board member picks "Website" in the project menu
- **THEN** only tasks of project `WEB` are listed and the URL contains `project=WEB`

#### Scenario: Project search
- **WHEN** "fac" is typed into the project menu's search
- **THEN** the menu lists only "Dark Factory"

#### Scenario: Live update respects filters
- **WHEN** the list shows Active tasks and a listed task becomes `completed`
- **THEN** the task leaves the list without a reload

### Requirement: Task title display and rename
Wherever a task title is shown, a placeholder title (no title set yet) SHALL be marked as provisional. The transcript header SHALL let a board member rename the task inline; the new title SHALL appear in the list and header without a reload.

#### Scenario: Placeholder in the list
- **WHEN** a task was created and its persona has not set a title yet
- **THEN** the list shows the first sentence of its description in italics

#### Scenario: Rename
- **WHEN** the board member renames DF-15 to "Stop duplicate nightly backups"
- **THEN** list and header show the new title in regular style

### Requirement: New task composer
The UI SHALL offer a new-task composer, opened from the new-task action of the rail or bottom bar, with project, assignee (persona), one description field and an options menu holding the optional linked issue. Submitting it SHALL create the task through the task-management operations and open the new task.

#### Scenario: Create DF-1
- **WHEN** a board member submits the composer for project `DF` with a description
- **THEN** the new task `DF-1` opens with status `queued`

#### Scenario: Missing description
- **WHEN** the composer is submitted with an empty description
- **THEN** the composer stays open with an error and the chosen project and assignee kept

#### Scenario: Linked issue set
- **WHEN** issue 7 is entered in the options menu
- **THEN** the composer shows "Issue #7" as a removable chip and the task is created with issue number 7
