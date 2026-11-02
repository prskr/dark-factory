# Spec Delta

## ADDED Requirements

### Requirement: Stop and cancel actions
The task detail SHALL offer "Stop" for an active run and "Cancel task" for tasks that can be cancelled, on phone and desktop. "Cancel task" SHALL ask for confirmation. Both SHALL update the page live when the run and task change.

#### Scenario: Stop on a phone
- **WHEN** a board member taps "Stop" at 390 px width on a running task
- **THEN** the run shows as finalizing and then `cancelled` without a page reload

#### Scenario: Cancel needs confirmation
- **WHEN** a board member clicks "Cancel task"
- **THEN** nothing changes until the confirmation is accepted
