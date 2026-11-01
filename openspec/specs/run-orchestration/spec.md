# run-orchestration Specification

## Purpose

Decides when a task gets a run, drives each run through its states, and turns the run's end into the task's status — sequential runs per task, sandboxes that never outlive their run (ADR-0016).

## Requirements

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
`Assign` SHALL contain the task reference, the workspace (repo URL through the gateway's Git proxy, base branch, task and WIP branch, the instance's commit identity), the persona's system prompt and ACP harness (adapter command, non-secret env, model, settings), the factory tools v2, budgets from instance configuration and the continuation. The adapter command SHALL be the project's, otherwise the instance's default.

#### Scenario: Assign for DF-1
- **WHEN** run 1 of `DF-1` connects
- **THEN** `Assign.workspace.task_branch` is `factory/task/DF-1` and `Assign.tools` contains exactly the six factory tools v2

#### Scenario: Default adapter command
- **WHEN** a run of a project without an adapter command connects
- **THEN** `Assign.harness.acp.command` is the instance's `harness.claudeCode.adapterCommand`

#### Scenario: Project adapter command
- **WHEN** a run of a project with adapter command `/opt/harness/bin/fake-acp-agent` connects
- **THEN** `Assign.harness.acp.command` is `["/opt/harness/bin/fake-acp-agent"]`

#### Scenario: Repository through the proxy
- **WHEN** run 1 of `DF-1` connects in an instance whose gateway Service is `df-gateway.df.svc` with Git port 8443
- **THEN** `Assign.workspace.repo_url` is `https://df-gateway.df.svc:8443/git/DF-1.git` and contains no credentials

### Requirement: Continuation across runs
Every run after the first SHALL carry the harness session id reported by the previous run, and a summary of the task's transcript for harnesses that can't resume the session.

#### Scenario: Follow-up run
- **WHEN** run 2 of a task starts after run 1 reported session id `S`
- **THEN** `Assign.continuation.harness_session_id` is `S` and `summary` is not empty

### Requirement: Run phases
The run state SHALL follow `pending → provisioning → connecting → preparing → running → finalizing → succeeded | failed | cancelled`, advanced by the provider events and the runtime's `PhaseChanged` and `RunReport` messages. Between turns a run SHALL move `running → awaiting_input → running` on `RUN_PHASE_AWAITING_INPUT` and `RUN_PHASE_RUNNING`. Once the control plane has sent `Release` or `Cancel` to a run, the run SHALL be `finalizing`.

#### Scenario: Phase reported
- **WHEN** the runtime reports `RUN_PHASE_RUNNING`
- **THEN** the run state becomes `running` and a `run_changed` notification is published

#### Scenario: Awaiting input reported
- **WHEN** the runtime reports `RUN_PHASE_AWAITING_INPUT`
- **THEN** the run state becomes `awaiting_input` and the task detail shows that the run waits for an answer

### Requirement: Task status while a run awaits input
When a run becomes `awaiting_input`, its task SHALL become `waiting` unless undelivered messages exist, in which case it stays `running`; when the run is `running` again, the task SHALL be `running`. The phase update and `queue_message` SHALL serialize on the task row, so neither overwrites the other.

#### Scenario: Idle after a turn
- **WHEN** the runtime reports `RUN_PHASE_AWAITING_INPUT` and no message is undelivered
- **THEN** the run state becomes `awaiting_input`, the sandbox keeps running and the task status becomes `waiting`

#### Scenario: Message queued as the turn ends
- **WHEN** a message is queued while the turn ends and the `RUN_PHASE_AWAITING_INPUT` report is processed afterwards
- **THEN** the run is `awaiting_input`, the task stays `running` and the message is forwarded

### Requirement: Run state and outcome
The terminal state SHALL record how the run ended technically; the outcome (`completed`, `end_turn`, `blocked`, `rate_limited`, …) SHALL be stored separately, so a `blocked` run ends in state `succeeded` with outcome `blocked`.

#### Scenario: Blocked run is terminal
- **WHEN** a run in state `finalizing` reports `RUN_OUTCOME_BLOCKED`
- **THEN** the run state becomes `succeeded` with outcome `blocked` and no further phase changes are accepted for it

### Requirement: Timeouts and lost sandboxes
The control plane SHALL fail a run that doesn't connect within the configured connect timeout (outcome `connect_timeout`), SHALL send `Cancel` when the wall-clock budget plus grace is exceeded, and SHALL fail a non-terminal run whose sandbox disappears (outcome `sandbox_lost`). Failed runs' sandboxes SHALL be torn down.

