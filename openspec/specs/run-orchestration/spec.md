# run-orchestration Specification

## Purpose

Decides when a task gets a run, drives each run through its states, and turns the run's end into the task's status — sequential runs per task, sandboxes that never outlive their run (ADR-0016).

## Requirements

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

### Requirement: Resumed runs prompt the agent to continue
When a run starts because the task's resume time passed (rate limit, automatic resume) and the task has no undelivered message, the control plane SHALL queue a system prompt (reason `run.resumed`, "Continue the task where the previous run stopped.") in the same transaction, so the run has a turn instead of ending idle. It SHALL NOT change the task's resume count.

#### Scenario: Due rate-limited task continues
- **WHEN** task `DF-2` is `rate_limited` with a resume time that has passed and all its messages were delivered
- **THEN** a run starts and its first prompt is the `run.resumed` prompt

#### Scenario: Resumed task with a queued message
- **WHEN** task `DF-3` is `queued` with a resume time that has passed and an undelivered message
- **THEN** a run starts with that message and no `run.resumed` prompt is queued

### Requirement: Concurrent run limit
At most `runs.maxConcurrent` runs (default 4) SHALL be non-terminal at a time, counted across all replicas. Due tasks beyond the limit SHALL keep their status until a slot frees and SHALL then start oldest resume time first.

#### Scenario: Limit reached
- **WHEN** `runs.maxConcurrent` is 2, two runs are active and `DF-4` becomes queued
- **THEN** `DF-4`'s run starts only after one of the active runs ended

#### Scenario: Oldest first
- **WHEN** one slot frees and `DF-5` (due since 02:00) and `DF-6` (due since 03:00) are waiting
- **THEN** `DF-5`'s run starts first

#### Scenario: Two replicas, one slot
- **WHEN** `runs.maxConcurrent` is 1 and two replicas tick at the same time with `DF-1` and `DF-2` both due
- **THEN** exactly one run is created

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

### Requirement: Provisioning waits for cluster targets
When a cluster target of the run is temporarily unresolvable and has no earlier resolution on this replica, the sandbox SHALL NOT be created and the run SHALL stay `provisioning` until a later attempt succeeds or the connect timeout ends it.

#### Scenario: Cluster target temporarily unresolvable
- **WHEN** run 1 of `DF-1` is provisioned, project `DF` has cluster target `nativelink/scheduler`, the Service lookup times out and no earlier lookup of that target succeeded
- **THEN** no pod is created for the run, the run stays `provisioning`, and a warning naming `nativelink/scheduler` is logged

#### Scenario: Cluster target resolves on a later attempt
- **WHEN** the lookup of `nativelink/scheduler` succeeds on a later attempt within the connect timeout
- **THEN** the pod is created with the `hostAliases` entry for `scheduler.nativelink.svc` and the run connects normally

#### Scenario: Cluster target never resolves
- **WHEN** the lookup of `nativelink/scheduler` keeps failing until the connect timeout
- **THEN** the run ends `connect_timeout` and the task is resumed or failed per "Automatic resume after infrastructure failures"

### Requirement: Assign content
`Assign` SHALL contain the task reference, the workspace (repo URL through the gateway's Git proxy, base branch, task and WIP branch, the instance's commit identity), the persona's system prompt and ACP harness (adapter command, non-secret env, model, settings), the factory tools v2, budgets from instance configuration and the continuation. The adapter command SHALL be the project's, otherwise the instance's default.

#### Scenario: Assign for DF-1
- **WHEN** run 1 of `DF-1` connects
- **THEN** `Assign.workspace.task_branch` is `factory/task/DF-1` and `Assign.tools` contains exactly the seven factory tools v2, including `request_approval`

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
Every run after the first SHALL carry the harness session id reported by the previous run, and a summary of the task's transcript for harnesses that can't resume the session. The control plane SHALL build the summary when it assigns a run, from all transcript events of the task except the events of the run being assigned.

