# Spec Delta

## MODIFIED Requirements

### Requirement: Runs start for queued tasks
A task is due when its status is `queued` or `rate_limited`, it has no non-terminal run and its resume time is unset or in the past. At most `runs.maxConcurrent` runs (default 4) SHALL be non-terminal at a time, counted across all replicas. While a slot is free, the control plane SHALL start a run for a due task within 5 seconds of it becoming due or of the slot becoming free, on any replica, without ever starting two concurrent runs for one task. Due tasks beyond the limit SHALL keep their status until a slot frees and SHALL then start oldest resume time first. Starting a run SHALL clear the task's resume time.

#### Scenario: New task
- **WHEN** task `DF-1` is created
- **THEN** run 1 of `DF-1` is created in state `pending` and the task status becomes `running`

#### Scenario: Two replicas
- **WHEN** two control plane replicas see the same queued task at the same time
- **THEN** exactly one run is created

#### Scenario: Two replicas, one slot
- **WHEN** `runs.maxConcurrent` is 1 and two replicas tick at the same time with `DF-1` and `DF-2` both due
- **THEN** exactly one run is created

#### Scenario: Due rate-limited task
- **WHEN** task `DF-2` is `rate_limited` with a resume time that has passed
- **THEN** a run starts without a queued message

#### Scenario: Not yet due
- **WHEN** task `DF-3` is `queued` with a resume time 5 minutes ahead
- **THEN** no run starts until that time

#### Scenario: Limit reached
- **WHEN** `runs.maxConcurrent` is 2, two runs are active and `DF-4` becomes queued
- **THEN** `DF-4`'s run starts only after one of the active runs ended

#### Scenario: Oldest first
- **WHEN** one slot frees and `DF-5` (due since 02:00) and `DF-6` (due since 03:00) are waiting
- **THEN** `DF-5`'s run starts first

### Requirement: Run provisioning
Starting a run SHALL ensure the task volume exists, then provision a sandbox for the run with the project's image and labels for run, task, project and persona, and move the run to `provisioning`, then `connecting` once the sandbox reports its pod IP. A run in `pending` or `provisioning` for more than 1 minute without a sandbox SHALL be provisioned again on the next tick; volume and sandbox creation SHALL be idempotent.

#### Scenario: Provisioning fails
- **WHEN** the provider rejects the sandbox for a reason other than capacity (e.g. invalid image reference)
- **THEN** the run ends with outcome `provisioning_failed` and the task is resumed or failed per "Automatic resume"

#### Scenario: Quota exceeded
- **WHEN** the provider rejects the sandbox because a quota is exceeded
- **THEN** the run ends `provisioning_failed` with detail `capacity`, the task becomes `queued` with a resume time 2 minutes ahead, and the resume count is unchanged

#### Scenario: Provisioning interrupted
- **WHEN** a replica stops after creating run 3 of `DF-7` and before creating its Sandbox
- **THEN** another tick provisions the run within 2 minutes and the run connects normally

### Requirement: Run end maps to task status
On `RunReport` the control plane SHALL store outcome, detail, retry time, session id and workspace state, delete the sandbox, reset the task's resume count, and set the task status: `completed`→`completed`, `end_turn`→`waiting`, `blocked`→`blocked`, `limit_reached`→`limit_reached`, `rate_limited`→`rate_limited` with the resume time set to the reported retry time or one hour ahead when none was reported, `cancelled`→`waiting`, `failed`→`failed`. If undelivered prompts exist and the outcome is neither `failed` nor `rate_limited`, the task SHALL become `queued` instead; for a run stopped by a human, only prompts queued after the stop count. A task whose status is still `cancelled` SHALL stay `cancelled`.

#### Scenario: Rate limit
- **WHEN** a run reports `RUN_OUTCOME_RATE_LIMITED` with retry time 04:30
- **THEN** the run is `succeeded` with outcome `rate_limited`, the task is `rate_limited` with resume time 04:30, and the next run starts at 04:30 without a message

#### Scenario: Unknown retry time
- **WHEN** a run reports `RUN_OUTCOME_RATE_LIMITED` without a retry time at 02:00
- **THEN** the task's resume time is 03:00

#### Scenario: Report resets the counter
- **WHEN** a task resumed twice automatically ends a run with `RUN_OUTCOME_END_TURN`
- **THEN** its resume count is 0

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

### Requirement: Automatic resume after infrastructure failures
When a run ends with outcome `sandbox_lost`, `connect_timeout` or `provisioning_failed` (other than detail `capacity`) and the task's resume count is below `runs.maxAutoResumes` (default 3), the control plane SHALL set the task to `queued` with a resume time 2 minutes × (resume count + 1) ahead and increment the resume count. Otherwise the task SHALL become `failed`. Runs ending with outcome `failed` SHALL NOT be resumed automatically.

#### Scenario: Sandbox lost is resumed
- **WHEN** the sandbox pod of `DF-8` is deleted during its first run at 02:00
- **THEN** the run ends `sandbox_lost`, `DF-8` is `queued` with resume time 02:02 and resume count 1, and run 2 starts at 02:02

#### Scenario: Resume cap reached
- **WHEN** `runs.maxAutoResumes` is 3 and the fourth consecutive run of `DF-8` ends `connect_timeout`
- **THEN** `DF-8` becomes `failed` and no run starts until a message is queued

#### Scenario: Harness crash is not resumed
- **WHEN** a run reports `RUN_OUTCOME_FAILED`
- **THEN** the task becomes `failed` and its resume time stays unset
