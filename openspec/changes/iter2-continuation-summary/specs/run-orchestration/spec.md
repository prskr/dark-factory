## MODIFIED Requirements

### Requirement: Continuation across runs
Every run after the first SHALL carry the harness session id reported by the previous run, and a summary of the task's transcript for harnesses that can't resume the session. The summary SHALL contain the task title and description, the visible text of user messages (blocks marked `hidden` are left out), the agent text messages and one line per tool call (final title and status), in transcript order, within a fixed size cap; when the cap is exceeded, the oldest entries are dropped first. The control plane SHALL store the summary per task together with the id of the last transcript event it covers, update it incrementally from the events after that id when a run ends, and, when assigning a run, first fold any events after that id and then read the stored summary.

#### Scenario: Follow-up run
- **WHEN** run 2 of a task starts after run 1 reported session id `S`
- **THEN** `Assign.continuation.harness_session_id` is `S` and `summary` is not empty

#### Scenario: Tool calls in the summary
- **WHEN** run 1 executed a tool call titled `cargo test` that ended `failed` and run 2 starts
- **THEN** `Assign.continuation.summary` contains a line with `cargo test` and `failed`

#### Scenario: Incremental update
- **WHEN** run 1 ends and the stored summary covers events up to `E1`, and run 2 ends later
- **THEN** the stored summary covers the last event of run 2, and only events after `E1` were read to update it

#### Scenario: Old history survives the size cap
- **WHEN** a task has more than 500 transcript events and its summary was updated at the end of every run
- **THEN** the entries dropped by the size cap are the oldest ones, regardless of how many events a single run produced

#### Scenario: Run without a stored summary
- **WHEN** run 2 starts and no summary row exists for the task (for example a task from before this change)
- **THEN** the control plane builds the summary from the whole transcript, stores it, and assigns it

#### Scenario: Stale summary
- **WHEN** run 1 ended without updating the summary (failed update, lost sandbox or hard timeout) and run 2 starts
- **THEN** the assigned summary includes run 1's events

#### Scenario: Hidden blocks
- **WHEN** a user message has a hidden block
- **THEN** the summary, whether updated incrementally or rebuilt, does not contain its text