#### Scenario: Follow-up run
- **WHEN** run 2 of a task starts after run 1 reported session id `S`
- **THEN** `Assign.continuation.harness_session_id` is `S` and `summary` is not empty

#### Scenario: Old history beyond a row window
- **WHEN** a task has more than 500 transcript events and its entries fit the cap
- **THEN** the summary contains the entries of its oldest events

#### Scenario: Reconnect mid-run
- **WHEN** the sandbox of run 2 reconnects mid-run
- **THEN** the summary in the new `Assign` contains run 1's events and none of run 2's

#### Scenario: Run ended without a report
- **WHEN** run 1 ended through a lost sandbox or a hard timeout and run 2 starts
- **THEN** the summary includes run 1's events

### Requirement: Continuation summary content
The continuation summary SHALL contain the task title and description, the visible text of user messages (blocks marked `hidden` are left out), the agent text messages and one line per tool call (final title and status), in transcript order. Two or more consecutive successful tool calls SHALL be collapsed into one line with their count.

#### Scenario: Tool calls in the summary
- **WHEN** run 1 executed a tool call titled `cargo test` that ended `failed` and run 2 starts
- **THEN** `Assign.continuation.summary` contains a line with `cargo test` and `failed`

#### Scenario: Successful tool calls collapsed
- **WHEN** run 1 executed 37 consecutive tool calls that all ended `completed` and run 2 starts
- **THEN** the summary contains one line `Tool: 37 calls completed` instead of 37 lines

#### Scenario: Failed call breaks a streak
- **WHEN** a failed tool call lies between two groups of completed calls
- **THEN** the summary has one collapsed line per group and the failed call as its own line between them

#### Scenario: Agent messages of two runs stay separate
- **WHEN** the last agent message of run 1 and the first agent message of run 2 follow each other and carry the same message id
- **THEN** the summary has two agent entries

#### Scenario: Hidden blocks
- **WHEN** a user message has a hidden block
- **THEN** the summary does not contain its text

### Requirement: Continuation summary size cap
The continuation summary SHALL stay within a fixed size cap. When the cap is exceeded, tool lines and agent messages SHALL be dropped oldest first before any user message is dropped. The newest entry SHALL always be kept (shortened if it alone exceeds the cap), exempt from the drop order.

#### Scenario: User messages kept at the cap
- **WHEN** the entries exceed the size cap and include tool lines and agent messages older than a user message
- **THEN** those tool lines and agent messages are dropped before the user message

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
The control plane SHALL fail a run that doesn't connect within the configured connect timeout (outcome `connect_timeout`), SHALL send `Cancel` when the wall-clock budget plus grace is exceeded, and SHALL fail a non-terminal run whose sandbox disappears (outcome `sandbox_lost`), including a sandbox that disappeared while no control plane replica was running. Failed runs' sandboxes SHALL be torn down.

#### Scenario: Sandbox never connects
- **WHEN** a sandbox doesn't open the stream within the connect timeout
- **THEN** the run fails with `connect_timeout`, the sandbox is deleted and the task is resumed or failed per "Automatic resume after infrastructure failures"

#### Scenario: Pod deleted mid-run
- **WHEN** the sandbox pod of a running run is deleted
- **THEN** the run fails with `sandbox_lost` and the task volume is kept

#### Scenario: Pod deleted while the control plane was down
- **WHEN** the sandbox pod of running run 2 of `DF-4` is deleted while no control plane replica runs, and a replica starts afterwards
- **THEN** within one minute of the replica's start the run fails with `sandbox_lost` with a reason stating that the sandbox pod is gone, and the task is resumed or failed per "Automatic resume after infrastructure failures"

#### Scenario: Pod still there after a restart
- **WHEN** a replica starts while the sandbox pod of running run 2 of `DF-4` still exists
- **THEN** the run is not ended

