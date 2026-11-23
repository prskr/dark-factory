# Spec Delta

## ADDED Requirements

### Requirement: Read-only workspace property
Every task SHALL have a read-only workspace property, stored on the task. It SHALL be true for every chat and false for every other task. The Git route of a run, `Assign.workspace.read_only`, the data volume of the run's pod and the session state SHALL follow this property, not the task key or kind.

#### Scenario: Chat
- **WHEN** a board member starts `DF-25-CHAT`
- **THEN** its read-only workspace property is true

#### Scenario: Task
- **WHEN** a task `DF-26` is created
- **THEN** its read-only workspace property is false

#### Scenario: Existing chats after the upgrade
- **WHEN** the migration runs on an instance with chat `DF-12-CHAT` and task `DF-1`
- **THEN** `DF-12-CHAT` has a read-only workspace and `DF-1` doesn't

## MODIFIED Requirements

### Requirement: Volume retention
Once per hour the control plane SHALL delete the volumes of tasks with a writable workspace without a non-terminal run that are `completed` or `cancelled` with their last run older than `volumes.retainAfterCloseSeconds` (default 7 days), or whose last run is older than `volumes.idleTtlSeconds` (default 30 days), and SHALL remember the deletion so it is not repeated. It SHALL skip tasks with a read-only workspace and SHALL NOT delete their stored session state.

#### Scenario: Closed task after the grace period
- **WHEN** `DF-1` has been `completed` for 8 days
- **THEN** its volume is deleted within the next hour

#### Scenario: Idle task
- **WHEN** `DF-2` is `waiting` and its last run ended 31 days ago
- **THEN** its volume is deleted and queueing a message later starts a run on a fresh volume

#### Scenario: Active run is skipped
- **WHEN** a task matches the idle TTL but has a run in `running`
- **THEN** its volume is kept

#### Scenario: Archived chat
- **WHEN** `DF-12-CHAT` was archived 8 days ago and has a stored session state
- **THEN** the retention scan deletes nothing for it and emits no lifecycle event

### Requirement: Reset workspace
A board member SHALL be able to delete the volume of a task with a writable workspace and without a non-terminal run. The action SHALL write an audit entry `task.reset_workspace` and SHALL be rejected while a run is active. The action SHALL NOT be shown for a task with a read-only workspace.

#### Scenario: Reset a waiting task
- **WHEN** a board member resets the workspace of `DF-3` in status `waiting`
- **THEN** its PVC is deleted, `task.reset_workspace` is audited and the next run clones afresh and restores the WIP snapshot if one exists

#### Scenario: Reset rejected while running
- **WHEN** a board member resets the workspace of a task with a run in `preparing`
- **THEN** the action is rejected and the volume is kept

#### Scenario: No reset for a read-only workspace
- **WHEN** a board member opens `DF-12-CHAT`
- **THEN** no "Reset workspace" action is shown
