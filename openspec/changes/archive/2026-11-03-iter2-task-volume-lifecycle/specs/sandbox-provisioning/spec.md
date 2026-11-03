# Spec Delta

## MODIFIED Requirements

### Requirement: Task volume
The control plane SHALL create one ReadWriteOnce PVC per task named `task-<key lower case>` in the sandbox namespace, using the configured StorageClass and size, before the task's first run and before any run that follows a volume deletion, and SHALL keep it after runs end.

#### Scenario: First run of DF-1
- **WHEN** run 1 of `DF-1` is provisioned
- **THEN** PVC `task-df-1` exists and is mounted at `/data`

#### Scenario: Second run reuses the volume
- **WHEN** run 2 of `DF-1` is provisioned
- **THEN** no new PVC is created and the same PVC is mounted

#### Scenario: Recreated on the next run
- **WHEN** the volume of `DF-1` was deleted and a message is queued on `DF-1`
- **THEN** a new PVC `task-df-1` is created before the run and the runtime clones afresh

## ADDED Requirements

### Requirement: Deleting a task volume
The provider SHALL delete a task's PVC on request. Deleting a volume that doesn't exist SHALL succeed. A volume SHALL only be deleted for tasks without a non-terminal run.

#### Scenario: Volume deleted
- **WHEN** the volume of `DF-1`, which has no active run, is deleted
- **THEN** PVC `task-df-1` no longer exists

#### Scenario: Already gone
- **WHEN** the volume of a task whose PVC was removed by hand is deleted
- **THEN** the call succeeds
