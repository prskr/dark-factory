# transcript-storage Specification

## Purpose

Persists every transcript event of a task's runs in order, keeps binary content out of the event rows, and uses the transcript to confirm that queued messages reached the agent (ADR-0012, ADR-0017).

## Requirements

### Requirement: Events stored once in order
Each transcript event SHALL be stored once per (run, seq) with its schema version, kind and payload in canonical protobuf JSON, and SHALL receive a task-wide increasing id usable for ordering and replay.

#### Scenario: Duplicate delivery
- **WHEN** the same (run, seq) event arrives twice
- **THEN** exactly one row exists

#### Scenario: Ordering across runs
- **WHEN** run 2's events are stored after run 1's
- **THEN** every event of run 2 has a higher id than every event of run 1

### Requirement: Binary content in the blob store
Inline binary data in an event SHALL be moved to the blob store and replaced by its SHA-256 before the event is stored.

#### Scenario: Image in agent output
- **WHEN** an event contains an inline image
- **THEN** the stored payload holds `sha256` instead of `inline` and the blob store returns the bytes for that hash

### Requirement: Delivery confirmation
Persisting a `UserMessage` event with a prompt id SHALL mark that queued message delivered by this run in the same transaction.

#### Scenario: Prompt delivered
- **WHEN** a `UserMessage` with prompt id P is persisted for run R
- **THEN** message P is marked delivered by R and is no longer re-sent

### Requirement: Live notification
After each committed batch of events, a `transcript_event` notification with the task id and the highest new id SHALL be published.

#### Scenario: UI follows live
- **WHEN** events are persisted for task `DF-1`
- **THEN** subscribers of `DF-1` receive a notification with the highest new id
