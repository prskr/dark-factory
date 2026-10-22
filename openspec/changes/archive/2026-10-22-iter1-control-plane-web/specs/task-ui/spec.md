# Spec Delta

## Purpose

The board member's view of delegated work: listing and creating tasks, following an agent's transcript live, sending follow-up messages and seeing runs, pull request and issue — usable on a phone (ADR-0035).

## ADDED Requirements

### Requirement: Task list
The UI SHALL list tasks newest first with key, title, project, persona, status and status line, and update statuses live without a reload.

#### Scenario: Status changes while viewing
- **WHEN** a listed task changes from `queued` to `running`
- **THEN** the list shows `running` within 2 seconds without a page reload

### Requirement: Task creation
The UI SHALL offer a form with title, description, project, persona and optional issue number; submitting it SHALL create the task through the task-management operations and open its detail page.

#### Scenario: Create DF-1
- **WHEN** a board member submits the form for project `DF`
- **THEN** the detail page of the new task `DF-1` opens with status `queued`

#### Scenario: Missing title
- **WHEN** the form is submitted without a title
- **THEN** the form is shown again with an error and the entered values kept

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
The task detail page SHALL list the task's runs with number, state, outcome, detail, start/end time and raw token counts (input, output, cache read, cache write, summed per model), and link to the pull request and linked issue when they exist.

#### Scenario: Rate-limited run
- **WHEN** a run ended with outcome `rate_limited` and a retry time
- **THEN** the run list shows "rate limited" and the retry time

#### Scenario: PR opened
- **WHEN** the draft PR is opened while the page is open
- **THEN** the PR link appears without a reload

### Requirement: Phone-ready task pages
Task list, task creation and task detail SHALL be usable at 390 px width: no horizontal page scrolling (code blocks scroll inside their block), touch-sized targets, no hover-only interactions.

#### Scenario: Long code line on a phone
- **WHEN** the transcript contains a 300-character code line
- **THEN** only that code block scrolls horizontally
