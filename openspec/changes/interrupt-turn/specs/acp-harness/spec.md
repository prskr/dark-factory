## ADDED Requirements

### Requirement: Interrupting a turn
On `Interrupt` during a turn, the runtime SHALL send ACP `session/cancel`, end the turn with `TurnEnded` stop reason `CANCELLED` and detail "Interrupted by <name>", and keep the run and the harness session. With prompts queued it SHALL start the next turn; otherwise it SHALL wait as in "Idle wait after a normal turn end". The next turn's input SHALL start with "Your previous turn was interrupted by <name>.". An `Interrupt` outside a turn SHALL be ignored.

#### Scenario: Interrupt without waiting messages
- **WHEN** the runtime receives `Interrupt` from "Peter Kurfer" during a turn and no prompt is queued
- **THEN** the turn ends with stop reason `CANCELLED` and detail "Interrupted by Peter Kurfer", and the runtime reports `RUN_PHASE_AWAITING_INPUT`

#### Scenario: Interrupt with a waiting message
- **WHEN** a prompt is queued and the runtime then receives `Interrupt` during the turn
- **THEN** the turn ends and the next turn starts with "Your previous turn was interrupted by Peter Kurfer." followed by the queued prompt

#### Scenario: Interrupt while idle
- **WHEN** the runtime receives `Interrupt` while it waits for input
- **THEN** nothing changes and the run keeps waiting

#### Scenario: Open Dark Factory tool call
- **WHEN** the runtime receives `Interrupt` while a Dark Factory tool call of the turn waits for its result
- **THEN** the tool call gets a cancelled result, and whatever the control plane already did for it stays done

#### Scenario: Interrupted turn and budgets
- **WHEN** a turn is interrupted in a run with `max_turns` 3
- **THEN** the interrupted turn counts as one of the 3 turns and the run does not end with a failure outcome
