# Spec Delta

## MODIFIED Requirements

### Requirement: Run provisioning
Starting a run SHALL ensure the task volume exists for a task with a writable workspace, then provision a sandbox for the run with the project's image and labels for run, task, project and persona, and move the run to `provisioning`, then `connecting` once the sandbox reports its pod IP. A run in `pending` or `provisioning` for more than 1 minute without a sandbox SHALL be provisioned again on the next tick; volume and sandbox creation SHALL be idempotent.

#### Scenario: Provisioning fails
- **WHEN** the provider rejects the sandbox for a reason other than capacity or a temporarily unresolvable cluster target (e.g. invalid image reference)
- **THEN** the run ends with outcome `provisioning_failed` and the task is resumed or failed per "Automatic resume"

#### Scenario: Quota exceeded
- **WHEN** the provider rejects the sandbox because a quota is exceeded
- **THEN** the run ends `provisioning_failed` with detail `capacity`, the task becomes `queued` with a resume time `scheduling.resumeBackoffSeconds` ahead (default 2 minutes), and the resume count is unchanged

#### Scenario: Provisioning interrupted
- **WHEN** a replica stops after creating run 3 of `DF-7` and before creating its Sandbox
- **THEN** another tick provisions the run within 2 minutes and the run connects normally

#### Scenario: Read-only workspace
- **WHEN** a run of `DF-12-CHAT` starts
- **THEN** no task volume is ensured and the sandbox is provisioned directly

### Requirement: Idle deadline
The control plane SHALL end an `awaiting_input` run by sending `Release` once the idle deadline passes: the time of the run's last persisted `TurnEnded` plus the run's keep-alive window (requirement "Keep-alive window by task kind"). The replica holding the run's stream SHALL send it; a replica that takes over the stream SHALL compute the same deadline. After `Release`, no prompt SHALL be sent on that run; prompts queued later start the next run.

#### Scenario: Default window
- **WHEN** a run is `awaiting_input` in an instance without `runs.keepAliveSeconds` and 10 minutes pass after its turn end
- **THEN** the control plane sends `Release` and the run ends `end_turn`

#### Scenario: Default chat window
- **WHEN** a run of `DF-12-CHAT` is `awaiting_input` in an instance without `runs.chatKeepAliveSeconds`
- **THEN** it is not released after 10 minutes and is released 30 minutes after its turn end

#### Scenario: Message within the chat window
- **WHEN** a board member sends a message to `DF-12-CHAT` 20 minutes after its last turn end
- **THEN** the message starts the next turn in the same run and pod, without provisioning and without a restore

#### Scenario: Reconnect after the deadline
- **WHEN** the runtime reconnects to another replica after the deadline has passed
- **THEN** that replica sends `Release` right after the handshake

#### Scenario: Message after release
- **WHEN** a message is queued after `Release` was sent but before the run ended
- **THEN** the message is not sent to that run and the task becomes `queued` when the run ends

### Requirement: Assign for chat runs
`Assign` of a chat run SHALL contain only the tools a chat run is offered and carry the persona's system prompt followed by the chat instructions. Its workspace follows the read-only workspace property, which is true for every chat.

#### Scenario: Assign for DF-12-CHAT
- **WHEN** a run of `DF-12-CHAT` connects
- **THEN** `Assign.workspace` names the base branch, has no task or WIP branch and is read-only, and `Assign.tools` contains `create_task` but not `complete_task`

## ADDED Requirements

### Requirement: Keep-alive window by task kind
The keep-alive window of a run SHALL be `runs.chatKeepAliveSeconds` (optional, default 1800) for a chat and `runs.keepAliveSeconds` (optional, default 600) for every other task, whether its workspace is read-only or not. A value of `0` SHALL release the run right after the turn.

#### Scenario: Chat window
- **WHEN** `runs.chatKeepAliveSeconds` is 900 and a run of `DF-12-CHAT` ends a turn
- **THEN** its idle deadline is 15 minutes after the turn end

