# Spec Delta

## Purpose

Decides when a task gets a run, drives each run through its states, and turns the run's end into the task's status — sequential runs per task, sandboxes that never outlive their run (ADR-0016).

## ADDED Requirements

### Requirement: Runs start for queued tasks
The control plane SHALL start a run for every task with status `queued` and no non-terminal run, within 5 seconds of the task becoming queued, on any replica, without ever starting two concurrent runs for one task.

#### Scenario: New task
- **WHEN** task `DF-1` is created
- **THEN** run 1 of `DF-1` is created in state `pending` and the task status becomes `running`

#### Scenario: Two replicas
- **WHEN** two control plane replicas see the same queued task at the same time
- **THEN** exactly one run is created

### Requirement: Run provisioning
Starting a run SHALL ensure the task volume exists, then provision a sandbox for the run with the project's image and labels for run, task, project and persona, and move the run to `provisioning`, then `connecting` once the sandbox reports its pod IP.

#### Scenario: Provisioning fails
- **WHEN** the provider rejects the sandbox (e.g. quota exceeded)
- **THEN** the run ends with outcome `provisioning_failed` and the task status becomes `failed`

### Requirement: Assign content
`Assign` SHALL contain the task reference, the workspace (project repo URL, base branch, task and WIP branch, the instance's commit identity), the persona's system prompt and ACP harness (adapter command, non-secret env, model, settings), the factory tools v1, budgets from instance configuration and the continuation.

#### Scenario: Assign for DF-1
- **WHEN** run 1 of `DF-1` connects
- **THEN** `Assign.workspace.task_branch` is `factory/task/DF-1` and `Assign.tools` contains exactly the four factory tools v1

### Requirement: Continuation across runs
Every run after the first SHALL carry the harness session id reported by the previous run, and a summary of the task's transcript for harnesses that can't resume the session.

#### Scenario: Follow-up run
- **WHEN** run 2 of a task starts after run 1 reported session id `S`
- **THEN** `Assign.continuation.harness_session_id` is `S` and `summary` is not empty

### Requirement: Run phases
The run state SHALL follow `pending → provisioning → connecting → preparing → running → finalizing → succeeded | failed | cancelled`, advanced by the provider events and the runtime's `PhaseChanged` and `RunReport` messages.

#### Scenario: Phase reported
- **WHEN** the runtime reports `RUN_PHASE_RUNNING`
- **THEN** the run state becomes `running` and a `run_changed` notification is published

### Requirement: Timeouts and lost sandboxes
The control plane SHALL fail a run that doesn't connect within the configured connect timeout (outcome `connect_timeout`), SHALL send `Cancel` when the wall-clock budget plus grace is exceeded, and SHALL fail a non-terminal run whose sandbox disappears (outcome `sandbox_lost`). Failed runs' sandboxes SHALL be torn down.

#### Scenario: Sandbox never connects
- **WHEN** a sandbox doesn't open the stream within the connect timeout
- **THEN** the run fails with `connect_timeout`, the sandbox is deleted and the task becomes `failed`

#### Scenario: Pod deleted mid-run
- **WHEN** the sandbox pod of a running run is deleted
- **THEN** the run fails with `sandbox_lost` and the task volume is kept

### Requirement: Run end maps to task status
On `RunReport` the control plane SHALL store outcome, detail, retry time, session id and workspace state, delete the sandbox, and set the task status: `completed`→`completed`, `end_turn`→`waiting`, `limit_reached`→`limit_reached`, `rate_limited`→`rate_limited`, `cancelled`→`waiting`, `failed`→`failed`. If undelivered prompts exist and the outcome is neither `failed` nor `rate_limited`, the task SHALL become `queued` instead.

#### Scenario: Rate limit
- **WHEN** a run reports `RUN_OUTCOME_RATE_LIMITED` with a retry time
- **THEN** the run is `succeeded` with outcome `rate_limited` and the retry time, the task is `rate_limited` and no new run starts until a message is queued

#### Scenario: Message arrived during finalize
- **WHEN** a message is queued while a run is finalizing and the run ends with `end_turn`
- **THEN** the task becomes `queued` and the next run starts with that message

### Requirement: Prompts during a run
A message queued while a run is active SHALL be sent to the runtime as a `Prompt` by the replica holding the run's stream.

#### Scenario: Follow-up during a turn
- **WHEN** a message is queued on a running task
- **THEN** the runtime receives it as a `Prompt` and the transcript later shows it as a `UserMessage`
