# Spec Delta

## MODIFIED Requirements

### Requirement: Live transcript
The task detail page SHALL show the task's transcript across all runs in order and append new events live via SSE. Agent text SHALL update in place while chunks stream and be rendered as Markdown when the message is complete. Tool calls SHALL be collapsed to title, kind and status, expandable on tap. The end of a turn SHALL NOT be shown as a transcript item, except that a turn ended by an interrupt ends in the row of "Interrupted turns in the transcript".

#### Scenario: Streaming answer
- **WHEN** the agent streams a message in many chunks
- **THEN** one message element grows in place, updated at most every ~200 ms, and is replaced by rendered Markdown when the turn ends

#### Scenario: Tool call details
- **WHEN** the user taps a collapsed tool call
- **THEN** its input, output and diffs are shown as described in "Readable tool input and output", with all content escaped

#### Scenario: Turn ends
- **WHEN** a turn ends
- **THEN** no turn-end line appears (unless an interrupt ended the turn) and the agent's message is rendered as Markdown

### Requirement: Forge activity as transcript rows
A forge activity event SHALL be shown as one line in the transcript in the notice row layout, tinted with the accent color and with its own icon: a commit icon for a push, a draft pull request icon for an opened draft PR, a pull request icon for a non-draft PR opened or marked ready, a webhook icon for a received webhook event. A rejected or failed push SHALL be tinted as an error and show the reason code. PR rows SHALL link to the PR URL. A webhook row SHALL link to its PR, issue or CI URL.

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

#### Scenario: Review webhook row
- **WHEN** the transcript contains a received webhook event for `alice` requesting changes on PR #12
- **THEN** one row with the webhook icon reads that `alice` requested changes on PR #12, links to PR #12, and no message bubble for the wake-up text is shown

#### Scenario: CI failure webhook row
- **WHEN** the transcript contains a received webhook event for the failed check `test` with URL `https://ci.example/run/7`
- **THEN** one row with the webhook icon names the check `test` and links to `https://ci.example/run/7`

### Requirement: Forge activity links are safe
A link in a forge activity row (PR or webhook) SHALL only be rendered for an `http` or `https` URL; any other URL SHALL be shown as text without a link.

#### Scenario: Unexpected scheme
- **WHEN** a forge activity event carries the URL `javascript:alert(1)`
- **THEN** the row shows the PR number without a link

#### Scenario: Unexpected scheme in a webhook row
- **WHEN** a received webhook event carries the CI URL `javascript:alert(1)`
- **THEN** the row shows the check name without a link

### Requirement: Transcript follows the latest item
The transcript SHALL open scrolled to its last item, also when another task is opened from the list. While the reader is at the bottom of the transcript (within 48 px), new and growing items SHALL keep the last item in view. A reader who scrolled up SHALL NOT be moved by new items. Neither the transcript nor the page SHALL scroll past the last item, also when the last item is a pending question or approval card.

#### Scenario: Open a long task
- **WHEN** a board member opens a task whose transcript is longer than the viewport
- **THEN** the latest item is visible without scrolling

#### Scenario: Agent streams while at the bottom
- **WHEN** the reader is at the bottom and the agent's message grows or a new item arrives
- **THEN** the view stays at the bottom

#### Scenario: Reader scrolled up
- **WHEN** the reader scrolled up to earlier items and a new item arrives
- **THEN** the scroll position does not change

#### Scenario: Pending question at the end
- **WHEN** a board member opens a task whose transcript ends with a pending question card, on desktop or phone width
- **THEN** the card with its answer field is fully visible, no blank area follows it, and the page itself does not scroll

#### Scenario: Pending question arrives live
- **WHEN** the reader is at the bottom and a pending network-access approval card is appended
- **THEN** the card is fully visible and no blank area follows it

## ADDED Requirements

### Requirement: Thinking rows
An agent thought SHALL be shown as one collapsible row in the tool row layout: a brain icon, the label "Thinking", the thought's first line as a truncated preview and a chevron that turns when open. Opening it (touch and keyboard) SHALL show the whole thought, as plain text while it streams and as Markdown once its turn has ended.

#### Scenario: Collapsed thought
- **WHEN** the transcript contains a thought starting with "Check the failing test first"
- **THEN** one row shows the brain icon, "Thinking", "Check the failing test first" truncated, and a chevron