#### Scenario: Sandbox check fails after a restart
- **WHEN** a replica starts after the sandbox pod of running run 2 of `DF-4` was deleted, and the first check for the pod fails because the Kubernetes API is unavailable
- **THEN** the check is retried and the run fails with `sandbox_lost` once the API answers, within the 3 attempts of the startup check

### Requirement: Run end maps to task status
On `RunReport` the control plane SHALL store outcome, detail, retry time, session id and workspace state, delete the sandbox, reset the task's resume count, and set the task status: `completed`→`in_review`, `end_turn`→`waiting`, `blocked`→`blocked`, `limit_reached`→`limit_reached`, `rate_limited`→`rate_limited` with the resume time set to the reported retry time or one hour ahead when none was reported, `cancelled`→`waiting`, `failed`→`failed`.

#### Scenario: Rate limit
- **WHEN** a run reports `RUN_OUTCOME_RATE_LIMITED` with retry time 04:30
- **THEN** the run is `succeeded` with outcome `rate_limited`, the task is `rate_limited` with resume time 04:30, and the next run starts at 04:30 without a message

#### Scenario: Unknown retry time
- **WHEN** a run reports `RUN_OUTCOME_RATE_LIMITED` without a retry time at 02:00
- **THEN** the task's resume time is 03:00

#### Scenario: Report resets the counter
- **WHEN** a task resumed twice automatically ends a run with `RUN_OUTCOME_END_TURN`
- **THEN** its resume count is 0

#### Scenario: Agent completes
- **WHEN** a run ends with outcome `completed`
- **THEN** the task becomes `in_review`

#### Scenario: Blocked run
- **WHEN** a run reports `RUN_OUTCOME_BLOCKED`
- **THEN** the task becomes `blocked` and no new run starts until a message is queued

### Requirement: Run end with undelivered prompts or a human status
If undelivered prompts exist at run end and the outcome is neither `failed` nor `rate_limited`, the task SHALL become `queued` instead of the mapped status; this also applies to a task a human marked done. For a run stopped by a human, only prompts queued after the stop count. Without undelivered prompts, a task a human marked done (`completed`) SHALL keep its status, whatever the outcome. A task whose status is still `cancelled` SHALL stay `cancelled`.

#### Scenario: Message arrived during finalize
- **WHEN** a message is queued while a run is finalizing and the run ends with `end_turn`
- **THEN** the task becomes `queued` and the next run starts with that message

#### Scenario: Marked done while idle
- **WHEN** a board member marks a task done while its run is `awaiting_input`
- **THEN** the run is cancelled, its sandbox deleted, and the task stays `completed`

#### Scenario: Completed with an undelivered message
- **WHEN** a human queues a message during the turn in which the agent calls `complete_task`
- **THEN** the run ends `completed`, the PR is marked ready, and the task becomes `queued` so the next run addresses the message

#### Scenario: Stopped with an undelivered message
- **WHEN** a message queued before the stop is still undelivered when the stopped run's `RunReport` arrives
- **THEN** the task becomes `waiting` and no new run starts until another message is queued

#### Scenario: Reopened while the stopped run finalizes
- **WHEN** a task is cancelled, a board member queues a message before the stopped run's `RunReport` arrives, and the run then reports `RUN_OUTCOME_CANCELLED`
- **THEN** the task stays `queued` and the next run starts with that message once the stopped run has ended

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

### Requirement: Prompts during a run
A message queued while a run is active and not `finalizing` SHALL be sent to the runtime as a prompt by the replica holding the run's stream. This includes a run that is `awaiting_input`; no new run SHALL start for it. A message queued while the run is `finalizing` (including after `Release` or `Cancel`) SHALL stay held back; the run end makes the task `queued` and the next run delivers it.

#### Scenario: Follow-up during a turn
- **WHEN** a message is queued on a running task
- **THEN** the runtime receives it as a prompt and the transcript later shows it as a `UserMessage`

