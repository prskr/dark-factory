# Spec Delta

## MODIFIED Requirements

### Requirement: Idle deadline
The control plane SHALL end an `awaiting_input` run by sending `Release` once the idle deadline passes: the time of the run's last persisted `TurnEnded` plus `runs.keepAliveSeconds` (default 600) for a task of kind `task`, or plus `runs.chatKeepAliveSeconds` (default 1800) for a chat; `0` releases right after the turn. The replica holding the run's stream SHALL send it; a replica that takes over the stream SHALL compute the same deadline. After `Release`, no prompt SHALL be sent on that run; prompts queued later start the next run.

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

## ADDED Requirements

### Requirement: Chat session state storage
When a chat run's `RunReport` carries a session state of at most 8 MiB, the control plane SHALL store it as the chat's only session state, together with the run and the reported harness session id, replacing any earlier one. When the report carries none, or one above 8 MiB, it SHALL delete the chat's stored session state; above 8 MiB it SHALL log a warning. It SHALL NOT parse the content. A run that ends without a report SHALL leave the stored session state unchanged.

#### Scenario: Overwritten at run end
- **WHEN** runs 1 and 2 of `DF-12-CHAT` each report a session state
- **THEN** only the session state of run 2 is stored

#### Scenario: Report without session state
- **WHEN** run 3 of `DF-12-CHAT` reports no session state
- **THEN** the chat has no stored session state

#### Scenario: Oversized report
- **WHEN** a chat run's report carries 9 MiB of session state
- **THEN** nothing is stored, the earlier session state is deleted and a warning is logged

### Requirement: Session state in Assign
`Assign.continuation.session_state` of a chat run SHALL carry the stored session state only if it was reported by the newest earlier run of the chat whose sandbox connected; then `harness_session_id` SHALL be the id stored with it. Otherwise it SHALL be empty. It SHALL be empty for tasks of kind `task`.

#### Scenario: Next run resumes
- **WHEN** run 2 of `DF-12-CHAT` stored session state with session id S and run 3 connects
- **THEN** `Assign.continuation` carries that session state and session id S

#### Scenario: Lost run in between
- **WHEN** run 2 stored session state, run 3 connected and was lost without a report, and run 4 connects
- **THEN** run 4's `Assign.continuation.session_state` is empty and its summary includes the events of runs 1 to 3

#### Scenario: Task run
- **WHEN** run 2 of `DF-1` connects
- **THEN** `Assign.continuation.session_state` is empty
