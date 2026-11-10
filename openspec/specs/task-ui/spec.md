# task-ui Specification

## Purpose
The board member's view of delegated work: listing and creating tasks, following an agent's transcript live, sending follow-up messages and seeing runs, pull request and issue — usable on a phone (ADR-0035).

## Requirements

### Requirement: Task list
The UI SHALL list tasks in the chosen sort order (default: last update, newest first) with key, title, status, time of the last update and status line, and update statuses and order live without a reload. A task's last update SHALL be the latest of its creation, a status change and a rename. Agent output and queued messages that do not change the status SHALL NOT change it.

#### Scenario: Status changes while viewing
- **WHEN** a listed task changes from `queued` to `running`
- **THEN** the list shows `running` within 2 seconds without a page reload

#### Scenario: Status change moves a task up
- **WHEN** the list shows the default sort and the third task changes its status
- **THEN** within 2 seconds that task is listed first without a page reload, and its row shows the new update time

### Requirement: Live transcript
The task detail page SHALL show the task's transcript across all runs in order and append new events live via SSE. Agent text SHALL update in place while chunks stream and be rendered as Markdown when the message is complete. Tool calls SHALL be collapsed to title, kind and status, expandable on tap. The end of a turn SHALL NOT be shown as a transcript item, except that a turn ended by an interrupt ends in the row of "Interrupted turns in the transcript".

#### Scenario: Streaming answer
- **WHEN** the agent streams a message in many chunks
- **THEN** one message element grows in place, updated at most every ~200 ms, and is replaced by rendered Markdown when the turn ends

#### Scenario: Tool call details
- **WHEN** the user taps a collapsed tool call
- **THEN** its input, output and diffs are shown as escaped text

#### Scenario: Turn ends
- **WHEN** a turn ends
- **THEN** no turn-end line appears (unless an interrupt ended the turn) and the agent's message is rendered as Markdown

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
The task detail page SHALL have a message input that queues a message on the task. The draft SHALL survive SSE reconnects. Queued, undelivered messages SHALL be shown in the queued message stack above the input until the transcript shows them delivered.

#### Scenario: Follow-up message
- **WHEN** the board member sends "also add a test for X" on a `waiting` task
- **THEN** the message appears in the queued message stack and the task status becomes `queued`

### Requirement: Runs, PR and issue
The task details SHALL show the task's status line, links to the pull request and linked issue when they exist, project, persona, task branch and the runs, newest first. A run shows number, state, outcome, detail, start/end time, compact token counts (k and M, exact value as tooltip) for input, output, cache read and cache write per model from the gateway's usage records, and the number of requests with unknown usage. The run number SHALL link to that run in the transcript.

#### Scenario: Rate-limited run
- **WHEN** a run ended with outcome `rate_limited` and a retry time
- **THEN** the run list shows "rate limited" and the retry time

#### Scenario: PR opened
- **WHEN** the draft PR is opened while the page is open
- **THEN** the PR link appears without a reload

#### Scenario: Run order
- **WHEN** a task has runs #1, #2 and #3
- **THEN** the list shows #3 first and #1 last

#### Scenario: Jump to a run
- **WHEN** a board member clicks the number of a run in the runs list
- **THEN** the transcript scrolls to the divider of that run and does not jump back to the latest item

#### Scenario: Token table
- **WHEN** a run used model `claude-sonnet-5-5` with 182,000 input tokens
- **THEN** the run card shows the model name on its own line, `182k` under "in", and the tooltip shows `182000`

#### Scenario: Requests without usage
- **WHEN** two model requests of a run reported no usage
- **THEN** the run card shows "2 requests without usage" below the token table

#### Scenario: Usage refreshed at turn end
- **WHEN** a turn of a running run ends while the page is open
- **THEN** the run card shows the usage stored up to then without a reload

### Requirement: Collapsible runs list
The runs list SHALL be collapsible to its header; it starts collapsed, shows the run count in its header and keeps its state across live updates.

#### Scenario: Closed by default
- **WHEN** a board member opens a task
- **THEN** the runs list is collapsed to its header with the run count

#### Scenario: Open runs
- **WHEN** a board member opens the runs list and a new run starts
- **THEN** the list stays open