#### Scenario: Follow-up to an idle run
- **WHEN** a message is queued while the task's run is `awaiting_input`
- **THEN** the runtime receives it as a prompt, the run and task become `running`, and no sandbox is provisioned

#### Scenario: Message right after mark done
- **WHEN** a board member marks an idle task done and queues a message before the cancelled run has ended
- **THEN** the message is not sent to that run, the task becomes `queued` when the run ends, and a new run delivers it

### Requirement: Released messages sent together
Messages released to a run at the same time (for example the held-back messages at a turn end) SHALL be sent to the runtime together in one control message, in queue order.

#### Scenario: Held-back messages form one turn
- **WHEN** two messages are queued during a turn and the turn ends
- **THEN** the runtime receives both in one control message and the next turn's `TurnStarted.prompt_ids` lists both

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

### Requirement: Stopping a run
A board member SHALL be able to stop a non-terminal run. A connected run SHALL receive `Cancel`; a run that isn't connected yet SHALL have its sandbox torn down and end with outcome `cancelled`. Stopping SHALL write an audit entry `run.stop`. A run that is already `finalizing` is not stopped again: the request changes nothing, so messages queued before it still make the task `queued` at run end.

#### Scenario: Stop a running run
- **WHEN** a board member stops run 2 of `DF-9` during a turn
- **THEN** the runtime receives `Cancel`, finalizes and reports `RUN_OUTCOME_CANCELLED`, and `DF-9` becomes `waiting`

#### Scenario: Stop while provisioning
- **WHEN** a board member stops a run in state `provisioning`
- **THEN** the sandbox is deleted, the run ends `cancelled` and the task volume is kept

#### Scenario: Stop while finalizing
- **WHEN** a board member stops a run that is `finalizing` after `complete_task` while a message queued before is undelivered
- **THEN** nothing changes, and the task becomes `queued` when the run ends `completed`

### Requirement: Stop timeout
If no `RunReport` arrives within 2 minutes after `Cancel` was sent for a human stop, the control plane SHALL tear down the sandbox and end the run `cancelled` with detail `stop-timeout`, keeping the task volume.

#### Scenario: Runtime hangs
- **WHEN** the runtime doesn't report within 2 minutes after a stop
- **THEN** the sandbox is deleted and the run ends `cancelled` with detail `stop-timeout`

### Requirement: Interrupting a turn from the control plane
A board member allowed to stop a run SHALL be able to interrupt its turn. If the run is `running` and its last persisted turn event is a `TurnStarted`, the control plane SHALL audit `run.interrupt` and the replica holding the run's stream SHALL send `Interrupt` with the board member's name and the turn's id; nothing SHALL be stored. Otherwise nothing changes and the answer is "No turn is running". An interrupt SHALL NOT use an automatic resume or change the task's status.

#### Scenario: Interrupt a running turn
- **WHEN** a board member interrupts the turn of `DF-9`'s run, which is `running`
- **THEN** the runtime receives `Interrupt`, an audit entry `run.interrupt` names the board member, and the run stays active

#### Scenario: No turn running
- **WHEN** a board member interrupts `DF-9` while its run is `awaiting_input`
- **THEN** the board member sees "No turn is running" and nothing is sent or audited

#### Scenario: Turn ended, run still running
- **WHEN** a board member interrupts `DF-9` after the turn's `TurnEnded` was stored, while the run is still `running` because the runtime pushes
- **THEN** the board member sees "No turn is running" and nothing is sent or audited

#### Scenario: Runtime not connected
- **WHEN** a board member interrupts a `running` run whose runtime is reconnecting
- **THEN** no `Interrupt` reaches the runtime later, and the turn continues

#### Scenario: Held-back message after an interrupt
- **WHEN** a board member queues "use the other API" during a turn and then interrupts it
- **THEN** the message is sent to the runtime at the turn end and starts the next turn of the same run

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
