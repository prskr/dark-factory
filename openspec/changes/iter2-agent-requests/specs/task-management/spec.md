# Spec Delta

## MODIFIED Requirements

### Requirement: Task status vocabulary
A task SHALL have exactly one status: `queued` (pending work, no active run), `running` (a run is active), `waiting` (last run ended its turn; waits for a human), `blocked` (last run waited for an answer or approval that didn't arrive within the keep-alive window), `completed` (agent called `complete_task`), `rate_limited`, `limit_reached` or `failed`.

#### Scenario: Status is always one of the defined values
- **WHEN** any component writes a task status outside this set
- **THEN** the database rejects the write

#### Scenario: Message on a blocked task
- **WHEN** a message is queued on a task in status `blocked`
- **THEN** the task status becomes `queued`
