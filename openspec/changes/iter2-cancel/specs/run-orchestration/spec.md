# Spec Delta

## MODIFIED Requirements

### Requirement: Run end maps to task status
On `RunReport` the control plane SHALL store outcome, detail, retry time, session id and workspace state, delete the sandbox, and set the task status: `completed`→`completed`, `end_turn`→`waiting`, `blocked`→`blocked`, `limit_reached`→`limit_reached`, `rate_limited`→`rate_limited`, `cancelled`→`waiting`, `failed`→`failed`. If undelivered prompts exist and the outcome is neither `failed` nor `rate_limited`, the task SHALL become `queued` instead; for a run stopped by a human, only prompts queued after the stop count. A task whose status is still `cancelled` SHALL stay `cancelled`.

#### Scenario: Rate limit
- **WHEN** a run reports `RUN_OUTCOME_RATE_LIMITED` with a retry time
- **THEN** the run is `succeeded` with outcome `rate_limited` and the retry time, the task is `rate_limited` and no new run starts until a message is queued

#### Scenario: Message arrived during finalize
- **WHEN** a message is queued while a run is finalizing and the run ends with `end_turn`
- **THEN** the task becomes `queued` and the next run starts with that message

#### Scenario: Blocked run
- **WHEN** a run reports `RUN_OUTCOME_BLOCKED`
- **THEN** the task becomes `blocked` and no new run starts until a message is queued

#### Scenario: Completed with an undelivered message
- **WHEN** a human queues a message during the turn in which the agent calls `complete_task`
- **THEN** the run ends `completed`, the PR is marked ready, and the task becomes `queued` so the next run addresses the message

#### Scenario: Stopped with an undelivered message
- **WHEN** a message queued before the stop is still undelivered when the stopped run's `RunReport` arrives
- **THEN** the task becomes `waiting` and no new run starts until another message is queued

#### Scenario: Reopened while the stopped run finalizes
- **WHEN** a task is cancelled, a board member queues a message before the stopped run's `RunReport` arrives, and the run then reports `RUN_OUTCOME_CANCELLED`
- **THEN** the task stays `queued` and the next run starts with that message once the stopped run has ended

## ADDED Requirements

### Requirement: Stopping a run
A board member SHALL be able to stop a non-terminal run. A connected run SHALL receive `Cancel`; a run that isn't connected yet SHALL have its sandbox torn down and end with outcome `cancelled`. Stopping SHALL write an audit entry `run.stop`.

#### Scenario: Stop a running run
- **WHEN** a board member stops run 2 of `DF-9` during a turn
- **THEN** the runtime receives `Cancel`, finalizes and reports `RUN_OUTCOME_CANCELLED`, and `DF-9` becomes `waiting`

#### Scenario: Stop while provisioning
- **WHEN** a board member stops a run in state `provisioning`
- **THEN** the sandbox is deleted, the run ends `cancelled` and the task volume is kept

### Requirement: Stop timeout
If no `RunReport` arrives within 2 minutes after `Cancel` was sent for a human stop, the control plane SHALL tear down the sandbox and end the run `cancelled` with detail `stop-timeout`, keeping the task volume.

#### Scenario: Runtime hangs
- **WHEN** the runtime doesn't report within 2 minutes after a stop
- **THEN** the sandbox is deleted and the run ends `cancelled` with detail `stop-timeout`
