## ADDED Requirements

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

## MODIFIED Requirements

### Requirement: Queued message stack
Undelivered messages written by a human SHALL be shown as a stack directly above the message input, oldest first, with their queue time, and not inside the transcript. The stack SHALL have a header "N queued · <persona> reads them at the next turn" (for a task that is `failed` or `completed`: "N queued · sent with the next run"). The list SHALL show at most three rows and scroll inside itself, so the message input keeps its position. A message SHALL leave the stack when it is delivered and then appear in the transcript as a normal message.

#### Scenario: Message while the agent works
- **WHEN** a board member sends a message during a turn
- **THEN** the message appears in the stack above the input, not in the transcript

#### Scenario: Message picked up
- **WHEN** the next turn starts with that message
- **THEN** it leaves the stack and appears in the transcript as the board member's message without a reload

#### Scenario: Many queued messages
- **WHEN** seven messages are queued
- **THEN** the header reads "7 queued · Linus reads them at the next turn", three rows are visible, the list scrolls to reach the others, and the message input is not pushed down

### Requirement: Runs, PR and issue
The task details SHALL show the task's status line, the pull request and linked issue as links when they exist, project, persona and task branch, and the task's runs with number, state, outcome, detail, start/end time and compact token counts (k and M, the exact value as tooltip) for input, output, cache read and cache write, summed per model. Runs SHALL be listed newest first. The run number in the runs list SHALL link to the start of that run in the transcript. The runs list SHALL be collapsible to its header; it starts collapsed, shows the run count in its header and keeps its state across live updates. From 768 px the sections above the runs SHALL stay fixed and the runs list SHALL scroll inside its own section, which fills the remaining height of the details column. The open runs section SHALL keep room for about two run cards; if the sections above leave less, the whole details body SHALL scroll instead. A long task description SHALL be cut after three lines, with the full text as its tooltip.

#### Scenario: Rate-limited run
- **WHEN** a run ended with outcome `rate_limited` and a retry time
- **THEN** the run list shows "rate limited" and the retry time

#### Scenario: PR opened
- **WHEN** the draft PR is opened while the page is open
- **THEN** the PR link appears without a reload

#### Scenario: Run order
- **WHEN** a task has runs #1, #2 and #3
- **THEN** the list shows #3 first and #1 last

#### Scenario: Closed by default
- **WHEN** a board member opens a task
- **THEN** the runs list is collapsed to its header with the run count

#### Scenario: Open runs
- **WHEN** a board member opens the runs list and a new run starts
- **THEN** the list stays open

#### Scenario: Many runs
- **WHEN** a task has more runs than fit in the details column
- **THEN** the sections above the runs do not move and the runs list scrolls inside the Runs section

#### Scenario: Collapsed runs area
- **WHEN** the runs list is collapsed
- **THEN** only the Runs header remains below the fixed sections

#### Scenario: Long description
- **WHEN** the task description is longer than three lines
- **THEN** the details show three lines and the full text as tooltip

#### Scenario: Jump to a run
- **WHEN** a board member clicks the number of a run in the runs list
- **THEN** the transcript scrolls to the divider of that run and does not jump back to the latest item

#### Scenario: Token table
- **WHEN** a run used model `claude-sonnet-5-5` with 182,000 input tokens
- **THEN** the run card shows the model name on its own line, `182k` under "in", and the tooltip shows `182000`

#### Scenario: Little room
- **WHEN** the sections above the runs leave room for fewer than about two run cards
- **THEN** the whole details body scrolls and the open runs list keeps room for two cards

### Requirement: Live transcript
The task detail page SHALL show the task's transcript across all runs in order and append new events live via SSE. Agent text SHALL update in place while chunks stream and be rendered as Markdown when the message is complete. Tool calls SHALL be collapsed to title, kind and status, expandable on tap. The end of a turn SHALL NOT be shown as a transcript item.

#### Scenario: Streaming answer
- **WHEN** the agent streams a message in many chunks
- **THEN** one message element grows in place, updated at most every ~200 ms, and is replaced by rendered Markdown when the turn ends

#### Scenario: Tool call details
- **WHEN** the user taps a collapsed tool call
- **THEN** its input, output and diffs are shown as escaped text

#### Scenario: Turn ends
- **WHEN** a turn ends
- **THEN** no turn-end line appears and the agent's message is rendered as Markdown

### Requirement: Conversation layout
The transcript SHALL read as a conversation: human messages aligned to the right, agent messages to the left, each with author name and time below it. Consecutive messages of one author SHALL show the author's avatar only once, at the last message. Tool calls SHALL be one-line rows between the messages, each with a tool icon in front, a chevron at the end that shows the row can be expanded, and a thin separator line above and below; consecutive tool calls SHALL share one separator. The start of each run SHALL be marked with a divider naming the run.

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
