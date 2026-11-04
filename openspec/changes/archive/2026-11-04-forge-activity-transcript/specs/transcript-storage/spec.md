# Spec Delta

## MODIFIED Requirements

### Requirement: Events stored once in order
Each transcript event SHALL be stored once with its schema version, kind and payload in canonical protobuf JSON, and SHALL receive a task-wide increasing id usable for ordering and replay. A runtime event SHALL be identified by (run, seq); an event written by the control plane SHALL be identified by (run, source key) and has no seq. Exactly one of seq and source key SHALL be set.

#### Scenario: Duplicate delivery
- **WHEN** the same (run, seq) event arrives twice
- **THEN** exactly one row exists

#### Scenario: Duplicate control-plane event
- **WHEN** the control plane writes an event with the same (run, source key) twice
- **THEN** exactly one row exists

#### Scenario: Ordering across runs
- **WHEN** run 2's events are stored after run 1's
- **THEN** every event of run 2 has a higher id than every event of run 1

#### Scenario: Control-plane event between runtime events
- **WHEN** the control plane writes an event while run R's runtime events are being stored
- **THEN** it gets an id from the same task-wide order and appears between the events stored before and after it

#### Scenario: Ids become visible in order
- **WHEN** a control-plane event and a runtime batch of the same task are stored concurrently
- **THEN** no event becomes visible after an event with a higher id, so a live view resuming from the highest id it has seen misses none

### Requirement: Live notification
After each committed batch of events, a `transcript_event` notification with the task id and the highest new id SHALL be published. This SHALL also apply to events written by the control plane.

#### Scenario: UI follows live
- **WHEN** events are persisted for task `DF-1`
- **THEN** subscribers of `DF-1` receive a notification with the highest new id

#### Scenario: Control-plane event live
- **WHEN** the control plane writes a forge activity event for task `DF-1`
- **THEN** subscribers of `DF-1` receive a notification with its id