#### Scenario: Other task
- **WHEN** `runs.chatKeepAliveSeconds` is 900 and a run of `DF-1` ends a turn in an instance without `runs.keepAliveSeconds`
- **THEN** its idle deadline is 10 minutes after the turn end

### Requirement: Assign for a read-only workspace
`Assign.workspace.read_only` SHALL be the read-only workspace property of the run's task. For a read-only workspace, `Assign.workspace` SHALL contain the base branch and no task or WIP branch. The control plane SHALL NOT derive it from the task key.

#### Scenario: Read-only property set
- **WHEN** a run of a task whose read-only workspace property is true connects
- **THEN** `Assign.workspace` is read-only, names the base branch and has no task or WIP branch

#### Scenario: Writable workspace
- **WHEN** run 1 of `DF-1` connects
- **THEN** `Assign.workspace.read_only` is false and `task_branch` is `factory/task/DF-1`

### Requirement: Session state storage
When the `RunReport` of a run whose task has a read-only workspace carries a session state of at most 8 MiB, the control plane SHALL store it as the task's only session state, keyed by the task, together with the run and the reported harness session id, replacing any earlier one. It SHALL NOT parse the content and SHALL NOT depend on the task key format. Reports of runs with a writable workspace SHALL NOT store session state.

#### Scenario: Overwritten at run end
- **WHEN** runs 1 and 2 of `DF-12-CHAT` each report a session state
- **THEN** only the session state of run 2 is stored

### Requirement: Session state cleared
When the `RunReport` of a run with a read-only workspace sets `session_state` with an empty archive, or with one above 8 MiB, the control plane SHALL delete the task's stored session state; above 8 MiB it SHALL also log a warning. A report that leaves `session_state` unset, and a run that ends without a report, SHALL leave the stored session state unchanged.

#### Scenario: Report with an empty archive
- **WHEN** run 3 of `DF-12-CHAT` reports `session_state` with an empty archive
- **THEN** the task has no stored session state

#### Scenario: Harness never started
- **WHEN** run 3 of `DF-12-CHAT` fails during prepare and reports `session_state` unset
- **THEN** the session state of run 2 stays stored

#### Scenario: Oversized report
- **WHEN** the report of a run with a read-only workspace carries 9 MiB of session state
- **THEN** nothing is stored, the earlier session state is deleted and a warning is logged

### Requirement: Session state in Assign
`Assign.continuation.session_state` of a run whose task has a read-only workspace SHALL carry the stored session state unless a run of the task newer than the one that reported it has a persisted `turn_started` transcript event; then `harness_session_id` SHALL be the id stored with it. Otherwise both SHALL be empty, without a fallback to an earlier run's or the task's session id. `session_state` SHALL be empty for runs with a writable workspace.

#### Scenario: Next run resumes
- **WHEN** run 2 of `DF-12-CHAT` stored session state with session id S and run 3 connects
- **THEN** `Assign.continuation` carries that session state and session id S

#### Scenario: Early failure in between
- **WHEN** run 2 stored session state, run 3 failed during prepare without a turn, and run 4 connects
- **THEN** run 4's `Assign.continuation` carries run 2's session state and session id

#### Scenario: Harness started, failed before the first turn
- **WHEN** run 2 stored session state, run 3 started its harness and failed in session setup before its first turn, and run 4 connects
- **THEN** run 4's `Assign.continuation` carries run 2's session state and session id

#### Scenario: Lost run in between
- **WHEN** run 2 stored session state, run 3 has a `turn_started` event and was lost without a report, and run 4 connects
- **THEN** run 4's `Assign.continuation` has an empty `session_state` and an empty `harness_session_id`, and its summary includes the events of runs 1 to 3

#### Scenario: No snapshot, no session id
- **WHEN** an existing chat without a stored session state gets its first run after the upgrade
- **THEN** `Assign.continuation.harness_session_id` is empty, no `session/load` is sent, and the run writes a `context_compacted` notice

#### Scenario: Writable workspace
- **WHEN** run 2 of `DF-1` connects
- **THEN** `Assign.continuation.session_state` is empty
