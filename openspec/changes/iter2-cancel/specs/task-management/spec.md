# Spec Delta

## MODIFIED Requirements

### Requirement: Task status vocabulary
A task SHALL have exactly one status: `queued` (pending work, no active run), `running` (a run is active), `waiting` (the last turn ended; waits for a human, the run may still be alive in its keep-alive window), `blocked` (last run waited for an answer or approval that didn't arrive within the idle deadline), `in_review` (the agent called `complete_task`; the PR waits for review), `completed` (a human marked the task done), `cancelled` (a human cancelled the task), `rate_limited`, `limit_reached` or `failed`.

#### Scenario: Status is always one of the defined values
- **WHEN** any component writes a task status outside this set
- **THEN** the database rejects the write

#### Scenario: Message on a blocked task
- **WHEN** a message is queued on a task in status `blocked`
- **THEN** the task status becomes `queued`

### Requirement: Mark a task done
A board member SHALL be able to mark a task done unless it is `queued`, `running` or already `completed`. The task SHALL become `completed`; if it has a run in `awaiting_input`, the control plane SHALL cancel that run. The change SHALL be audited with the actor. If the task has a PR, the control plane SHALL mark it ready for review after the status change, without a summary; a forge failure is logged and doesn't undo the status. Undelivered messages stay queued; a new message reopens the task as for any `completed` task.

#### Scenario: Close a waiting task
- **WHEN** a board member marks `DF-1` in status `waiting` done
- **THEN** `DF-1` becomes `completed` and an audit entry names the board member

#### Scenario: Close a waiting task with a PR
- **WHEN** a board member marks `DF-1` in status `waiting` with a draft PR done
- **THEN** `DF-1` becomes `completed`, the PR is no longer a draft and gets no new comment

#### Scenario: Mark done without a PR
- **WHEN** a board member marks a task without a PR done
- **THEN** the task becomes `completed` and the forge is not called

#### Scenario: Not while working
- **WHEN** a board member tries to mark a `running` task done
- **THEN** the request is rejected and the status does not change

#### Scenario: Not twice
- **WHEN** a board member tries to mark a `completed` task done
- **THEN** the request is rejected as a conflict and no audit entry is written

## ADDED Requirements

### Requirement: Cancelling a task
A board member SHALL be able to cancel a task in any status except `completed` and `cancelled` (including `in_review`). Cancelling SHALL set the status `cancelled`, stop its active run, mark its undelivered messages as discarded and write an audit entry `task.cancel`. The task's PR and linked issue SHALL NOT be changed.

#### Scenario: Cancel a running task
- **WHEN** a board member cancels `DF-9` while its run is active and one message is undelivered
- **THEN** the run is stopped, the message is discarded, `DF-9` is `cancelled` and its draft PR is still open

#### Scenario: Cancel a completed task
- **WHEN** a board member tries to cancel a completed task
- **THEN** the action is rejected and nothing changes

### Requirement: Reopening a cancelled task
Queueing a message on a cancelled task SHALL set it to `queued`. Discarded messages SHALL NOT be delivered.

#### Scenario: Reopen
- **WHEN** a board member queues "please continue after all" on cancelled task `DF-9`
- **THEN** `DF-9` becomes `queued` and the next run receives only the new message

#### Scenario: Reopen before the stopped run ended
- **WHEN** a message is queued on cancelled task `DF-9` while its stopped run is still finalizing
- **THEN** `DF-9` stays `queued` when that run's `RunReport` arrives, and a new run starts with the message after the old run ended
