# Spec Delta

## ADDED Requirements

### Requirement: Volume retention
Once per hour the control plane SHALL delete the volumes of tasks without a non-terminal run that are `completed` or `cancelled` with their last run older than `volumes.retainAfterClose` (default 7 days), or whose last run is older than `volumes.idleTtl` (default 30 days), and SHALL remember the deletion so it is not repeated.

#### Scenario: Closed task after the grace period
- **WHEN** `DF-1` has been `completed` for 8 days
- **THEN** its volume is deleted within the next hour

#### Scenario: Idle task
- **WHEN** `DF-2` is `waiting` and its last run ended 31 days ago
- **THEN** its volume is deleted and queueing a message later starts a run on a fresh volume

#### Scenario: Active run is skipped
- **WHEN** a task matches the idle TTL but has a run in `running`
- **THEN** its volume is kept

### Requirement: Reset workspace
A board member SHALL be able to delete the volume of a task without a non-terminal run. The action SHALL write an audit entry `task.reset_workspace` and SHALL be rejected while a run is active.

#### Scenario: Reset a waiting task
- **WHEN** a board member resets the workspace of `DF-3` in status `waiting`
- **THEN** its PVC is deleted, `task.reset_workspace` is audited and the next run clones afresh and restores the WIP snapshot if one exists

#### Scenario: Reset rejected while running
- **WHEN** a board member resets the workspace of a task with a run in `preparing`
- **THEN** the action is rejected and the volume is kept
