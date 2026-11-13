# Spec Delta

## MODIFIED Requirements

### Requirement: Runs start for queued tasks
A task is due when its status is `queued` or `rate_limited`, it has no non-terminal run and its resume time is unset or in the past. While a run slot is free (see "Concurrent run limit"), the control plane SHALL start a run for a due task within one scheduler tick (`scheduling.tickMillis`, default 5 seconds) of it becoming due or of the slot becoming free, on any replica, without ever starting two concurrent runs for one task. Starting a run SHALL clear the task's resume time.

#### Scenario: New task
- **WHEN** task `DF-1` is created
- **THEN** run 1 of `DF-1` is created in state `pending` and the task status becomes `running`

#### Scenario: Two replicas
- **WHEN** two control plane replicas see the same queued task at the same time
- **THEN** exactly one run is created

#### Scenario: Due rate-limited task
- **WHEN** task `DF-2` is `rate_limited` with a resume time that has passed
- **THEN** a run starts without a message from a board member

#### Scenario: Not yet due
- **WHEN** task `DF-3` is `queued` with a resume time 5 minutes ahead
- **THEN** no run starts until that time

### Requirement: Run provisioning
Starting a run SHALL ensure the task volume exists, then provision a sandbox for the run with the project's image and labels for run, task, project and persona, and move the run to `provisioning`, then `connecting` once the sandbox reports its pod IP. A run in `pending` or `provisioning` for more than 1 minute without a sandbox SHALL be provisioned again on the next tick; volume and sandbox creation SHALL be idempotent.

#### Scenario: Provisioning fails
- **WHEN** the provider rejects the sandbox for a reason other than capacity or a temporarily unresolvable cluster target (e.g. invalid image reference)
- **THEN** the run ends with outcome `provisioning_failed` and the task is resumed or failed per "Automatic resume"

#### Scenario: Quota exceeded
- **WHEN** the provider rejects the sandbox because a quota is exceeded
- **THEN** the run ends `provisioning_failed` with detail `capacity`, the task becomes `queued` with a resume time `scheduling.resumeBackoffSeconds` ahead (default 2 minutes), and the resume count is unchanged

#### Scenario: Provisioning interrupted
- **WHEN** a replica stops after creating run 3 of `DF-7` and before creating its Sandbox
- **THEN** another tick provisions the run within 2 minutes and the run connects normally

### Requirement: Automatic resume after infrastructure failures
When a run ends with outcome `sandbox_lost`, `connect_timeout` or `provisioning_failed` (other than detail `capacity`) and the task's resume count is below `runs.maxAutoResumes` (default 3), the control plane SHALL set the task to `queued` with a resume time `scheduling.resumeBackoffSeconds` (default 2 minutes) × (resume count + 1) ahead and increment the resume count. Otherwise the task SHALL become `failed`. Runs ending with outcome `failed` SHALL NOT be resumed automatically.

#### Scenario: Sandbox lost is resumed
- **WHEN** the sandbox pod of `DF-8` is deleted during its first run at 02:00
- **THEN** the run ends `sandbox_lost`, `DF-8` is `queued` with resume time 02:02 and resume count 1, and run 2 starts at 02:02

#### Scenario: Resume cap reached
- **WHEN** `runs.maxAutoResumes` is 3 and the fourth consecutive run of `DF-8` ends `connect_timeout`
- **THEN** `DF-8` becomes `failed` and no run starts until a message is queued

#### Scenario: Harness crash is not resumed
- **WHEN** a run reports `RUN_OUTCOME_FAILED`
- **THEN** the task becomes `failed` and its resume time stays unset

## ADDED Requirements

### Requirement: Configurable scheduling waits
The control plane SHALL read an optional configuration section `scheduling`: `tickMillis` (default 5000, scheduler tick, lost-sandbox sweep and job dispatcher poll), `resumeBackoffSeconds` (default 120, automatic resume base and `capacity` requeue) and `forgeRetryBaseSeconds` (default 30, first retry of PR sync and mark-draft). Without the section, behavior SHALL be unchanged. A value of 0 or above its limit SHALL stop startup with an error naming the setting.

#### Scenario: Section unset
- **WHEN** the control plane starts without a `scheduling` section
- **THEN** the scheduler ticks and the job dispatcher polls every 5 s, a `sandbox_lost` run is resumed after 2 minutes and a failed PR sync is first retried after 30 s

#### Scenario: Short tick
- **WHEN** `scheduling.tickMillis` is 500 and a PR sync job's backoff elapses without a new job notification
- **THEN** the dispatcher picks the job up within about 500 ms

#### Scenario: Short backoff
- **WHEN** `scheduling.resumeBackoffSeconds` is 5 and the sandbox of `DF-8` is lost during its first run
- **THEN** `DF-8` is `queued` with a resume time 5 s ahead and run 2 starts within one tick after it

#### Scenario: Invalid value
- **WHEN** the configuration sets `scheduling.tickMillis: 0`
- **THEN** the control plane doesn't start and the error names `scheduling.tickMillis`