#### Scenario: Open a finished thought
- **WHEN** the reader opens a thought of an ended turn that contains a fenced `rust` block
- **THEN** the thought is shown as Markdown with the block highlighted

### Requirement: Tool call groups
Two or more consecutive tool calls SHALL be shown as one collapsed group row with a tool icon, "N tool calls", the number of failed calls when there are any, a running indicator while a call is in progress, and a chevron. Thoughts and notices after a call of the group SHALL be part of it. Any other transcript item SHALL end the group. A request card raised during the streak SHALL be shown after the group. A single tool call SHALL be shown as a plain tool row.

#### Scenario: Streak of tool calls
- **WHEN** the agent makes 12 tool calls in a row, with two thoughts between them, and then writes a message
- **THEN** one group row reads "12 tool calls" and is followed by the agent's message

#### Scenario: Failed calls
- **WHEN** two of the 12 calls failed
- **THEN** the group row also shows "2 failed", tinted as an error

#### Scenario: Single call
- **WHEN** a tool call is directly preceded and followed by agent messages
- **THEN** it is shown as a plain tool row, not as a group

#### Scenario: Interrupted by a message
- **WHEN** 3 tool calls are followed by an agent message and then 4 more tool calls
- **THEN** the transcript shows a group of 3, the message, and a group of 4

#### Scenario: Approval request during a streak
- **WHEN** a network-access request is raised after the 4th of 9 consecutive tool calls
- **THEN** one group of 9 is shown, followed by the request card, live and after a reload

### Requirement: Expanding a tool call group
Opening a group row (touch and keyboard) SHALL show its tool calls, thoughts and notices nested below it, in order, each collapsed and expandable on its own. The open state of the group and of its nested rows SHALL be kept while the group updates.

#### Scenario: Open a group
- **WHEN** the reader opens a group of 5 tool calls
- **THEN** 5 collapsed tool rows are shown, indented below the group row, and opening one shows its details

#### Scenario: Group grows while open
- **WHEN** the group is open, one nested tool row is open, and a new tool call arrives
- **THEN** the count increases, the new call appears at the end of the nested list, and the group and the nested row stay open

### Requirement: Live tool call groups
Groups SHALL form and grow live in the same way as they are shown after a reload. A second consecutive tool call SHALL turn the plain tool row of the first one into a group.

#### Scenario: Second call arrives
- **WHEN** the page is open, one tool call is shown as a plain row, and a second consecutive tool call arrives
- **THEN** both are shown in one group row "2 tool calls", without a reload

#### Scenario: Reload matches live view
- **WHEN** the reader reloads the page after watching a turn with several tool streaks
- **THEN** the same groups with the same counts are shown

### Requirement: Readable tool input and output
Opened tool details SHALL show the input as pretty-printed, highlighted JSON, preceded by a highlighted shell line when the input has a top-level `command` string. Text output of a finished call SHALL be rendered as Markdown, fences highlighted by language; output of a running call SHALL stay plain text. Diffs SHALL be highlighted as diffs, raw results as pretty-printed JSON. Input that is not valid JSON SHALL be shown as plain text.

#### Scenario: Console output
- **WHEN** a finished tool call's output is a fence tagged `console` containing `$ cargo test`
- **THEN** the output is shown as a highlighted code block without the fence markers

#### Scenario: Shell command input
- **WHEN** a tool call's input is `{"command":"cargo test -p df-web","timeout":120000}`
- **THEN** the details start with `cargo test -p df-web` highlighted as shell, followed by the input as indented, highlighted JSON

#### Scenario: Invalid JSON input
- **WHEN** a tool call's input is not valid JSON
- **THEN** it is shown as plain escaped text

#### Scenario: Script in output
- **WHEN** a finished tool call's output contains `<script>alert(1)</script>`
- **THEN** the text is shown literally and no script runs

### Requirement: Webhook messages in the queued stack
A queued webhook message SHALL be shown in the queued message stack as the one-line summary of its event, after the author prefix, not as its hidden text.

#### Scenario: Review waiting for the next turn
- **WHEN** `alice` requests changes on PR #12 while the agent of `DF-1` is in a turn
- **THEN** the stack shows a row "Forge events: alice requested changes on PR #12"
