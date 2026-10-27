# Spec Delta

## MODIFIED Requirements

### Requirement: Task status vocabulary
A task SHALL have exactly one status: `queued` (pending work, no active run), `running` (a run is active), `waiting` (the last turn ended; waits for a human, the run may still be alive in its keep-alive window), `blocked` (last run waited for an answer or approval that didn't arrive within the idle deadline), `in_review` (the agent called `complete_task`; the PR waits for review), `completed` (a human marked the task done), `rate_limited`, `limit_reached` or `failed`.

#### Scenario: Status is always one of the defined values
- **WHEN** any component writes a task status outside this set
- **THEN** the database rejects the write

#### Scenario: Message on a blocked task
- **WHEN** a message is queued on a task in status `blocked`
- **THEN** the task status becomes `queued`
