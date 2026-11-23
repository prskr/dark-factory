# Spec Delta

## MODIFIED Requirements

### Requirement: Volume provisioned event
The control plane SHALL emit `volume.provisioned` exactly once per volume creation, when a run of a task with a writable workspace is scheduled and the task's volume does not exist. A volume created again after `volume.destroyed` SHALL emit it again. The event SHALL carry the run whose scheduling created the volume.

#### Scenario: First run creates the volume
- **WHEN** the first run of task `DF-42` is scheduled and the volume `task-df-42` does not exist yet
- **THEN** one `volume.provisioned` event for `DF-42` with that run is emitted

#### Scenario: Continuation run
- **WHEN** a later run of `DF-42` is scheduled and the volume already exists
- **THEN** no `volume.provisioned` event is emitted

#### Scenario: Volume created again after a reset
- **WHEN** the workspace of `DF-42` was reset (`volume.destroyed` with reason `reset`) and the next run of `DF-42` is scheduled
- **THEN** one `volume.provisioned` event for `DF-42` with that run is emitted

## ADDED Requirements

### Requirement: No volume events for read-only workspaces
The control plane SHALL NOT emit `volume.provisioned` or `volume.destroyed` for a task with a read-only workspace, because such a task has no task volume. `sandbox.*` events SHALL be emitted for its runs as for other runs.

#### Scenario: Chat run scheduled
- **WHEN** the first run of `DF-12-CHAT` is scheduled
- **THEN** a `sandbox.provisioning` event is emitted and no `volume.provisioned` event

#### Scenario: Chat run torn down
- **WHEN** a run of `DF-12-CHAT` ends and its pod is deleted
- **THEN** no `volume.destroyed` event is emitted
