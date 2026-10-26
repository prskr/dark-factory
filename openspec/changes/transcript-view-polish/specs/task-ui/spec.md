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
The task details SHALL show the task's status line, the pull request and linked issue as links when they exist, project, persona and task branch, and the task's runs with number, state, outcome, detail, start/end time and raw token counts (input, output, cache read, cache write, summed per model). Runs SHALL be listed newest first. The runs list SHALL be collapsible to its header; it starts expanded and keeps its state across live updates.

#### Scenario: Rate-limited run
- **WHEN** a run ended with outcome `rate_limited` and a retry time
- **THEN** the run list shows "rate limited" and the retry time

#### Scenario: PR opened
- **WHEN** the draft PR is opened while the page is open
- **THEN** the PR link appears without a reload

#### Scenario: Run order
- **WHEN** a task has runs #1, #2 and #3
- **THEN** the list shows #3 first and #1 last

#### Scenario: Collapse runs
- **WHEN** a board member collapses the runs list and a new run starts
- **THEN** the list stays collapsed
