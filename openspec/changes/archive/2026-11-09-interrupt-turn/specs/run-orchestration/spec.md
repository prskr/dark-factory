## ADDED Requirements

### Requirement: Interrupting a turn from the control plane
A board member allowed to stop a run SHALL be able to interrupt its current turn. If the run is `running`, the control plane SHALL write an audit entry `run.interrupt` and the replica holding the run's stream SHALL send `Interrupt` with the board member's name. The request SHALL NOT be stored for later. Otherwise the request SHALL change nothing and answer "No turn is running". An interrupt SHALL NOT use an automatic resume or change the task's status by itself.

#### Scenario: Interrupt a running turn
- **WHEN** a board member interrupts the turn of `DF-9`'s run, which is `running`
- **THEN** the runtime receives `Interrupt`, an audit entry `run.interrupt` names the board member, and the run stays active

#### Scenario: No turn running
- **WHEN** a board member interrupts `DF-9` while its run is `awaiting_input`
- **THEN** the board member sees "No turn is running" and nothing is sent or audited

#### Scenario: Runtime not connected
- **WHEN** a board member interrupts a `running` run whose runtime is reconnecting
- **THEN** no `Interrupt` reaches the runtime later, and the turn continues

#### Scenario: Held-back message after an interrupt
- **WHEN** a board member queues "use the other API" during a turn and then interrupts it
- **THEN** the message is sent to the runtime at the turn end and starts the next turn of the same run
