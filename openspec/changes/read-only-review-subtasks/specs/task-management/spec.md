# Spec Delta

## MODIFIED Requirements

### Requirement: Read-only workspace property
Every task SHALL have a read-only workspace property, stored on the task and fixed at creation. It SHALL be true for every chat and for every task created with `read_only: true` (`factory-tools`, "create_task for a read-only task"), and false for every other task. The Git route of a run, `Assign.workspace.read_only`, the data volume of the run's pod and the session state SHALL follow this property, not the task key or kind.

#### Scenario: Chat
- **WHEN** a board member starts `DF-25-CHAT`
- **THEN** its read-only workspace property is true

#### Scenario: Task
- **WHEN** a task `DF-26` is created
- **THEN** its read-only workspace property is false

#### Scenario: Read-only task
- **WHEN** the agent of `DF-19` creates `DF-27` with `base_task: "DF-20"` and `read_only: true`
- **THEN** `DF-27` has kind `task` and its read-only workspace property is true

#### Scenario: Existing chats after the upgrade
- **WHEN** the migration runs on an instance with chat `DF-12-CHAT` and task `DF-1`
- **THEN** `DF-12-CHAT` has a read-only workspace and `DF-1` doesn't

### Requirement: Reset workspace
A board member SHALL be able to delete the volume of a task with a writable workspace and without a non-terminal run. The action SHALL write an audit entry `task.reset_workspace` and SHALL be rejected while a run is active. The action SHALL NOT be shown for a task with a read-only workspace, and a request for one SHALL be rejected as a conflict without an audit entry.

#### Scenario: Reset a waiting task
- **WHEN** a board member resets the workspace of `DF-3` in status `waiting`
- **THEN** its PVC is deleted, `task.reset_workspace` is audited and the next run clones afresh and restores the WIP snapshot if one exists

#### Scenario: Reset rejected while running
- **WHEN** a board member resets the workspace of a task with a run in `preparing`
- **THEN** the action is rejected and the volume is kept

#### Scenario: No reset for a read-only workspace
- **WHEN** a board member opens `DF-12-CHAT`
- **THEN** no "Reset workspace" action is shown

#### Scenario: Reset request for a read-only task
- **WHEN** a request to reset the workspace of read-only task `DF-21` arrives
- **THEN** it is rejected as a conflict, nothing is deleted and no audit entry is written
