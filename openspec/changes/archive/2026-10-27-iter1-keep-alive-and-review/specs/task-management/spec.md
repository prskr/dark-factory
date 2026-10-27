## MODIFIED Requirements

### Requirement: Task status vocabulary
A task SHALL have exactly one status: `queued` (pending work, no active run), `running` (a turn is in progress or about to start), `waiting` (the last turn ended; waits for a human, the run may still be alive in its keep-alive window), `in_review` (the agent called `complete_task`; the PR waits for review), `completed` (a human marked the task done), `rate_limited`, `limit_reached` or `failed`. `waiting`, `in_review` and `limit_reached` form the "Waiting" group of the task list filters; `completed` and `failed` the "Done" group.

#### Scenario: Status is always one of the defined values
- **WHEN** any component writes a task status outside this set
- **THEN** the database rejects the write

### Requirement: Queued messages
A human SHALL be able to queue a message on any task at any time. Each message SHALL get a unique prompt id, its author and queue time, and stays undelivered until a transcript `UserMessage` event with that prompt id is persisted. If the task has an active run that is not `finalizing`, the status SHALL become `running`; otherwise it SHALL become `queued`.

#### Scenario: Message on a waiting task
- **WHEN** a board member queues "also add a test for X" on a task in status `waiting` without an active run
- **THEN** an undelivered prompt is stored and the task status becomes `queued`

#### Scenario: Message on a failed task
- **WHEN** a message is queued on a task in status `failed`
- **THEN** the task status becomes `queued`

#### Scenario: Message on a task with an idle run
- **WHEN** a message is queued on a `waiting` task whose run is `awaiting_input`
- **THEN** the task status becomes `running` and no new run is scheduled

#### Scenario: Message on a task whose run is finalizing
- **WHEN** a message is queued on a task whose run is `finalizing`
- **THEN** the task status becomes `queued` and the next run starts once that run has ended

#### Scenario: Message on a task in review
- **WHEN** a message is queued on a task in status `in_review`
- **THEN** the task status becomes `queued` and a new run starts

## ADDED Requirements

### Requirement: Mark a task done
A board member SHALL be able to mark a task done unless it is `queued` or `running`. The task SHALL become `completed`; if it has a run in `awaiting_input`, the control plane SHALL cancel that run. The change SHALL be audited with the actor. Undelivered messages stay queued; a new message reopens the task as for any `completed` task.

#### Scenario: Close a waiting task
- **WHEN** a board member marks `DF-1` in status `waiting` done
- **THEN** `DF-1` becomes `completed` and an audit entry names the board member

#### Scenario: Not while working
- **WHEN** a board member tries to mark a `running` task done
- **THEN** the request is rejected and the status does not change
