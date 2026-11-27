## REMOVED Requirements

### Requirement: Continuation summary content
**Reason**: Completed tool calls are no longer collapsed into a counted line ("Successful tool calls collapsed", "Failed call breaks a streak" no longer apply), and tool lines gain inputs and a recent window with outputs.
**Migration**: Replaced by "Continuation summary entries", "Continuation summary tool lines" and "Continuation summary recent window". The scenarios "Tool calls in the summary", "Agent messages of two runs stay separate" and "Hidden blocks" move there unchanged. Nothing is stored, so no data migrates.

## MODIFIED Requirements

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

### Requirement: Continuation summary entries
The continuation summary SHALL contain the task title and description, the visible text of user messages (blocks marked `hidden` are left out), the agent text messages and one line per tool call, in transcript order.

#### Scenario: Tool calls in the summary
- **WHEN** run 1 executed a tool call titled `cargo test` that ended `failed` and run 2 starts
- **THEN** `Assign.continuation.summary` contains a line with `cargo test` and `failed`

#### Scenario: Agent messages of two runs stay separate
- **WHEN** the last agent message of run 1 and the first agent message of run 2 follow each other and carry the same message id
- **THEN** the summary has two agent entries

#### Scenario: Hidden blocks
- **WHEN** a user message has a hidden block
- **THEN** the summary does not contain its text

### Requirement: Continuation summary tool lines
A tool line SHALL contain the call's final title, kind and status and a short form of its input (command, paths, search pattern or URL, depending on the kind), cut to 160 bytes and without line breaks. Paths below the workspace SHALL be shown relative to it. Completed tool calls SHALL NOT be collapsed into a counted line.

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

### Requirement: Continuation summary recent window
For the tool calls of the last 2 turns of the earlier runs (the recent window), the summary SHALL also contain the call's output, cut to its first 512 and last 1,536 bytes, with diffs shown as path and changed line counts. The outputs of the recent window together SHALL NOT exceed 24 KiB; the outputs of its oldest calls SHALL be left out first.

#### Scenario: Output in the recent window
- **WHEN** the last turn of run 1 ran a command whose output is 10 KiB and ends with `error[E0425]`
- **THEN** the summary contains the first 512 and the last 1,536 bytes of that output, including `error[E0425]`, and a marker with the number of bytes cut

#### Scenario: Diff in the recent window
- **WHEN** the last turn of run 1 edited a file with a diff that adds 12 lines and removes 3
- **THEN** the summary shows the path with `+12 -3 lines` and not the diff text

#### Scenario: Window over its limit
- **WHEN** the recent window has 20 calls with 2 KiB of output each
- **THEN** the outputs of the oldest calls are left out until the rest fits 24 KiB, and their tool lines stay

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

### Requirement: Minion summary job at run end
The transaction that ends a run SHALL enqueue a `minion.continuation_summary` outbox job for the task when the effective strategy is `minion`. The job SHALL run in an outbox dispatcher that handles no other kind. It SHALL end without a minion call when the task's ended runs have no events after the stored summary, the project's strategy is `mechanical` or the minion is off.

#### Scenario: Every end path enqueues
- **WHEN** run 1 of a task with strategy `minion` ends because its sandbox was lost
- **THEN** a `minion.continuation_summary` job for the task is enqueued in the same transaction

#### Scenario: Mechanical project enqueues nothing
- **WHEN** a run of a task in a project with `continuation_summary: mechanical` ends
- **THEN** no `minion.continuation_summary` job is enqueued

#### Scenario: Restart during a summary job
- **WHEN** the control plane restarts while a `minion.continuation_summary` job is pending
- **THEN** the job runs after the restart and the stored summary is updated

#### Scenario: Two quick run ends
- **WHEN** two jobs for the same task are pending and the first stores a summary that covers both runs
- **THEN** the second job ends without a minion call

#### Scenario: Summary jobs do not delay titles
- **WHEN** 2 summary jobs wait for slow model responses
- **THEN** a pending `minion.task_title` job still runs without waiting for them

### Requirement: Stored minion summary
The control plane SHALL keep at most one stored summary per task, with the highest transcript event id it covers. The summary job SHALL send the previous stored summary and the folded events of the task's ended runs after that id to the `continuation-summary` minion call, and on success SHALL store the new text and id. A stored summary SHALL NOT be replaced by one that covers a lower id. A failed or given-up job SHALL leave it unchanged.

#### Scenario: Summary updated at run end
- **WHEN** run 1 of a task with strategy `minion` ends and the job succeeds
- **THEN** a stored summary exists for the task that covers the highest event id of run 1

#### Scenario: Next run end catches up
- **WHEN** the stored summary covers run 1, the job after run 2 was given up, and run 3 ends
- **THEN** the job after run 3 sends the events of runs 2 and 3 and the stored summary then covers run 3

#### Scenario: Older job finishes last
- **WHEN** an attempt covering up to event 900 finishes after an attempt covering up to event 1,200 stored its result
- **THEN** the stored summary still covers event 1,200

### Requirement: Assign with a stored minion summary
With the strategy `minion` and a stored summary, `Assign` SHALL send the task header, the stored summary (cut to 32 KiB) and the mechanical entries from the earlier of the first event after the covered id and the start of the recent window, within the size cap. Without a stored summary it SHALL send the mechanical summary.

#### Scenario: Stored summary used
- **WHEN** a stored summary covers all events of runs 1 and 2 and run 3 starts
- **THEN** `Assign.continuation.summary` contains the stored text under "Summary of earlier runs" and the mechanical entries of the last 2 turns under "Recent activity"

#### Scenario: Events after the stored summary
- **WHEN** the stored summary covers run 1, the job after run 2 was given up and run 3 starts
- **THEN** the summary contains the stored text and the mechanical entries of all events of run 2

#### Scenario: No stored summary yet
- **WHEN** the job after run 1 is still pending and run 2 starts
- **THEN** the summary is the mechanical summary and `Assign` is not delayed