### Requirement: Task details layout
From 768 px the sections above the runs SHALL stay fixed and the runs list SHALL scroll inside its own section, which fills the remaining height of the details column. The open runs section SHALL keep room for about two run cards; if the sections above leave less, the whole details body SHALL scroll instead. A long task description SHALL be cut after three lines, with the full text as its tooltip.

#### Scenario: Many runs
- **WHEN** a task has more runs than fit in the details column
- **THEN** the sections above the runs do not move and the runs list scrolls inside the Runs section

#### Scenario: Collapsed runs area
- **WHEN** the runs list is collapsed
- **THEN** only the Runs header remains below the fixed sections

#### Scenario: Long description
- **WHEN** the task description is longer than three lines
- **THEN** the details show three lines and the full text as tooltip

#### Scenario: Little room
- **WHEN** the sections above the runs leave room for fewer than about two run cards
- **THEN** the whole details body scrolls and the open runs list keeps room for two cards

### Requirement: Phone-ready task pages
Task list, new-task composer and task detail SHALL be usable at 390 px width: no horizontal page scrolling (code blocks scroll inside their block), touch-sized targets, no hover-only interactions. On phones the composer SHALL open as a bottom sheet.

#### Scenario: Long code line on a phone
- **WHEN** the transcript contains a 300-character code line
- **THEN** only that code block scrolls horizontally

#### Scenario: New task on a phone
- **WHEN** the floating New task button is tapped at 390 px width
- **THEN** the composer opens as a bottom sheet over the task list

### Requirement: Task workspace layout
At 1280 px and more, a task's page SHALL show the task list, the transcript with the message input and the task details side by side. From 768 to 1279 px the task list and the transcript SHALL be side by side and the details SHALL open as a panel over the transcript. Below 768 px the task list and the transcript SHALL be separate screens, with the details in a bottom sheet. Selecting a task SHALL NOT reload the task list.

#### Scenario: Selecting another task on desktop
- **WHEN** a board member clicks DF-11 in the list while DF-12 is open at 1440 px width
- **THEN** the transcript and details of DF-11 replace those of DF-12, the list keeps its scroll position and the URL becomes `/tasks/DF-11`

#### Scenario: Split-screen window
- **WHEN** a task is opened at 900 px width
- **THEN** the task list and the transcript are shown side by side, and the details button opens the details as a panel over the transcript

#### Scenario: Details on a phone
- **WHEN** the details button of DF-12 is tapped at 390 px width
- **THEN** a bottom sheet with status line, links, project, persona, branch and runs opens over the transcript

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

### Requirement: Project filter
From 768 px the project menu SHALL sit in the task list's title row and offer a search over project names and keys; below 768 px the list SHALL offer no project menu and is filtered by project from the Projects screen, showing the project as a removable filter.

#### Scenario: Filter by project
- **WHEN** the board member picks "Website" in the project menu
- **THEN** only tasks of project `WEB` are listed and the URL contains `project=WEB`

#### Scenario: Project search
- **WHEN** "fac" is typed into the project menu's search
- **THEN** the menu lists only "Dark Factory"

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

### Requirement: Conversation layout
The transcript SHALL read as a conversation: human messages aligned to the right, agent messages to the left, each with author name and time below it. Consecutive messages of one author SHALL show the avatar only once, at the last message. Tool calls SHALL be one-line rows between the messages with a tool icon in front, an expand chevron at the end and a thin separator line above and below, shared by consecutive tool calls. The start of each run SHALL be marked with a divider naming the run.

#### Scenario: Agent writes twice
- **WHEN** the agent sends two messages in a row, with a tool call between them
- **THEN** both agent messages are on the left, only the second shows the avatar, and the tool call is a collapsed one-line row between them

#### Scenario: Second run
- **WHEN** run #2 of a task starts
- **THEN** the transcript shows a divider "Run #2 started" with its start time before the run's first event

#### Scenario: Tool call row
- **WHEN** the transcript shows a tool call
- **THEN** the row starts with the tool icon, ends with a chevron and has a separator line above and below

#### Scenario: Consecutive tool calls
- **WHEN** two tool calls follow each other, also when the second arrives while the page is open
- **THEN** a single separator line is between them

