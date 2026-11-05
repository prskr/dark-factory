## MODIFIED Requirements

### Requirement: Prompts during a run
A message queued while a run is active and not `finalizing` SHALL be sent to the runtime as a prompt by the replica holding the run's stream. Messages released at the same time (for example the held-back messages at a turn end) SHALL be sent together in one control message, in queue order. This includes a run that is `awaiting_input`; no new run SHALL start for it. A message queued while the run is `finalizing` (including after `Release` or `Cancel`) SHALL stay held back; the run end makes the task `queued` and the next run delivers it.

#### Scenario: Follow-up during a turn
- **WHEN** a message is queued on a running task
- **THEN** the runtime receives it as a prompt and the transcript later shows it as a `UserMessage`

#### Scenario: Held-back messages form one turn
- **WHEN** two messages are queued during a turn and the turn ends
- **THEN** the runtime receives both in one control message and the next turn's `TurnStarted.prompt_ids` lists both

#### Scenario: Follow-up to an idle run
- **WHEN** a message is queued while the task's run is `awaiting_input`
- **THEN** the runtime receives it as a prompt, the run and task become `running`, and no sandbox is provisioned

#### Scenario: Message right after mark done
- **WHEN** a board member marks an idle task done and queues a message before the cancelled run has ended
- **THEN** the message is not sent to that run, the task becomes `queued` when the run ends, and a new run delivers it
