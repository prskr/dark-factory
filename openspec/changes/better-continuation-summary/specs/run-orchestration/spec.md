## MODIFIED Requirements

### Requirement: Continuation summary content
The continuation summary SHALL contain the task title and description, the visible text of user messages (blocks marked `hidden` are left out), the agent text messages and one line per tool call, in transcript order. A tool line SHALL contain the call's final title, kind and status and a short form of its input (command, paths, search pattern or URL, depending on the kind), cut to 160 bytes. Completed tool calls SHALL NOT be collapsed into a counted line. For the tool calls of the last 2 turns of the earlier runs (the recent window), the summary SHALL also contain the call's output, cut to its first 512 and last 1,536 bytes, with diffs shown as path and changed line counts; the outputs of the recent window together SHALL NOT exceed 24 KiB, and the outputs of its oldest calls SHALL be left out first.

#### Scenario: Tool calls in the summary
- **WHEN** run 1 executed a tool call titled `cargo test` that ended `failed` and run 2 starts
- **THEN** `Assign.continuation.summary` contains a line with `cargo test` and `failed`

#### Scenario: Command of an older call
- **WHEN** run 1 had 5 turns and its first turn executed the command `cargo build -p df-db`, which completed
- **THEN** the summary contains a tool line with `execute`, `cargo build -p df-db` and `completed`, and no output of that call

#### Scenario: Paths of an edit
- **WHEN** an edit call reports the location `/data/repo/crates/df-db/src/tasks.rs`
- **THEN** its tool line contains `crates/df-db/src/tasks.rs`

#### Scenario: Long input
- **WHEN** a call's command is 2,000 bytes long
- **THEN** its tool line contains the first part of the command cut to 160 bytes and no line break

#### Scenario: Completed calls stay individual
- **WHEN** run 1 executed 37 consecutive tool calls that all ended `completed` and the summary fits the cap
- **THEN** the summary contains 37 tool lines and no line `Tool: 37 calls completed`

#### Scenario: Output in the recent window
- **WHEN** the last turn of run 1 ran a command whose output is 10 KiB and ends with `error[E0425]`
- **THEN** the summary contains the first 512 and the last 1,536 bytes of that output, including `error[E0425]`, and a marker with the number of bytes cut

#### Scenario: Diff in the recent window
- **WHEN** the last turn of run 1 edited a file with a diff that adds 12 lines and removes 3
- **THEN** the summary shows the path with `+12 -3 lines` and not the diff text

#### Scenario: Agent messages of two runs stay separate
- **WHEN** the last agent message of run 1 and the first agent message of run 2 follow each other and carry the same message id
- **THEN** the summary has two agent entries

#### Scenario: Hidden blocks
- **WHEN** a user message has a hidden block
- **THEN** the summary does not contain its text

### Requirement: Continuation summary size cap
The continuation summary SHALL stay within a fixed size cap. When the cap is exceeded, entries SHALL be dropped oldest first in this order: tool lines and agent messages outside the recent window, then tool outputs inside the recent window (their tool lines stay), then user messages outside the recent window, then entries inside the recent window. The newest entry SHALL always be kept (shortened if it alone exceeds the cap), exempt from the drop order.

#### Scenario: User messages kept at the cap
- **WHEN** the entries exceed the size cap and include tool lines and agent messages older than a user message
- **THEN** those tool lines and agent messages are dropped before the user message

#### Scenario: Recent outputs kept before old messages are dropped
- **WHEN** the entries exceed the size cap and old tool lines and agent messages outside the recent window exist
- **THEN** they are dropped before any output of the recent window

#### Scenario: Recent outputs dropped before user messages
- **WHEN** the entries still exceed the cap after all tool lines and agent messages outside the recent window are dropped
- **THEN** outputs of the recent window are removed oldest first before any user message is dropped

## ADDED Requirements

### Requirement: Continuation summary strategy
The control plane SHALL build the continuation summary with the strategy `minion` only if the task's project has the setting `continuation_summary: minion` and the instance has a factory minion model; otherwise it SHALL use the strategy `mechanical`. Building the summary for an `Assign` SHALL NOT call a model.

#### Scenario: Default project
- **WHEN** run 2 of a task in a project with `continuation_summary: mechanical` starts
- **THEN** the summary is the mechanical summary and no minion request is sent

#### Scenario: Minion selected but off
- **WHEN** the project has `continuation_summary: minion`, no minion model is set and run 2 starts
- **THEN** the summary is the mechanical summary

#### Scenario: Assign does not wait for a model
- **WHEN** the project has `continuation_summary: minion`, the minion provider does not answer and run 2 starts
- **THEN** `Assign` is sent without waiting for the provider

### Requirement: Stored minion summary
With the strategy `minion`, the control plane SHALL keep at most one stored summary per task with the highest transcript event id it covers. After a run of the task ends, it SHALL start a `continuation-summary` minion job in the background with the previous stored summary and the folded events after that id, and on success store the new text and id; a stored summary SHALL NOT be replaced by one that covers a lower id. A failed job SHALL leave the stored summary unchanged. `Assign` SHALL send the task header, the stored summary (cut to 32 KiB) and the mechanical entries from the earlier of the first event after the covered id and the start of the recent window, within the size cap; without a stored summary it SHALL send the mechanical summary. When `Assign` finds the stored summary missing or behind the events of ended runs, it SHALL start the job in the background without waiting for it.

#### Scenario: Summary updated at run end
- **WHEN** run 1 of a task with strategy `minion` ends and the job succeeds
- **THEN** a stored summary exists for the task that covers the highest event id of run 1

#### Scenario: Stored summary used
- **WHEN** a stored summary covers all events of runs 1 and 2 and run 3 starts
- **THEN** `Assign.continuation.summary` contains the stored text under "Summary of earlier runs" and the mechanical entries of the last 2 turns under "Recent activity"

#### Scenario: Events after the stored summary
- **WHEN** the stored summary covers run 1, the job after run 2 failed and run 3 starts
- **THEN** the summary contains the stored text and the mechanical entries of all events of run 2

#### Scenario: No stored summary yet
- **WHEN** the job after run 1 has not finished and run 2 starts
- **THEN** the summary is the mechanical summary and `Assign` is not delayed

#### Scenario: Older job finishes last
- **WHEN** a job covering up to event 900 finishes after a job covering up to event 1,200 stored its result
- **THEN** the stored summary still covers event 1,200
