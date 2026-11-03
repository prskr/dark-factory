## MODIFIED Requirements

### Requirement: Continuation across runs
Every run after the first SHALL carry the harness session id reported by the previous run, and a summary of the task's transcript for harnesses that can't resume the session. The control plane SHALL build the summary when it assigns a run, from all transcript events of the task except the events of the run being assigned. The summary SHALL contain the task title and description, the visible text of user messages (blocks marked `hidden` are left out), the agent text messages and one line per tool call (final title and status), in transcript order; two or more consecutive successful tool calls SHALL be collapsed into one line with their count. The summary SHALL stay within a fixed size cap; when the cap is exceeded, tool lines and agent messages SHALL be dropped oldest first before any user message is dropped. The newest entry SHALL always be kept (shortened if it alone exceeds the cap), exempt from the drop order.

#### Scenario: Follow-up run
- **WHEN** run 2 of a task starts after run 1 reported session id `S`
- **THEN** `Assign.continuation.harness_session_id` is `S` and `summary` is not empty

#### Scenario: Tool calls in the summary
- **WHEN** run 1 executed a tool call titled `cargo test` that ended `failed` and run 2 starts
- **THEN** `Assign.continuation.summary` contains a line with `cargo test` and `failed`

#### Scenario: Successful tool calls collapsed
- **WHEN** run 1 executed 37 consecutive tool calls that all ended `completed` and run 2 starts
- **THEN** the summary contains one line `Tool: 37 calls completed` instead of 37 lines

#### Scenario: Failed call breaks a streak
- **WHEN** a failed tool call lies between two groups of completed calls
- **THEN** the summary has one collapsed line per group and the failed call as its own line between them

#### Scenario: User messages kept at the cap
- **WHEN** the entries exceed the size cap and include tool lines and agent messages older than a user message
- **THEN** those tool lines and agent messages are dropped before the user message

#### Scenario: Old history beyond a row window
- **WHEN** a task has more than 500 transcript events and its entries fit the cap
- **THEN** the summary contains the entries of its oldest events

#### Scenario: Reconnect mid-run
- **WHEN** the sandbox of run 2 reconnects mid-run
- **THEN** the summary in the new `Assign` contains run 1's events and none of run 2's

#### Scenario: Run ended without a report
- **WHEN** run 1 ended through a lost sandbox or a hard timeout and run 2 starts
- **THEN** the summary includes run 1's events

#### Scenario: Hidden blocks
- **WHEN** a user message has a hidden block
- **THEN** the summary does not contain its text
