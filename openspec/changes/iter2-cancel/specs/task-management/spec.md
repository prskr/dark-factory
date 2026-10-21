# Spec Delta

## MODIFIED Requirements

### Requirement: Task status vocabulary
A task SHALL have exactly one status: `queued` (pending work, no active run), `running` (a run is active), `waiting` (last run ended its turn; waits for a human), `blocked` (last run waited for an answer or approval that didn't arrive within the keep-alive window), `completed` (agent called `complete_task`), `cancelled` (a human cancelled the task), `rate_limited`, `limit_reached` or `failed`.

#### Scenario: Status is always one of the defined values
- **WHEN** any component writes a task status outside this set
- **THEN** the database rejects the write

#### Scenario: Message on a blocked task
- **WHEN** a message is queued on a task in status `blocked`
- **THEN** the task status becomes `queued`

## ADDED Requirements

### Requirement: Cancelling a task
A board member SHALL be able to cancel a task in any status except `completed` and `cancelled`. Cancelling SHALL set the status `cancelled`, stop its active run, mark its undelivered messages as discarded and write an audit entry `task.cancel`. The task's PR and linked issue SHALL NOT be changed.

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