#### Scenario: Expand a tool call
- **WHEN** a board member opens a tool call
- **THEN** the chevron points the other way and the input and output of the call are shown

### Requirement: Queued message stack
Undelivered messages of the task SHALL be shown as a stack directly above the message input, oldest first, with their queue time, not in the transcript. The stack SHALL have a header "N queued · <persona> reads them at the next turn" (for a `failed` or `completed` task: "N queued · sent with the next run"). It SHALL show at most three rows and scroll inside itself, so the message input keeps its position. A delivered message SHALL leave the stack and appear in the transcript as a normal message.

#### Scenario: Message while the agent works
- **WHEN** a board member sends a message during a turn
- **THEN** the message appears in the stack above the input, not in the transcript

#### Scenario: Message picked up
- **WHEN** the next turn starts with that message
- **THEN** it leaves the stack and appears in the transcript as the board member's message without a reload

#### Scenario: Many queued messages
- **WHEN** seven messages are queued
- **THEN** the header reads "7 queued · Linus reads them at the next turn", three rows are visible, the list scrolls to reach the others, and the message input is not pushed down

### Requirement: Authors in the queued stack
The queued message stack SHALL include messages queued by agents and the system. A row of an agent or system author SHALL start with the author's readable name (`<name>: `); a row of a human author SHALL have no prefix.

#### Scenario: Author prefix of non-human rows
- **WHEN** the stack holds a message queued by the agent "Linus", a message queued by the system with reason `task.assigned`, and a board member's message
- **THEN** the first row starts with "Linus: ", the second with "Task assignment: ", the third has no prefix, and neither the agent's nor the system's row offers edit or remove

### Requirement: Readable system authors
Messages authored by a system principal SHALL show a readable author name in the transcript and the queued message stack, never the reason code. The agent SHALL receive the same name in its author line. A reason without a defined name SHALL show "Dark Factory".

#### Scenario: Assignment message
- **WHEN** a board member opens a task whose first message was queued by the system with reason `task.assigned`
- **THEN** the message's author reads "Task assignment"

#### Scenario: Unknown reason
- **WHEN** a message was queued by a system principal with a reason that has no defined name
- **THEN** its author reads "Dark Factory"

### Requirement: Editing queued messages in the input
Each held-back message of a human in the stack SHALL offer edit and remove. Edit SHALL load the message into the message input, mark the input as editing a queued message with a way to cancel, and replace the send action by save; the message in the stack stays highlighted until saved or cancelled. Messages the agent has already received SHALL offer neither action.

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

### Requirement: Transcript follows the latest item
The transcript SHALL open scrolled to its last item, also when another task is opened from the list. While the reader is at the bottom of the transcript (within 48 px), new and growing items SHALL keep the last item in view. A reader who scrolled up SHALL NOT be moved by new items.

#### Scenario: Open a long task
- **WHEN** a board member opens a task whose transcript is longer than the viewport
- **THEN** the latest item is visible without scrolling

#### Scenario: Agent streams while at the bottom
- **WHEN** the reader is at the bottom and the agent's message grows or a new item arrives
- **THEN** the view stays at the bottom

#### Scenario: Reader scrolled up
- **WHEN** the reader scrolled up to earlier items and a new item arrives
- **THEN** the scroll position does not change

### Requirement: Mark done action
The details pane SHALL show the task status as a select above Project. Its menu SHALL list "Mark done" for every task except a `completed` one, disabled while the task is `queued` or `running`; a `completed` task shows the plain status name. Using it SHALL set the task to `completed` without a reload; the badge and the task list row update live. Tasks in `in_review` SHALL show an "In review" badge.

#### Scenario: Mark done from the status select
- **WHEN** a board member presses "Mark done" on a `waiting` task
- **THEN** the badge shows "Completed", the action disappears and the task moves to the "Done" filter

#### Scenario: Disabled while working
- **WHEN** the task is `running`
- **THEN** the status menu shows "Mark done" disabled

#### Scenario: Plain status when completed
- **WHEN** the task is `completed`
- **THEN** the details pane shows the status without a menu

### Requirement: Notices as transcript rows
A runtime or control plane notice SHALL be shown as one line in the transcript, aligned with tool rows, with the `bot-message-square` icon, the notice code and the message, without a background box. The row SHALL be expandable (touch and keyboard) to show the full message. The level SHALL only tint the code. A notice row directly after a tool row or another notice row SHALL share its divider line.

