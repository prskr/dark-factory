## MODIFIED Requirements

### Requirement: Stop and cancel actions
The task detail SHALL offer "Interrupt" in the message box while a turn of the active run is running, "Stop run" for an active run in the status switch and in the run row, and "Cancel task" for tasks that can be cancelled, on phone and desktop. "Cancel task" SHALL ask for confirmation. All SHALL update the page live when the run and task change.

#### Scenario: Stop on a phone
- **WHEN** a board member taps "Stop run" at 390 px width on a running task
- **THEN** the run shows as finalizing and then `cancelled` without a page reload

#### Scenario: Interrupt only during a turn
- **WHEN** the agent finishes its turn and waits for input while the task page is open
- **THEN** the "Interrupt" button disappears from the message box without a page reload

#### Scenario: Interrupt without a turn
- **WHEN** a board member presses "Interrupt" just after the turn ended
- **THEN** the page shows the notice "No turn is running"

#### Scenario: Cancel needs confirmation
- **WHEN** a board member clicks "Cancel task"
- **THEN** nothing changes until the confirmation is accepted

## ADDED Requirements

### Requirement: Interrupted turns in the transcript
A turn that ended because of an interrupt SHALL end in the transcript with a row "Interrupted by <name>", using the name the interrupt was sent with.

#### Scenario: Interrupted turn
- **WHEN** a board member opens a task whose last turn "Peter Kurfer" interrupted
- **THEN** the transcript shows "Interrupted by Peter Kurfer" after the agent's last output of that turn
