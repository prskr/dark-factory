# Spec Delta

## ADDED Requirements

### Requirement: Requests closed on cancellation
When a task is cancelled, its pending requests SHALL be closed with reason `task-cancelled`.

#### Scenario: Cancelled with a pending approval
- **WHEN** a task with a pending `push-check-exception` request is cancelled
- **THEN** the request is `closed` with reason `task-cancelled` and no longer appears in the inbox