#### Scenario: Notice row
- **WHEN** the transcript contains the notice `runtime.outbox_discarded`
- **THEN** it is one line with the icon in front, the code and the message, and a divider line above and below

#### Scenario: Long notice
- **WHEN** a notice message is longer than one line
- **THEN** the row shows it truncated and opening the row shows the full message

#### Scenario: Mixed rows
- **WHEN** a notice directly follows a tool call
- **THEN** both share one divider line between them

### Requirement: Hidden content blocks
By default, content blocks marked `hidden` SHALL NOT be shown in the transcript or in the queued message stack, and a message whose blocks are all hidden SHALL NOT be rendered. With the instance option `web.showHiddenBlocks` set, hidden blocks SHALL be shown in both places, visually distinct and labeled as an instruction for the agent, and a message of only hidden blocks SHALL be rendered. Hidden blocks SHALL still be delivered to the harness.

#### Scenario: Title instruction not shown
- **WHEN** a task without a title is created and the first run starts
- **THEN** the transcript shows the description as the user's message and does not show the `set_task_title` instruction

#### Scenario: Debug option
- **WHEN** `web.showHiddenBlocks` is `true` and the same transcript is opened
- **THEN** the instruction is shown below the description, muted, with the label "Instruction for the agent"

#### Scenario: Queued stack
- **WHEN** a message with a hidden block is queued and `web.showHiddenBlocks` is `false`
- **THEN** the queued stack shows only its visible text, and with the option set it also shows the hidden text

#### Scenario: Existing events
- **WHEN** a stored `UserMessage` event has no `hidden` key on its blocks
- **THEN** all its blocks are shown

### Requirement: Stop and cancel actions
The task detail SHALL offer "Interrupt" in the message box while a turn of the active run is running, "Stop run" for an active run in the status switch and in the run row, and "Cancel task" for tasks that can be cancelled, on phone and desktop. "Cancel task" SHALL ask for confirmation. All SHALL update the page live when the run and task change.

#### Scenario: Stop on a phone
- **WHEN** a board member taps "Stop run" at 390 px width on a running task
- **THEN** the run shows as finalizing and then `cancelled` without a page reload

#### Scenario: Interrupt only during a turn
- **WHEN** the agent finishes its turn and waits for input while the task page is open
- **THEN** the "Interrupt" button disappears from the message box without a page reload

#### Scenario: Interrupt without a turn
- **WHEN** a board member presses "Interrupt" just after the turn ended
- **THEN** the page shows the notice "No turn is running"

#### Scenario: Cancel needs confirmation
- **WHEN** a board member clicks "Cancel task"
- **THEN** nothing changes until the confirmation is accepted

### Requirement: Resume information
The task detail SHALL show the resume time of a task that waits for one, and the resume count when it is above 0.

#### Scenario: Rate-limited task
- **WHEN** a board member opens a `rate_limited` task with resume time 04:30
- **THEN** the page shows that the task resumes at 04:30

### Requirement: Reset workspace action
The task detail SHALL offer "Reset workspace" for tasks without an active run, with a confirmation that names what is lost (unpushed commits, uncommitted changes without a WIP snapshot).

#### Scenario: Reset needs confirmation
- **WHEN** a board member clicks "Reset workspace"
- **THEN** nothing changes until the confirmation is accepted

### Requirement: Forge activity as transcript rows
A forge activity event SHALL be shown as one line in the transcript in the notice row layout, tinted with the accent color and with its own icon: a commit icon for a push, a draft pull request icon for an opened draft PR, a pull request icon for a non-draft PR opened or marked ready. A rejected or failed push SHALL be tinted as an error and show the reason code. PR rows SHALL link to the PR URL.

#### Scenario: Accepted push row
- **WHEN** the transcript contains an accepted push of `factory/task/DF-1` from `abc1234` to `def5678`
- **THEN** one row with the commit icon shows the branch and `abc1234..def5678`

#### Scenario: Rejected push row
- **WHEN** the transcript contains a push rejected with `df:non-fast-forward`
- **THEN** the row is tinted as an error and shows `df:non-fast-forward`

