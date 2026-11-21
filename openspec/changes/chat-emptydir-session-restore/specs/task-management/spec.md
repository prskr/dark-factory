# Spec Delta

## MODIFIED Requirements

### Requirement: Volume retention
Once per hour the control plane SHALL delete the volumes of tasks of kind `task` without a non-terminal run that are `completed` or `cancelled` with their last run older than `volumes.retainAfterCloseSeconds` (default 7 days), or whose last run is older than `volumes.idleTtlSeconds` (default 30 days), and SHALL remember the deletion so it is not repeated. In the same scan it SHALL delete the stored session state of chats without a non-terminal run that were archived longer than `volumes.retainAfterCloseSeconds` ago, or whose last run is older than `volumes.idleTtlSeconds`.

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
- **THEN** the session state is deleted within the next hour, no volume is deleted and no lifecycle event is emitted

#### Scenario: Unarchived after retention
- **WHEN** the session state of archived `DF-12-CHAT` was deleted and a board member unarchives it and sends a message
- **THEN** the next run starts a new harness session with the transcript summary

### Requirement: Reset workspace
A board member SHALL be able to delete the volume of a task of kind `task` without a non-terminal run. The action SHALL write an audit entry `task.reset_workspace` and SHALL be rejected while a run is active. For a chat it SHALL be rejected with a conflict, because a chat has no volume.

#### Scenario: Reset a waiting task
- **WHEN** a board member resets the workspace of `DF-3` in status `waiting`
- **THEN** its PVC is deleted, `task.reset_workspace` is audited and the next run clones afresh and restores the WIP snapshot if one exists

#### Scenario: Reset rejected while running
- **WHEN** a board member resets the workspace of a task with a run in `preparing`
- **THEN** the action is rejected and the volume is kept

#### Scenario: Reset of a chat
- **WHEN** a board member posts a workspace reset for `DF-12-CHAT`
- **THEN** the request is rejected with a conflict, nothing is deleted and no audit entry is written
