# Spec Delta

## MODIFIED Requirements

### Requirement: Task status vocabulary
A task SHALL have exactly one status: `queued` (work pending, no active run), `running` (a turn is in progress or starting), `waiting` (waits for a human; the run may still be in its keep-alive window), `blocked` (an answer or approval was not given within the idle deadline), `in_review` (the agent called `complete_task`), `completed` (marked done by a human, or its PR was merged), `cancelled` (cancelled by a human), `rate_limited`, `limit_reached` or `failed`.

#### Scenario: Status is always one of the defined values
- **WHEN** any component writes a task status outside this set
- **THEN** the database rejects the write

#### Scenario: Message on a blocked task
- **WHEN** a message is queued on a task in status `blocked`
- **THEN** the task status becomes `queued`

## ADDED Requirements

### Requirement: Merged PR completes the task
When the forge reports the task's PR as merged, the task SHALL become `completed` unless it is `completed` or `cancelled`, SHALL remember the merge time, and SHALL stop its active run. An audit entry `task.done` SHALL name the system principal with reason `pr.merged` and the merging login. Undelivered messages stay queued, as for a task marked done by hand.

#### Scenario: Merged while in review
- **WHEN** the PR of `DF-1` in status `in_review` is merged by `alice`
- **THEN** `DF-1` becomes `completed` and the audit entry names reason `pr.merged` and `alice`

#### Scenario: Merged while running
- **WHEN** the PR of `DF-2` is merged while its run is in a turn
- **THEN** the run is stopped and `DF-2` is `completed`, not `cancelled`

#### Scenario: Merged after cancel
- **WHEN** the PR of cancelled task `DF-3` is merged
- **THEN** `DF-3` stays `cancelled`, and the merge time is remembered

#### Scenario: Redelivered merge event
- **WHEN** the merge event of `DF-1` arrives twice
- **THEN** `DF-1` is `completed` once and one audit entry exists

### Requirement: Volume of a merged task
The retention scan SHALL delete the volume of a task without a non-terminal run whose PR was merged, without waiting for `volumes.retainAfterClose`.

#### Scenario: Merged an hour ago
- **WHEN** the PR of `DF-1` was merged and its stopped run has ended
- **THEN** the next retention scan deletes the volume of `DF-1`
