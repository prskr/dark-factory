# Spec Delta

## ADDED Requirements

### Requirement: Conversation layout
The transcript SHALL read as a conversation: human messages aligned to the right, agent messages to the left, each with author name and time below it. Consecutive messages of one author SHALL show the author's avatar only once, at the last message. Tool calls SHALL be one-line rows between the messages, and the start of each run SHALL be marked with a divider naming the run.

#### Scenario: Agent writes twice
- **WHEN** the agent sends two messages in a row, with a tool call between them
- **THEN** both agent messages are on the left, only the second shows the avatar, and the tool call is a collapsed one-line row between them

#### Scenario: Second run
- **WHEN** run #2 of a task starts
- **THEN** the transcript shows a divider "Run #2 started" with its start time before the run's first event

### Requirement: Queued message stack
Undelivered messages written by a human SHALL be shown as a stack directly above the message input, oldest first, with their queue time, and not inside the transcript. A message SHALL leave the stack when it is delivered and then appear in the transcript as a normal message.

#### Scenario: Message while the agent works
- **WHEN** a board member sends a message during a turn
- **THEN** the message appears in the stack above the input, not in the transcript

#### Scenario: Message picked up
- **WHEN** the next turn starts with that message
- **THEN** it leaves the stack and appears in the transcript as the board member's message without a reload

### Requirement: Editing queued messages in the input
Each held-back message in the stack SHALL offer edit and remove. Edit SHALL load the message into the message input, mark the input as editing a queued message with a way to cancel, and replace the send action by save; the message in the stack stays highlighted until saved or cancelled. Messages the agent has already received SHALL offer neither action.

#### Scenario: Edit and save
- **WHEN** a board member edits a queued message in the input and saves it
- **THEN** the stack shows the new text and the input is empty again

#### Scenario: Forwarded while editing
- **WHEN** the message is forwarded to the agent while it is being edited and the board member saves
- **THEN** the input keeps the edited text and shows "The agent has already received this message."

#### Scenario: Remove
- **WHEN** a board member removes a queued message
- **THEN** it disappears from the stack without a reload

### Requirement: Collapsible details column
At 1280 px and more, the details column SHALL have a control that collapses it to a narrow strip showing the task status and links to the pull request and issue, and a control in the strip that expands it again. The browser SHALL remember the choice across tasks and reloads.

#### Scenario: Collapse and reload
- **WHEN** a board member collapses the details at 1440 px width and reloads the page
- **THEN** the details are still collapsed and the transcript uses the freed width

### Requirement: Empty task list and workspace
When the task list has no entries for the current filters, it SHALL say so and offer to show all tasks. When no task is selected on a screen that shows the transcript column, that column SHALL say that no task is selected and offer the new-task action.

#### Scenario: No active tasks
- **WHEN** the task list is filtered to Active and no task is active
- **THEN** the list shows "No active tasks" and a link "Show all tasks"

#### Scenario: Workspace without selection
- **WHEN** `/tasks` is opened at 1440 px width
- **THEN** the transcript column shows "No task selected" and a New task button

## MODIFIED Requirements

### Requirement: Message input
The task detail page SHALL have a message input that queues a message on the task. The draft SHALL survive SSE reconnects. Queued, undelivered messages SHALL be shown in the queued message stack above the input until the transcript shows them delivered.

#### Scenario: Follow-up message
- **WHEN** the board member sends "also add a test for X" on a `waiting` task
- **THEN** the message appears in the queued message stack and the task status becomes `queued`

### Requirement: Task list filters
The task list SHALL filter by status group (Active = queued, running, rate limited; Waiting = waiting, limit reached; Done = completed, failed; All) and by project, with Active and all projects as default. From 1024 px the project menu SHALL sit in the task list's title row and offer a search over project names and keys; below 1024 px the list SHALL offer no project menu and is filtered by project from the Projects screen, showing the project as a removable filter. Filters SHALL be part of the URL and SHALL also apply to live updates.

#### Scenario: Filter by project
- **WHEN** the board member picks "Website" in the project menu
- **THEN** only tasks of project `WEB` are listed and the URL contains `project=WEB`

#### Scenario: Project search
- **WHEN** "fac" is typed into the project menu's search
- **THEN** the menu lists only "Dark Factory"

#### Scenario: Live update respects filters
- **WHEN** the list shows Active tasks and a listed task becomes `completed`
- **THEN** the task leaves the list without a reload

#### Scenario: Project filter on a phone
- **WHEN** a board member opens the task list of project `DF` from the Projects screen at 390 px width
- **THEN** only tasks of `DF` are listed, the list shows "Dark Factory" as a filter with a control to remove it, and the status chips still work

### Requirement: Task title display and rename
Wherever a task title is shown, a placeholder title (no title set yet) SHALL be marked as provisional. The transcript header SHALL let a board member rename the task inline, with explicit save and cancel controls; the new title SHALL appear in the list and header without a reload.

#### Scenario: Placeholder in the list
- **WHEN** a task was created and its persona has not set a title yet
- **THEN** the list shows the first sentence of its description in italics

#### Scenario: Rename
- **WHEN** the board member renames DF-15 to "Stop duplicate nightly backups"
- **THEN** list and header show the new title in regular style

#### Scenario: Cancel rename
- **WHEN** the board member edits the title and chooses cancel
- **THEN** the header shows the previous title and nothing is stored
