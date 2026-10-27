# Spec Delta

## MODIFIED Requirements

### Requirement: Run phases
The run state SHALL follow `pending → provisioning → connecting → preparing → running ⇄ awaiting_input → finalizing → succeeded | failed | cancelled`, advanced by the provider events and the runtime's `PhaseChanged` and `RunReport` messages. The terminal state records how the run ended technically; the outcome (`completed`, `end_turn`, `blocked`, `rate_limited`, …) is stored separately, so a `blocked` run ends in state `succeeded` with outcome `blocked`.

#### Scenario: Phase reported
- **WHEN** the runtime reports `RUN_PHASE_RUNNING`
- **THEN** the run state becomes `running` and a `run_changed` notification is published

#### Scenario: Awaiting input reported
- **WHEN** the runtime reports `RUN_PHASE_AWAITING_INPUT`
- **THEN** the run state becomes `awaiting_input` and the task detail shows that the run waits for an answer

#### Scenario: Blocked run is terminal
- **WHEN** a run in state `finalizing` reports `RUN_OUTCOME_BLOCKED`
- **THEN** the run state becomes `succeeded` with outcome `blocked` and no further phase changes are accepted for it

### Requirement: Run end maps to task status
On `RunReport` the control plane SHALL store outcome, detail, retry time, session id and workspace state, delete the sandbox, and set the task status: `completed`→`in_review`, `end_turn`→`waiting`, `blocked`→`blocked`, `limit_reached`→`limit_reached`, `rate_limited`→`rate_limited`, `cancelled`→`waiting`, `failed`→`failed`. If undelivered prompts exist and the outcome is neither `failed` nor `rate_limited`, the task SHALL become `queued` instead.

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

## ADDED Requirements

### Requirement: Deadline for blocking questions
For a run whose last turn had a blocking request, the control plane SHALL apply the idle deadline of "Idle deadline" (`iter1-keep-alive-and-review`) and send `Release` with outcome `BLOCKED` expected. The wall-clock budget includes time spent awaiting input.

#### Scenario: Default window
- **WHEN** a run awaits an answer in an instance without `runs.keepAliveSeconds`
- **THEN** the control plane sends `Release` after 10 minutes and the run ends `blocked`