#### Scenario: Draft PR row
- **WHEN** draft PR #12 is opened while the page is open
- **THEN** a row with the draft pull request icon and a link to PR #12 appears without a reload, in order with the agent's output

#### Scenario: PR ready row
- **WHEN** the transcript contains PR #12 marked ready
- **THEN** one row with the pull request icon says PR #12 is ready for review and links to it

### Requirement: Forge activity links are safe
A PR link in a forge activity row SHALL only be rendered for an `http` or `https` URL; any other URL SHALL be shown as text without a link.

#### Scenario: Unexpected scheme
- **WHEN** a forge activity event carries the URL `javascript:alert(1)`
- **THEN** the row shows the PR number without a link

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

### Requirement: Artifacts tab on the task page
Once a task has at least one artifact, the task details column SHALL have an Artifacts tab with the number of the task's artifacts, listing them as artifact cards. A task without artifacts SHALL have no Artifacts tab; when no other tab remains besides Overview, the details column SHALL keep its "Details" header without a tab bar. On phones the tab SHALL be part of the details bottom sheet.

#### Scenario: Research task
- **WHEN** a board member opens `DF-22` with the artifacts `findings.md` and `sources.md`
- **THEN** the Artifacts tab shows the count 2 and both cards

#### Scenario: No artifacts
- **WHEN** a board member opens a task without artifacts and without other tabs
- **THEN** the details column shows its "Details" header and no Artifacts tab

### Requirement: Artifacts tab appears with the first artifact
When the live transcript of an open task receives the task's first `write_artifact` call, the Artifacts tab SHALL appear without a page reload.

#### Scenario: First artifact
- **WHEN** a board member has a task without artifacts open and the agent writes `findings.md`
- **THEN** the Artifacts tab appears with the count 1 without a page reload

### Requirement: Artifact split view on the task page
From 768 px, opening an artifact from its card or from an artifact row in the transcript SHALL show it in the artifact split view next to the transcript, with the message input staying below the transcript, and SHALL collapse the details column to its strip. Closing the artifact SHALL restore the details column to the state it had before. Below 768 px opening an artifact SHALL open the artifact page full-screen.

#### Scenario: Desktop
- **WHEN** a board member opens `findings.md` of `DF-22` at 1440 px width
- **THEN** the transcript and the rendered `findings.md` are side by side, next to the task list, and the details column is collapsed to its strip

#### Scenario: Close restores details
- **WHEN** the details column was expanded before `findings.md` was opened and the board member closes the artifact
- **THEN** the details column is expanded again

#### Scenario: Phone
- **WHEN** a board member opens `findings.md` at 390 px width
- **THEN** the artifact page of `findings.md` is shown full-screen with a way back to the task

### Requirement: Collapsible task list
From 768 px, the task list column SHALL have a control that collapses it to a narrow strip and a control in the strip that expands it again. The browser SHALL remember the choice across tasks and reloads; the page SHALL work when browser storage is unavailable.

#### Scenario: Room for an artifact
- **WHEN** a board member collapses the task list at 1280 px width while an artifact is open
- **THEN** the transcript and the artifact share the freed width

#### Scenario: Collapse and reload
- **WHEN** a board member collapses the task list and reloads the page
- **THEN** the task list is still collapsed

### Requirement: Artifact rows in the transcript
A `write_artifact` call SHALL be shown as a transcript row with an artifact icon, the artifact name and its type that opens the artifact's current version. A failed call SHALL be shown as an ordinary tool row.

#### Scenario: Agent writes findings
- **WHEN** the agent of `DF-22` writes `findings.md`
- **THEN** the transcript shows a row "findings.md · Markdown" that opens the artifact

#### Scenario: Replaced later
- **WHEN** a board member opens the row of a `findings.md` that has since been replaced
- **THEN** the current content of `findings.md` is shown

### Requirement: Interrupted turns in the transcript
A turn that ended because of an interrupt SHALL end in the transcript with a row "Interrupted by <name>", using the name the interrupt was sent with.

#### Scenario: Interrupted turn
- **WHEN** a board member opens a task whose last turn "Peter Kurfer" interrupted
- **THEN** the transcript shows "Interrupted by Peter Kurfer" after the agent's last output of that turn

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