#### Scenario: Sandbox never connects
- **WHEN** a sandbox doesn't open the stream within the connect timeout
- **THEN** the run fails with `connect_timeout`, the sandbox is deleted and the task becomes `failed`

#### Scenario: Pod deleted mid-run
- **WHEN** the sandbox pod of a running run is deleted
- **THEN** the run fails with `sandbox_lost` and the task volume is kept

### Requirement: Run end maps to task status
On `RunReport` the control plane SHALL store outcome, detail, retry time, session id and workspace state, delete the sandbox and set the task status: `completed`→`in_review`, `end_turn` and `cancelled`→`waiting`, any other outcome to the status of the same name. Undelivered prompts SHALL make the task `queued` instead unless the outcome is `failed` or `rate_limited`, even if a human marked it done; without them, a task marked done (`completed`) SHALL keep its status, whatever the outcome.

#### Scenario: Rate limit
- **WHEN** a run reports `RUN_OUTCOME_RATE_LIMITED` with a retry time
- **THEN** the run is `succeeded` with outcome `rate_limited` and the retry time, the task is `rate_limited` and no new run starts until a message is queued

#### Scenario: Message arrived during finalize
- **WHEN** a message is queued while a run is finalizing and the run ends with `end_turn`
- **THEN** the task becomes `queued` and the next run starts with that message

#### Scenario: Agent completes
- **WHEN** a run ends with outcome `completed`
- **THEN** the task becomes `in_review`

#### Scenario: Marked done while idle
- **WHEN** a board member marks a task done while its run is `awaiting_input`
- **THEN** the run is cancelled, its sandbox deleted, and the task stays `completed`

#### Scenario: Blocked run
- **WHEN** a run reports `RUN_OUTCOME_BLOCKED`
- **THEN** the task becomes `blocked` and no new run starts until a message is queued

#### Scenario: Completed with an undelivered message
- **WHEN** a human queues a message during the turn in which the agent calls `complete_task`
- **THEN** the run ends `completed`, the PR is marked ready, and the task becomes `queued` so the next run addresses the message

### Requirement: Prompts during a run
A message queued while a run is active and not `finalizing` SHALL be sent to the runtime as a `Prompt` by the replica holding the run's stream. This includes a run that is `awaiting_input`; no new run SHALL start for it. A message queued while the run is `finalizing` (including after `Release` or `Cancel`) SHALL stay held back; the run end makes the task `queued` and the next run delivers it.

#### Scenario: Follow-up during a turn
- **WHEN** a message is queued on a running task
- **THEN** the runtime receives it as a `Prompt` and the transcript later shows it as a `UserMessage`

#### Scenario: Follow-up to an idle run
- **WHEN** a message is queued while the task's run is `awaiting_input`
- **THEN** the runtime receives it as a `Prompt`, the run and task become `running`, and no sandbox is provisioned

#### Scenario: Message right after mark done
- **WHEN** a board member marks an idle task done and queues a message before the cancelled run has ended
- **THEN** the message is not sent to that run, the task becomes `queued` when the run ends, and a new run delivers it

### Requirement: Idle deadline
The control plane SHALL end an `awaiting_input` run by sending `Release` once the idle deadline passes: the time of the run's last persisted `TurnEnded` plus `runs.keepAliveSeconds` (default 600; `0` releases right after the turn). The replica holding the run's stream SHALL send it; a replica that takes over the stream SHALL compute the same deadline. After `Release`, no prompt SHALL be sent on that run; prompts queued later start the next run.

#### Scenario: Default window
- **WHEN** a run is `awaiting_input` in an instance without `runs.keepAliveSeconds` and 10 minutes pass after its turn end
- **THEN** the control plane sends `Release` and the run ends `end_turn`

#### Scenario: Reconnect after the deadline
- **WHEN** the runtime reconnects to another replica after the deadline has passed
- **THEN** that replica sends `Release` right after the handshake

#### Scenario: Message after release
- **WHEN** a message is queued after `Release` was sent but before the run ended
- **THEN** the message is not sent to that run and the task becomes `queued` when the run ends

### Requirement: Deadline for blocking questions
For a run whose last turn had a blocking request, the control plane SHALL apply the idle deadline of "Idle deadline" (`iter1-keep-alive-and-review`) and send `Release` with outcome `BLOCKED` expected. The wall-clock budget includes time spent awaiting input.

#### Scenario: Default window
- **WHEN** a run awaits an answer in an instance without `runs.keepAliveSeconds`
- **THEN** the control plane sends `Release` after 10 minutes and the run ends `blocked`
