## MODIFIED Requirements

### Requirement: Run phases
The run state SHALL follow `pending → provisioning → connecting → preparing → running → finalizing → succeeded | failed | cancelled`, advanced by the provider events and the runtime's `PhaseChanged` and `RunReport` messages. Between turns a run SHALL move `running → awaiting_input → running` on `RUN_PHASE_AWAITING_INPUT` and `RUN_PHASE_RUNNING`. When a run becomes `awaiting_input`, its task SHALL become `waiting` unless undelivered messages exist, in which case it stays `running`; when the run is `running` again, the task SHALL be `running`. The phase update and `queue_message` SHALL serialize on the task row, so neither overwrites the other. Once the control plane has sent `Release` or `Cancel` to a run, the run SHALL be `finalizing`.

#### Scenario: Phase reported
- **WHEN** the runtime reports `RUN_PHASE_RUNNING`
- **THEN** the run state becomes `running` and a `run_changed` notification is published

#### Scenario: Idle after a turn
- **WHEN** the runtime reports `RUN_PHASE_AWAITING_INPUT` and no message is undelivered
- **THEN** the run state becomes `awaiting_input`, the sandbox keeps running and the task status becomes `waiting`

#### Scenario: Message queued as the turn ends
- **WHEN** a message is queued while the turn ends and the `RUN_PHASE_AWAITING_INPUT` report is processed afterwards
- **THEN** the run is `awaiting_input`, the task stays `running` and the message is forwarded

### Requirement: Run end maps to task status
On `RunReport` the control plane SHALL store outcome, detail, retry time, session id and workspace state, delete the sandbox, and set the task status: `completed`→`in_review`, `end_turn`→`waiting`, `limit_reached`→`limit_reached`, `rate_limited`→`rate_limited`, `cancelled`→`waiting`, `failed`→`failed`. If undelivered prompts exist and the outcome is neither `failed` nor `rate_limited`, the task SHALL become `queued` instead; this also applies to a task a human marked done. Without undelivered prompts, a task a human marked done (`completed`) SHALL keep its status, whatever the outcome.

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

## ADDED Requirements

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
